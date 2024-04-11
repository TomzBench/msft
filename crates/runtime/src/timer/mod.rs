//! ThreadpoolTimer Create, Close, Set, Wait
//!
//! https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-createthreadpooltimer
//! https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-closethreadpooltimer
//! https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-setthreadpooltimer
//! https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-waitforthreadpooltimercallbacks

use crate::{
    common::{ThreadpoolCallbackEnvironment, WaitPending},
    futures::{FuturesExt, Signal, StreamExt, Watch},
};
use crossbeam::queue::ArrayQueue;
use futures::Stream;
use parking_lot::Mutex;
use std::{
    ffi::c_void,
    future::Future,
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll, Waker},
    time::Duration,
};
use tracing::{debug, warn};
use windows_sys::Win32::{
    Foundation::FILETIME,
    System::Threading::{
        CloseThreadpoolTimer, CreateThreadpoolTimer, SetThreadpoolTimer,
        WaitForThreadpoolTimerCallbacks, PTP_CALLBACK_INSTANCE, PTP_TIMER,
    },
};

/// Configure Timer threadpool behavior
pub struct TimerThreadpoolOptions<'env> {
    /// A private threadpool configuration (or use the public threadpools)
    pub env: Option<&'env ThreadpoolCallbackEnvironment>,
    /// Number of timeouts to queue should multiple timeouts occur before acknowledged
    pub capacity: usize,
    /// Timer resolution (allows batching timeouts to save cpu cycles)
    pub window: Option<Duration>,
}

impl Default for TimerThreadpoolOptions<'_> {
    fn default() -> Self {
        Self {
            env: None,
            capacity: 8,
            window: None,
        }
    }
}

/// A handle to pool of workers who will wait for timer objects. The context is also shared by the
/// futures and weakly by the kernel. The weak reference is used by the kernel is guarenteed to be
/// valid because the threadpool will wait for all kernel callbacks to resolve prior to dropping.
///
/// This is guarenteed because RFC 1857 specifying drop order. The threadpool is dropped first
/// and waits for callbacks to finish executing. Then the pointers are dropped.
///
/// Safety: DO NOT CHANGE ORDER IN STRUCT (RFC 1857)
pub struct TimerPool {
    /// A pool of workers to wait on waitable timers. See [`OwnedTimerHandle`]
    pool: OwnedTimerHandle,
    /// Shared state between the timer worker callbacks and the future waiting for timeout
    shared: Arc<Shared>,
    /// Allow batching timeouts to conserve power
    window: u32,
    /// Any previous timers that may be running must be stopped prior to creating a new timer
    timer: Option<Signal>,
}

impl TimerPool {
    pub fn new(options: &TimerThreadpoolOptions) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            waker: Mutex::new(None),
            timeouts: ArrayQueue::new(options.capacity),
            stopped: AtomicBool::new(false),
        });
        let window = options
            .window
            .map(|dur| dur.as_millis() as u32)
            .unwrap_or(0);
        OwnedTimerHandle::new(options.env, Arc::as_ptr(&shared) as _).map(|pool| Self {
            pool,
            shared,
            timer: None,
            window,
        })
    }

    /// Create a relative oneshot timer. Will wait for any outstanding timers if any outstanding
    /// timers are pending.
    pub async fn oneshot(&mut self, duration: Duration) -> OneshotTimer<'_> {
        let shared = Arc::clone(&self.shared);
        let (signal, fut) = TimerFuture { shared }.watch();
        if let Some(signal) = self.timer.replace(signal) {
            warn!("waiting for previous timer to finished before starting oneshot timer");
            signal.await;
        }
        self.shared.reset();
        OneshotTimer {
            fut,
            due: duration,
            window: self.window,
            pool: &self.pool,
        }
    }

    /// Start a stream of periodic timer events. Will wait for any outstanding timers if any
    /// outstanding timers are pending.
    pub async fn periodic(&mut self, duration: Duration, period: Duration) -> PeriodicTimer<'_> {
        let shared = Arc::clone(&self.shared);
        let (signal, stream) = TimerStream { shared }.watch();
        if let Some(signal) = self.timer.replace(signal) {
            warn!("waiting for previous timer to finished before starting perodic timer");
            signal.await;
        }
        self.shared.reset();
        PeriodicTimer {
            stream,
            due: duration,
            period,
            window: self.window,
            pool: &self.pool,
        }
    }

    /// Cancel any pending timers
    pub fn cancel(&self) -> &Self {
        self.pool.stop();
        self.shared.stop().maybe_wake_by_ref();
        self
    }
}

pub struct OneshotTimer<'pool> {
    fut: Watch<TimerFuture>,
    due: Duration,
    window: u32,
    pool: &'pool OwnedTimerHandle,
}

