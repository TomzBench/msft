//! Threadpool Work
//! https://learn.microsoft.com/en-us/windows/win32/procthread/thread-pool-api
//!
//! ThreadpoolWork Create, Close, Submit, Wait
//! https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-createthreadpoolwork
//! https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-closethreadpoolwork
//! https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-submitthreadpoolwork
//! https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-waitforthreadpoolworkcallbacks

use crate::common::{ThreadpoolCallbackEnvironment, ThreadpoolCallbackInstance, WaitPending};
use parking_lot::Mutex;
use std::{
    cell::UnsafeCell,
    ffi::c_void,
    future::Future,
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, RawHandle},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};
use windows_sys::Win32::System::Threading::{
    CloseThreadpoolWork, CreateThreadpoolWork, SubmitThreadpoolWork,
    WaitForThreadpoolWorkCallbacks, PTP_CALLBACK_INSTANCE, PTP_WORK,
};

pub fn once<F, O>(workfn: F) -> io::Result<WorkOncePoolGuard<F>>
where
    F: FnOnce(ThreadpoolCallbackInstance) -> O,
{
    WorkOncePool::new(workfn).map(|pool| pool.submit_once())
}

/// A WorkOnceFn is called once by a threadpool work
pub trait WorkOnceFn {
    type Output;
    fn work_once(self, instance: ThreadpoolCallbackInstance) -> Self::Output;
}

impl<F, O> WorkOnceFn for F
where
    F: FnOnce(ThreadpoolCallbackInstance) -> O,
{
    type Output = O;
    fn work_once(self, instance: ThreadpoolCallbackInstance) -> Self::Output {
        (self)(instance)
    }
}

struct OwnedWorkHandle(PTP_WORK);
impl OwnedWorkHandle {
    /// Create a new threadpool handle
    ///
    /// https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-createthreadpoolwork
    ///
    /// Safety: env must point to an initialized ThreadpoolCallbackEnvironment, context must point
    /// to initialized data and live as long as the handle
    unsafe fn new<W>(env: *const ThreadpoolCallbackEnvironment, cx: *mut c_void) -> io::Result<Self>
    where
        W: WorkOnceFn,
    {
        let handle = CreateThreadpoolWork(Some(work_once_callback::<W>), cx, env as _);
        match handle {
            0 => Err(io::Error::last_os_error()),
            _ => Ok(Self(handle)),
        }
    }

    /// Submit work to the threadpool. NOTE that this method may only be called one time. This
    /// routine will consume self and return a Guard which ensures you may not call additional work
    /// to the threadpool work pool
    ///
    /// [See also]
    /// (https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-submitthreadpoolwork)
    pub fn submit(&self) -> &Self {
        unsafe { SubmitThreadpoolWork(self.0) };
        self
    }

    /// Wait for the callback to return. Additionally you may specify to cancel any pending
    /// callbacks that have been submitted to the threadpool work pool
    ///
    /// [See also]
    /// (https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-waitforthreadpoolworkcallbacks)
    pub fn wait(&self, cancel: WaitPending) -> &Self {
        unsafe { WaitForThreadpoolWorkCallbacks(self.0, cancel as _) };
        self
    }
}

impl Drop for OwnedWorkHandle {
    fn drop(&mut self) {
        self.wait(WaitPending::Cancel);
        unsafe { CloseThreadpoolWork(self.0) };
    }
}

impl AsRawHandle for OwnedWorkHandle {
    fn as_raw_handle(&self) -> RawHandle {
        self.0 as _
    }
}

// Windows handles can be shared between threads
unsafe impl Send for OwnedWorkHandle {}
unsafe impl Sync for OwnedWorkHandle {}

/// An owned handle to a Threadpoolwork
///
/// https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-createthreadpoolwork
pub struct WorkOncePool<W>
where
    W: WorkOnceFn,
{
    /// A handle to the underlying Worker
    handle: OwnedWorkHandle,
    /// The worker callback must be kept alive for as long as the threadpool handle. Therefore we
    /// store them together and guarentee the handle lives as long as the callback. When the handle
    /// drops, all callbacks shall have been returned because our drop function waits for callbacks
    /// to finish
    worker: Arc<Oneshot<W>>,
}

impl<W> WorkOncePool<W>
where
    W: WorkOnceFn,
{
    /// Construct a new ThreadpoolWork handle
    ///
    /// https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-createthreadpoolwork
    pub fn new(work: W) -> io::Result<Self> {
        // Safety: A null pointer is allowed
        unsafe { Self::with_environment_raw(std::ptr::null(), work) }
    }

    /// Construct a new threadpool work handle with a private threadpool
    pub fn with_environment(env: &ThreadpoolCallbackEnvironment, work: W) -> io::Result<Self> {
        // Safety: A reference to a threadpool callback environment is already initialized
        unsafe { Self::with_environment_raw(env as *const _, work) }
    }

    /// Construct a new threadpool work handle with a private threadpool
    ///
    /// Safety: raw pointer must be a pointer to an initialized ThreadpoolCallbackEnvironment or a
    /// NULL Pointer
    pub unsafe fn with_environment_raw(
        env: *const ThreadpoolCallbackEnvironment,
        work: W,
    ) -> io::Result<Self> {
        let worker = Arc::new(Oneshot::new(work));
        let handle = OwnedWorkHandle::new::<W>(env, Arc::as_ptr(&worker) as _)?;
        Ok(Self { handle, worker })
    }

    /// Submit work to the threadpool worker pool. You may only submit work once, a guard is
    /// returned to guarentee you may only submit work to the work pool once.
    pub fn submit_once(self) -> WorkOncePoolGuard<W> {
        self.handle.submit();
        WorkOncePoolGuard {
            handle: self.handle,
            worker: self.worker,
        }
    }
}

