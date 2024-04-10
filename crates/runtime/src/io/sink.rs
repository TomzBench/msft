//! sink

use crate::{
    codec::{SinkEncode, SinkEncodeLen},
    common::{ThreadpoolCallbackEnvironment, ThreadpoolCallbackInstance},
    futures::{FuturesExt, Signal, Watch},
    io::{
        drive::WriteDriver,
        error::SinkEncodeError,
        futures::Flush,
        handle::{
            AsRawIoHandle, BorrowedThreadpoolIoHandle, OwnedThreadpoolIoHandle,
            RawThreadpoolIoHandle,
        },
        overlapped::{Overlapped, OverlappedError, WriteOverlapped},
    },
};
use bytes::BytesMut;
use std::{ffi::c_void, io, os::windows::io::FromRawHandle, sync::Arc};
use windows_sys::Win32::System::Threading::{PTP_CALLBACK_INSTANCE, PTP_IO};

/// A handle to a Threadpool IO resource. NOTE that completion IO requires that the I/O buffer
/// stays alive for atleast as long as the kernel may access it. We model this guarentee by storing
/// the I/O buffer and the handle which manages the kernel callbacks in the same storage. Therefore
/// the buffer will not drop until the threadpool and pending I/O callbacks are finished.
///
/// NOTE the order is important RFC 1857 specifies drop order. The threadpool is dropped first.
/// Therefore no more callbacks can run, we then free the shared context pointer
pub struct SinkPool<W> {
    /// The underlying threadpool. See [`OwnedThreadpoolIoHandle`]
    pool: OwnedThreadpoolIoHandle,
    /// Shared state between the threadpool and the [`Flush`] future. See [`WriteDriver`]
    drive: Arc<WriteDriver<W>>,
    /// A signal to ensure only one write is allowed at a time
    signal: Option<Signal>,
}

impl<W> SinkPool<W>
where
    W: WriteOverlapped + AsRawIoHandle,
{
    /// Create a pool to manage overlapped writes
    pub fn new(maybe_env: Option<ThreadpoolCallbackEnvironment>, handle: W) -> io::Result<Self> {
        let raw = handle.as_raw_io_handle();
        let drive = Arc::new(WriteDriver::new(handle));
        let cx = Arc::as_ptr(&drive) as _;
        let env = maybe_env
            .map(|env| &env as *const _)
            .unwrap_or(std::ptr::null() as *const _);
        let pool = unsafe { OwnedThreadpoolIoHandle::new_sink::<W>(env, raw, cx) }?;
        Ok(SinkPool {
            pool,
            drive,
            signal: None,
        })
    }

    /// Create a buffer for writes
    pub async fn buffer(&mut self, capacity: usize) -> SinkBuffer<'_, W> {
        let (signal, flush) = Flush::new(Arc::clone(&self.drive)).watch();
        if let Some(signal) = self.signal.replace(signal) {
            signal.await;
        }
        self.drive.start(BytesMut::with_capacity(capacity));
        SinkBuffer::new(flush, self.pool.as_handle())
    }
}
pub struct SinkBuffer<'pool, W> {
    future: Watch<Flush<W>>,
    pool: BorrowedThreadpoolIoHandle<'pool>,
}

impl<'pool, W> SinkBuffer<'pool, W>
where
    W: WriteOverlapped,
{
    fn new(
        future: Watch<Flush<W>>,
        pool: BorrowedThreadpoolIoHandle<'pool>,
    ) -> SinkBuffer<'pool, W> {
        SinkBuffer { future, pool }
    }

    pub fn encode<Item>(&self, item: &Item) -> Result<&Self, SinkEncodeError<Item::Error>>
    where
        Item: SinkEncode + SinkEncodeLen,
    {
        // Safety: We have not started future yet, so it is guarenteed to be Some
        let fut = unsafe { self.future.inner().unwrap_unchecked() };
        fut.drive().push_encodable(item).map(|_| self)
    }

    pub fn flush(self) -> Result<Watch<Flush<W>>, OverlappedError> {
        // Safety: We have not started future yet, so it is guarenteed to be Some
        let fut = unsafe { self.future.inner().unwrap_unchecked() };
        // Safety: We have not started future yet, so it is guarenteed to be Some
        unsafe { fut.drive().flush(&self.pool) }
        Ok(self.future)
    }
}

/// This callback will call the read_complete routine if success and then begin another read on
/// the io resource
pub unsafe extern "system" fn sink_system_callback<W>(
    instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    overlapped: *mut c_void,
    io_result: u32,
    bytes_transferred: usize,
    io: PTP_IO,
) where
    W: WriteOverlapped,
{
    // extract all the c_void stuff
    let cx = &*(context as *const WriteDriver<W>);
    let pool = RawThreadpoolIoHandle::from_raw_handle(io as _);
    let _i = ThreadpoolCallbackInstance::from_raw_handle(instance as _);
    let _overlapped = &mut *(overlapped as *mut Overlapped);

    cx.completion(io_result, bytes_transferred, &pool);
}