impl<'pool> OneshotTimer<'pool> {
    pub fn start(self) -> Watch<TimerFuture> {
        debug!(duration=?self.due, "starting oneshot timer");
        self.pool.start_relative(self.due, 0, self.window);
        self.fut
    }
}

pub struct PeriodicTimer<'pool> {
    stream: Watch<TimerStream>,
    due: Duration,
    period: Duration,
    window: u32,
    pool: &'pool OwnedTimerHandle,
}

impl<'pool> PeriodicTimer<'pool> {
    pub fn start(self) -> Watch<TimerStream> {
        debug!(duration=?self.due, period=?self.period, "starting periodic timer");
        let period = self.period.as_millis() as _;
        self.pool.start_relative(self.due, period, self.window);
        self.stream
    }
}

#[derive(Debug)]
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct TimerFuture {
    shared: Arc<Shared>,
}

impl Future for TimerFuture {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.shared.timeouts.len() > 0 {
            Poll::Ready(())
        } else {
            self.shared.update_waker(cx.waker());
            Poll::Pending
        }
    }
}

#[derive(Debug)]
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct TimerStream {
    shared: Arc<Shared>,
}

impl Stream for TimerStream {
    type Item = ();
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.shared.is_stopped() {
            Poll::Ready(None)
        } else {
            self.shared.update_waker(cx.waker());
            self.shared
                .timeouts
                .pop()
                .map_or(Poll::Pending, |_| Poll::Ready(Some(())))
        }
    }
}

#[derive(Debug)]
pub struct Shared {
    waker: Mutex<Option<Waker>>,
    stopped: AtomicBool,
    timeouts: ArrayQueue<()>,
}

impl Shared {
    fn update_waker(&self, new_waker: &Waker) {
        let mut waker = self.waker.lock();
        *waker = match waker.take() {
            None => Some(new_waker.clone()),
            Some(old_waker) => {
                if old_waker.will_wake(new_waker) {
                    Some(old_waker)
                } else {
                    Some(new_waker.clone())
                }
            }
        };
    }

    fn fire(&self) -> &Self {
        let _ = self.timeouts.push(());
        self
    }

    fn stop(&self) -> &Self {
        self.stopped.store(true, Ordering::SeqCst);
        self
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    fn maybe_wake_by_ref(&self) -> &Self {
        if let Some(waker) = self.waker.lock().as_ref() {
            waker.wake_by_ref()
        }
        self
    }

    fn reset(&self) -> &Self {
        self.stopped.store(false, Ordering::SeqCst);
        while let Some(_) = self.timeouts.pop() {}
        self
    }
}

pub(in crate::timer) struct OwnedTimerHandle(PTP_TIMER);
impl Drop for OwnedTimerHandle {
    fn drop(&mut self) {
        self.stop();
        self.wait(WaitPending::Cancel);
        unsafe { CloseThreadpoolTimer(self.0) }
    }
}

impl OwnedTimerHandle {
    /// Create a new threadpool timer object
    ///
    /// See also:
    /// https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-createthreadpooltimer
    pub(in crate::timer) fn new(
        maybe_env: Option<&ThreadpoolCallbackEnvironment>,
        cx: *mut c_void,
    ) -> io::Result<Self> {
        let env = maybe_env.map_or_else(std::ptr::null, |env| env as *const _ as _);
        let result = unsafe { CreateThreadpoolTimer(Some(timer_callback), cx, env) };
        match result {
            0 => Err(io::Error::last_os_error()),
            handle => Ok(OwnedTimerHandle(handle)),
        }
    }

    /// Stop a timer from from queue new callbacks. (Callbacks already queued will still occur)
    pub(in crate::timer) fn stop(&self) {
        unsafe { SetThreadpoolTimer(self.0, std::ptr::null(), 0, 0) }
    }

    /// Start a timer.
    ///
    /// See also:
    /// https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-setthreadpooltimer
    pub(in crate::timer) fn start_relative(&self, due: Duration, period: u32, window: u32) {
        let tick = due.as_millis() as i64 * -10_000;
        let ft = FILETIME {
            dwLowDateTime: (tick & 0xFFFFFFFF) as u32,
            dwHighDateTime: (tick >> 32) as u32,
        };
        unsafe { SetThreadpoolTimer(self.0, &ft as *const _, period, window) }
    }

    /// Waits for outstanding timer callbacks to complete and optionally cancels pending callbacks
    /// that have not yet started to execute.
    pub(in crate::timer) fn wait(&self, pending: WaitPending) {
        unsafe { WaitForThreadpoolTimerCallbacks(self.0, pending as _) };
    }
}

unsafe extern "system" fn timer_callback(
    _instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    _wait: PTP_TIMER,
) {
    let cx = unsafe { &*(context as *const Shared) };
    cx.fire().maybe_wake_by_ref();
}