pub struct WorkOncePoolGuard<W>
where
    W: WorkOnceFn,
{
    handle: OwnedWorkHandle,
    worker: Arc<Oneshot<W>>,
}

impl<W> WorkOncePoolGuard<W>
where
    W: WorkOnceFn,
{
    /// Wait for the worker to finish its work. Additionally, you may specify to cancel any pending
    /// callbacks that have been submitted to the threadpool work pool
    pub fn wait(&self, cancel: WaitPending) -> &Self {
        self.handle.wait(cancel);
        self
    }

    /// Cancel any pending callbacks, wait for current callbacks to finish, and resolve the future
    pub fn cancel_with(&self, result: W::Output) -> &Self {
        self.wait(WaitPending::Cancel);
        // Safety: We ensure all callbacks have finished. We now have exclusive access
        // NOTE we do not need to take the inner worker but it is nice sanity check
        let _inner = unsafe { self.worker.try_take() };
        let mut state = self.worker.state.lock();
        state.result = Some(result);
        if let Some(waker) = state.waker.take() {
            waker.wake()
        }
        self
    }

    pub fn future(&self) -> WorkOnceFuture<W> {
        WorkOnceFuture {
            worker: Arc::clone(&self.worker),
        }
    }
}

pub struct WorkOnceFuture<W>
where
    W: WorkOnceFn,
{
    worker: Arc<Oneshot<W>>,
}

impl<W> Future for WorkOnceFuture<W>
where
    W: WorkOnceFn,
{
    type Output = W::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut lock = self.worker.state.lock();
        match lock.result.take() {
            Some(result) => Poll::Ready(result),
            None => {
                // Some waker accounting
                let new_waker = cx.waker();
                lock.waker = match lock.waker.take() {
                    None => Some(new_waker.clone()),
                    Some(old_waker) => {
                        if old_waker.will_wake(new_waker) {
                            Some(old_waker)
                        } else {
                            Some(new_waker.clone())
                        }
                    }
                };
                Poll::Pending
            }
        }
    }
}

/// Shared state between the callback and the future
struct Shared<O> {
    waker: Option<Waker>,
    result: Option<O>,
}

/// A wrapper around a FnOnce which allows for oneshot via Option::take
struct Oneshot<W: WorkOnceFn> {
    inner: UnsafeCell<Option<W>>,
    state: Mutex<Shared<W::Output>>,
}

impl<W: WorkOnceFn> Oneshot<W> {
    /// Construct a Oneshot
    fn new(work: W) -> Self {
        Self {
            inner: UnsafeCell::new(Some(work)),
            state: Mutex::new(Shared {
                waker: None,
                result: None,
            }),
        }
    }

    /// Take the inner worker
    ///
    /// Inner is private so there are no public references to the inner worker. Additoinally, the
    /// worker may only be called once because we returned a handle which cannot submit additional
    /// work when the caller submits work for the first time. Therefore, only one worker callback
    /// can exist, so this callback "must" have exclusive access to the inner. Privately however,
    /// this is unsafe because the WorkOnceFuture future is able to reach the inner unsafe cell.
    ///
    /// Safety: The inner cell must only be accessed by the worker callback routine in order to
    /// guarentee exclusive access to the inner cell. The WorkOnceFuture routine must never reach for
    /// the inner unsafe cell.
    unsafe fn take(&self) -> W {
        let inner = &mut *self.inner.get();
        if let Some(inner) = inner.take() {
            inner
        } else {
            unreachable!("The work once routine has been executed more than once!")
        }
    }

    /// Try to take the inner worker
    ///
    /// Safety: You must ensure exclusive access to the underlying cell. This means that there must
    /// not be any outstanding callbacks pending. This means you must cancel all pending callbacks
    /// and wait for any current callbacks to finish.
    unsafe fn try_take(&self) -> Option<W> {
        let inner = &mut *self.inner.get();
        inner.take()
    }
}

/// NOTE UnsafeCell strips Syncness. However, we guarentee exclusive access so we add back syncness
unsafe impl<W: WorkOnceFn> Sync for Oneshot<W> where W: Sync {}

/// We run the callers callback (only once!)
pub unsafe extern "system" fn work_once_callback<W>(
    instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    _work: PTP_WORK,
) where
    W: WorkOnceFn,
{
    // Safety: instance is a raw handle to a PTP_CALLBACK_INSTANCE
    let i = unsafe { ThreadpoolCallbackInstance::from_raw_handle(instance as _) };
    let cx = &*(context as *const Oneshot<W>);
    // Safety: We guarentee exclusive access to the inner because only we are allowed to call the
    // take method.  The WorkOnceFuture must not reference the inner worker.
    let inner = unsafe { cx.take() };
    let result = inner.work_once(i);
    let mut lock = cx.state.lock();
    lock.result = Some(result);
    if let Some(waker) = lock.waker.take() {
        waker.wake()
    }
}
