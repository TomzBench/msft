//! ThreadpoolWait Create, Close, Set, Wait
//!
//! https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-closethreadpoolwait
//! https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-setthreadpoolwait
//! https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-waitforthreadpoolwaitcallbacks

use crate::common::{ThreadpoolCallbackEnvironment, WaitPending};
use parking_lot::Mutex;
use std::{
    error,
    ffi::c_void,
    fmt,
    future::Future,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{FILETIME, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        CloseThreadpoolWait, CreateThreadpoolWait, SetThreadpoolWait,
        WaitForThreadpoolWaitCallbacks, PTP_CALLBACK_INSTANCE, PTP_WAIT,
    },
};

/// Waiting on a waitable object will resolve with Ok or a WaitError
pub type WaitResult = Result<(), WaitError>;

/// When waiting on a waitable object. The wait may resolve with a wait error.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum WaitError {
    /// A caller signaled they are no longer interested in waiting for the wait object.
    Cancelled,
    /// The waitable object failed to complete before the specified timeout
    Timeout,
    /// Already waiting for the waitable object
    InProgress,
}

impl fmt::Display for WaitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WaitError::Timeout => write!(f, "wait timeout"),
            WaitError::Cancelled => write!(f, "wait cancelled"),
            WaitError::InProgress => write!(f, "wait already in progress"),
        }
    }
}

impl error::Error for WaitError {}

/// A handle to pool of workers who wait for wait objects . The context is also shared by the
/// futures and weakly by the kernel. The weak reference used by the kernel is guarenteed to be
/// valid because the threadpool will wait for all kernel callbacks to resolve prior to
/// dropping.
///
/// This is guarenteed because RFC 1857 specifying drop order. The threadpool is dropped first
/// and waits for callbacks to finish executing. Then the pointers are dropped.
///
/// Safety: DO NOT CHANGE ORDER IN STRUCT (RFC 1857)
pub struct WaitPool {
    /// A pool of workers to wait on waitable objects. See [`self::OwnedWaitHandle`]. NOTE the
    pool: OwnedWaitHandle,
    /// Shared state between the waitable worker callbacks and future waiting for event
    shared: Arc<Mutex<Shared>>,
    /// We track if we have started already so to panic if caller calls start more than once
    started: bool,
}

impl WaitPool {
    pub fn new() -> io::Result<Self> {
        let shared = Arc::new(Mutex::new(Shared::default()));
        OwnedWaitHandle::new(None, Arc::as_ptr(&shared) as _).map(|pool| Self {
            pool,
            shared,
            started: false,
        })
    }

    pub fn with_environment(env: &ThreadpoolCallbackEnvironment) -> io::Result<Self> {
        let shared = Arc::new(Mutex::new(Shared::default()));
        OwnedWaitHandle::new(Some(env), Arc::as_ptr(&shared) as _).map(|pool| Self {
            pool,
            shared,
            started: false,
        })
    }

    /// Return a future that resolve when the wait object is completed.
    ///
    /// Will panic if this is called more than once. To start listening to another wait object, use
    /// [`Self::restart`]
    ///
    /// See also [`OwnedWaitHandle::start`]
    pub fn start(&mut self, handle: HANDLE, timeout: Option<Duration>) -> WaitFuture {
        if !self.started {
            self.started = true;
            self.pool.start(handle, timeout);
            WaitFuture {
                shared: Arc::clone(&self.shared),
            }
        } else {
            panic!("Cannot start waiting more than once! use restart instead")
        }
    }

    /// Start a new wait for another waitable object. Will error if previous wait object is still
    /// in progress. Use [`Self::cancel`] to discard the old wait and start a new wait.
    ///
    /// See also [`OwnedWaitHandle::start`]
    pub fn restart(
        &self,
        handle: HANDLE,
        timeout: Option<Duration>,
    ) -> Result<WaitFuture, WaitError> {
        let mut shared = self.shared.lock();
        let _old = shared.result.take().ok_or(WaitError::InProgress)?;
        self.pool.start(handle, timeout);
        Ok(WaitFuture {
            shared: Arc::clone(&self.shared),
        })
    }

