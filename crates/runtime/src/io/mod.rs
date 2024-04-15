//! io
//!
//! ThreadpoolIo Create, Close, Start, Wait, Cancel
//!
//! https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-createthreadpoolio
//! https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-closethreadpoolio
//! https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-startthreadpoolio
//! https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-waitforthreadpooliocallbacks
//! https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-cancelthreadpoolio

mod drive;
mod error;
mod futures;
mod handle;
mod overlapped;

#[cfg(test)]
mod tests;

pub use error::{SinkError, StreamError};
pub use futures::{DecodeStream, Flush};
pub use handle::{
    AsRawIo, AsRawIoHandle, OwnedThreadpoolIoHandle, RawFileHandle, RawThreadpoolIoHandle,
    ThreadpoolIoWork,
};
pub use overlapped::{
    Overlapped, OverlappedError, OverlappedKind, ReadOverlapped, ReadOverlappedEx, WriteOverlapped,
};

use crate::{
    codec::Decode,
    common::{ThreadpoolCallbackEnvironment, ThreadpoolCallbackInstance, WaitPending},
    futures::{FuturesExt, Signal, StreamExt, Watch},
};
use bytes::BytesMut;
use drive::{Drive, ReadDriver, WriteDriver};
use std::{ffi::c_void, io, os::windows::io::FromRawHandle, sync::Arc};
use tracing::debug;
use windows_sys::Win32::System::Threading::{PTP_CALLBACK_INSTANCE, PTP_IO};

use self::handle::BorrowedThreadpoolIoHandle;

#[derive(Debug)]
pub enum ThreadpoolError<R>
where
    R: std::error::Error,
{
    Stream(StreamError<R>),
    Sink,
    Io(io::Error),
}

impl<R> std::error::Error for ThreadpoolError<R> where R: std::error::Error {}
impl<R> std::fmt::Display for ThreadpoolError<R>
where
    R: std::error::Error,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stream(e) => write!(f, "Stream error => {e}"),
            Self::Sink => write!(f, "Sink error"),
            Self::Io(e) => write!(f, "Io error => {e}"),
        }
    }
}

impl<R> From<io::Error> for ThreadpoolError<R>
where
    R: std::error::Error,
{
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub struct ThreadpoolOptions<D: Decode> {
    /// Threadpool Environment
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
/// the buffer will not drop until the threadpool and pending I/O callbacks are finished.
///
/// NOTE the order is important RFC 1857 specifies drop order. The threadpool is dropped first.
/// Therefore no more callbacks can run, we then free the shared context pointer
pub struct ThreadpoolIo<H, D>
where
    H: AsRawIo,
    D: Decode,
{
    /// The underlying threadpool. See [`OwnedThreadpoolIoHandle`]
    pool: OwnedThreadpoolIoHandle,
    /// Shared state between futures and callbacks. See [`ReadDriver`] and [`WriteDriver`]
    drive: Arc<Drive<H::Raw, D>>,
    /// The underlying I/O resource. We cache here to keep resource from dropping
    #[allow(unused)]
    handle: H,
}

impl<H, D> ThreadpoolIo<H, D>
where
    H: AsRawIo,
    D: Decode,
{
    pub fn new(
        handle: H,
        options: ThreadpoolOptions<D>,
    ) -> Result<Self, ThreadpoolError<D::Error>> {
        let reader = ReadDriver::new(
            handle.as_raw_io(),
            options.decoder,
            options.capacity,
            options.queue,
        );
        let raw = handle.as_raw_io().as_raw_io_handle();
        let writer = WriteDriver::new(handle.as_raw_io());
        let drive = Arc::new(Drive::new(reader, writer));
        let pool = OwnedThreadpoolIoHandle::new(
            options.environment,
            raw,
            io_callback::<H::Raw, D>,
            Arc::as_ptr(&drive) as _,
        )?;
        Ok(ThreadpoolIo {
            pool,
            handle,
            drive,
        })
    }

    pub fn reader(&self) -> Reader<'_, H::Raw, D> {
        Reader {
            pool: self.pool.as_handle(),
            drive: Arc::clone(&self.drive),
            reading: None,
        }
    }

    pub fn writer(&self) -> Writer<'_, H::Raw, D> {
        Writer {
            pool: self.pool.as_handle(),
            drive: Arc::clone(&self.drive),
            writing: None,
        }
    }

    pub fn reader_writer(&self) -> (Reader<'_, H::Raw, D>, Writer<'_, H::Raw, D>) {
        (self.reader(), self.writer())
    }
}

