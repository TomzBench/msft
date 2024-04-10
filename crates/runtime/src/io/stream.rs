//! stream

use crate::{
    codec::Decode,
    common::{ThreadpoolCallbackEnvironment, ThreadpoolCallbackInstance, WaitPending},
    io::{
        drive::ReadDriver,
        futures::DecodeStream,
        handle::{AsRawIoHandle, OwnedThreadpoolIoHandle, RawThreadpoolIoHandle, ThreadpoolIoWork},
        overlapped::{Overlapped, ReadOverlapped},
    },
};
use std::{ffi::c_void, io, os::windows::io::FromRawHandle, sync::Arc};
use tracing::debug;
use windows_sys::Win32::System::Threading::{PTP_CALLBACK_INSTANCE, PTP_IO};

pub struct StreamOptions<D: Decode> {
    /// An optional Private Threadpool configuration for which to run the stream
    pub environment: Option<ThreadpoolCallbackEnvironment>,
    /// Convert bytes into a stream of I. See [`Decode`]
    pub decoder: D,
    /// Maximum allowed bytes in our buffer
    pub capacity: usize,
    /// Must restart the stream should the queue reach max capacity of queue length
    pub queue: usize,
}

/// A handle to a Threadpool IO resource. NOTE that completion IO requires that the I/O buffer
/// stays alive for atleast as long as the kernel may access it. We model this guarentee by storing
/// the I/O buffer and the handle which manages the kernel callbacks in the same storage. Therefore
/// the buffer will not drop until the threadpool and pending I/O callbacks are finished
pub struct StreamPool<R, D>
where
    R: ReadOverlapped,
    D: Decode,
{
    /// The underlying threadpool. See [`self::OwnedThreadpoolIoHandle`]
    pool: OwnedThreadpoolIoHandle,
    /// Shared state between the threadpool and the stream. See [`ReadDriver`]
    drive: Arc<ReadDriver<R, D>>,
}

impl<R, D> StreamPool<R, D>
where
    R: ReadOverlapped + AsRawIoHandle,
    D: Decode,
{
    pub fn new(
        maybe_env: Option<ThreadpoolCallbackEnvironment>,
        handle: R,
        decoder: D,
        capacity: usize,
        queue: usize,
    ) -> io::Result<Self> {
        let raw = handle.as_raw_io_handle();
        let drive = Arc::new(ReadDriver::new(handle, decoder, capacity, queue));
        let cx = Arc::as_ptr(&drive) as _;
        let env = maybe_env
            .map(|env| &env as *const _)
            .unwrap_or(std::ptr::null() as *const _);
        // Safety: a reference to ThreadpoolCallbackEnvironment is valid so we may take const ptr
        // to environment. NULL is also allowed
        let pool = unsafe { OwnedThreadpoolIoHandle::new_stream::<R, D>(env, raw, cx) }?;
        Ok(StreamPool { pool, drive })
    }
}

impl<R, D> StreamPool<R, D>
where
    R: ReadOverlapped,
    D: Decode,
{
    /// Begin reading bytes from the I/O resource. This function will return a guard that will
    /// prevent you from starting read again. If you wish to schedule more reads, you have to use
    /// [`self::StreamPoolGuard::restart`]
    pub fn start(self) -> StreamPoolGuard<R, D> {
        // safety: This is first read so no read callbacks are running. we have exclusive ownership
        unsafe { self.drive.start(&self.pool) };
        StreamPoolGuard {
            pool: self.pool,
            drive: self.drive,
        }
    }
}

/// A StreamPoolGuard is used to prevent callers from potentially starting multiple reads on
/// the same I/O resource at the same time. This means a threadpool may only have 1 read pending at
/// a time.
pub struct StreamPoolGuard<R, D>
where
    R: ReadOverlapped,
    D: Decode,
{
    pool: OwnedThreadpoolIoHandle,
    drive: Arc<ReadDriver<R, D>>,
}

impl<R, D> StreamPoolGuard<R, D>
where
    R: ReadOverlapped,
    D: Decode,
{
    /// Restart read operations on the underlying I/O resource.  If an I/O stream associated with
    /// this handle has stopped (because of a failure), the caller can restart the stream using the
    /// restart method.
    ///
    /// The restart routine will block until all pending kernel callbacks from the threadpool have
    /// finished.
    pub fn restart(&self, pending: WaitPending) {
        self.pool.wait(pending);
        // safety: We have waited for worker callbacks to complete so we have exclusive ownership.
        unsafe { self.drive.start(&self.pool) }
    }

    /// A read stream will begin producing bytes from the underlying I/O resource. To consume these
    /// bytes you must await them on a stream.  This method will return a stream which will yeild
    /// the bytes associated with the I/O resource.
    pub fn stream(&self) -> DecodeStream<R, D> {
        DecodeStream::new(Arc::clone(&self.drive))
    }
}

/// This callback will call the read_complete routine if success and then begin another read on
/// the io resource
pub(in crate::io) unsafe extern "system" fn stream_system_callback<R, D>(
    instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    overlapped: *mut c_void,
    io_result: u32,
    bytes_transferred: usize,
    io: PTP_IO,
) where
    R: ReadOverlapped,
    D: Decode,
{
    // extract all the c_void stuff
    let cx = &*(context as *const ReadDriver<R, D>);
    let pool = RawThreadpoolIoHandle::from_raw_handle(io as _);
    let _i = ThreadpoolCallbackInstance::from_raw_handle(instance as _);
    let _overlapped = &mut *(overlapped as *mut Overlapped);
    debug!(ptr = context as usize, "stream pointer from callback");

    // Drive new bytes into context so to populate the queue with results from the decoder
    cx.completion(io_result, bytes_transferred, &pool);
}