    /// Resolve any pending WaitFutures with a [`WaitError::Cancelled`]
    pub fn cancel(&self) -> &Self {
        self.pool.stop();
        self.shared
            .lock()
            .maybe_wake_with(Err(WaitError::Cancelled));
        self
    }
}

#[derive(Debug, Clone)]
pub struct WaitFuture {
    shared: Arc<Mutex<Shared>>,
}

impl Future for WaitFuture {
    type Output = WaitResult;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut shared = self.shared.lock();
        let new_waker = cx.waker();

        match shared.result {
            Some(result) => {
                // If a result is ready, wake executor with result
                if let Some(waker) = shared.waker.take() {
                    waker.wake()
                }
                Poll::Ready(result)
            }
            None => {
                // Update our waker
                shared.waker = match shared.waker.take() {
                    None => Some(new_waker.clone()),
                    Some(old_waker) => match old_waker.will_wake(new_waker) {
                        false => Some(new_waker.clone()),
                        true => Some(old_waker),
                    },
                };
                Poll::Pending
            }
        }
    }
}

#[derive(Default, Debug)]
pub struct Shared {
    waker: Option<Waker>,
    result: Option<WaitResult>,
}

impl Shared {
    fn maybe_wake_with(&mut self, result: WaitResult) {
        match self.result.replace(result) {
            Some(result) => self.result = Some(result),
            None => {
                if let Some(waker) = self.waker.take() {
                    waker.wake()
                }
            }
        }
    }
}

pub(in crate::wait) struct OwnedWaitHandle(PTP_WAIT);
impl Drop for OwnedWaitHandle {
    fn drop(&mut self) {
        self.stop();
        self.wait(WaitPending::Cancel);
        unsafe { CloseThreadpoolWait(self.0) }
    }
}

impl OwnedWaitHandle {
    /// https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-createthreadpoolwait
    pub(in crate::wait) fn new(
        maybe_env: Option<&ThreadpoolCallbackEnvironment>,
        cx: *mut c_void,
    ) -> io::Result<Self> {
        let env = maybe_env.map_or_else(std::ptr::null, |env| env as *const _ as _);
        let result = unsafe { CreateThreadpoolWait(Some(wait_callback), cx, env) };
        match result {
            0 => Err(io::Error::last_os_error()),
            handle => Ok(OwnedWaitHandle(handle)),
        }
    }

    /// Set a wait object on a threadpool which will trigger a wait callback
    ///
    /// https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-setthreadpoolwait
    pub(in crate::wait) fn start(&self, handle: HANDLE, timeout: Option<Duration>) {
        let ft = timeout
            .map(|to| {
                let ms = to.as_millis();
                &FILETIME {
                    dwHighDateTime: (ms >> 32) as u32,
                    dwLowDateTime: (ms & 0xFFFFFFFF) as u32,
                } as *const _
            })
            .unwrap_or_else(std::ptr::null);
        unsafe { SetThreadpoolWait(self.0, handle, ft) };
    }

    /// The wait object will cease to queue new callbacks. Callbacks already queued will still fire
    ///
    /// https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-setthreadpoolwait
    pub(in crate::wait) fn stop(&self) {
        let ft = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        // Handle == 0 pool will cease to queue new callbacks. existing callbacks still occur
        unsafe { SetThreadpoolWait(self.0, 0, &ft) };
    }

    /// Waits for outstanding wait callbacks to complete and optionally cancels pending callbacks
    /// that have not yet started to execute.
    ///
    /// https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-waitforthreadpoolwaitcallbacks
    pub(in crate::wait) fn wait(&self, pending: WaitPending) {
        unsafe { WaitForThreadpoolWaitCallbacks(self.0, pending as _) };
    }
}

unsafe extern "system" fn wait_callback(
    _instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    _wait: PTP_WAIT,
    waitresult: u32,
) {
    let state = &*(context as *const Mutex<Shared>);
    let mut shared = state.lock();
    shared.result = match waitresult {
        WAIT_OBJECT_0 => Some(Ok(())),
        WAIT_TIMEOUT => Some(Err(WaitError::Timeout)),
        _ => panic!("Unsupported kernel argument passed to wait callback!"),
    };
    if let Some(waker) = shared.waker.as_ref() {
        waker.wake_by_ref()
    }
}