pub struct Reader<'pool, R, D>
where
    D: Decode,
{
    /// A reference to [`OwnedThreadpoolIoHandle`]
    pool: BorrowedThreadpoolIoHandle<'pool>,
    /// Shared state between futures and callbacks. See [`ReadDriver`] and [`WriteDriver`]
    drive: Arc<Drive<R, D>>,
    /// Shared state between futures and callbacks. See [`ReadDriver`] and [`WriteDriver`]
    reading: Option<Signal>,
}

impl<'pool, R, D> Reader<'pool, R, D>
where
    R: ReadOverlapped,
    D: Decode,
{
    /// A future that resolves with a stream of incoming bytes.
    pub async fn stream(&mut self) -> Watch<DecodeStream<'_, R, D>> {
        let (signal, stream) = DecodeStream::new(self.drive.as_ref().reader()).watch();
        if let Some(signal) = self.reading.replace(signal) {
            debug!("waiting for previous stream to complete");
            signal.await;
        }
        debug!("starting read stream");
        // Safety: we have no outstanding reads in progress because we waited for last read to
        // complete. Therefore we have exclusive access and it is safe to start another read
        unsafe { self.drive.reader().start(&self.pool) };
        stream
    }
}

pub struct Writer<'pool, W, D>
where
    D: Decode,
{
    /// A reference to [`OwnedThreadpoolIoHandle`]
    pool: BorrowedThreadpoolIoHandle<'pool>,
    /// Shared state between futures and callbacks. See [`ReadDriver`] and [`WriteDriver`]
    drive: Arc<Drive<W, D>>,
    /// Shared state between futures and callbacks. See [`ReadDriver`] and [`WriteDriver`]
    writing: Option<Signal>,
}

impl<'pool, W, D> Writer<'pool, W, D>
where
    W: WriteOverlapped,
    D: Decode,
{
    /// A future that resolves with a buffer for which to sink bytes into
    pub async fn with_capacity(&mut self, capacity: usize) -> Sink<'_, W> {
        self.buffer(BytesMut::with_capacity(capacity)).await
    }

    /// A future that resolves when the bytes have been written
    pub async fn write<B: Into<BytesMut>>(&mut self, buffer: B) -> Result<(), SinkError> {
        self.buffer(buffer).await.flush()?.await
    }

    pub async fn buffer<B: Into<BytesMut>>(&mut self, buffer: B) -> Sink<'_, W> {
        let (signal, future) = Flush::new(self.drive.as_ref().writer()).watch();
        if let Some(signal) = self.writing.replace(signal) {
            signal.await
        }
        self.drive.writer().buffer(buffer.into());
        Sink {
            future,
            pool: self.pool.as_handle(),
        }
    }
}

pub struct Sink<'pool, W> {
    future: Watch<Flush<'pool, W>>,
    pool: BorrowedThreadpoolIoHandle<'pool>,
}

impl<'pool, W> Sink<'pool, W>
where
    W: WriteOverlapped,
{
    pub fn flush(self) -> Result<Watch<Flush<'pool, W>>, SinkError> {
        // Safety: We have not started future yet, so it is guarenteed to be Some
        let fut = unsafe { self.future.inner().unwrap_unchecked() };
        // Safety: Sink is not dispursed until previous Write is completed, so access is exclusive
        unsafe { fut.drive().flush(&self.pool) }
        Ok(self.future)
    }
}

/// This callback will call the read_complete routine if success and then begin another read on
/// the io resource
pub(in crate::io) unsafe extern "system" fn io_callback<H, D>(
    instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    overlapped: *mut c_void,
    io_result: u32,
    bytes_transferred: usize,
    io: PTP_IO,
) where
    H: ReadOverlapped + WriteOverlapped,
    D: Decode,
{
    // extract all the c_void stuff
    let cx = &*(context as *const Drive<H, D>);
    let pool = RawThreadpoolIoHandle::from_raw_handle(io as _);
    let _i = ThreadpoolCallbackInstance::from_raw_handle(instance as _);
    let overlapped = &mut *(overlapped as *mut Overlapped);

    match overlapped.kind {
        OverlappedKind::Read => cx.reader().completion(io_result, bytes_transferred, &pool),
        OverlappedKind::Write => cx.writer().completion(io_result, bytes_transferred, &pool),
    }
}
