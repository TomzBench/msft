//! overlapped

use super::handle::ThreadpoolIoWork;
use bytes::BytesMut;
use std::{error, fmt, io};
use tracing::debug;
use windows_sys::Win32::{
    Foundation::{ERROR_IO_PENDING, HANDLE},
    System::IO::OVERLAPPED,
};

#[repr(usize)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum OverlappedKind {
    Read = 0,
    Write = 1,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Overlapped {
    pub internal: usize,
    pub internal_high: usize,
    pub offset: u32,
    pub offset_high: u32,
    pub hevent: HANDLE,
    pub kind: OverlappedKind,
}

impl Overlapped {
    pub fn new_read(offset: u64) -> Self {
        Overlapped::new(0, offset, OverlappedKind::Read)
    }

    pub fn new_write(offset: u64) -> Self {
        Overlapped::new(0, offset, OverlappedKind::Write)
    }

    pub fn new(hevent: HANDLE, offset: u64, kind: OverlappedKind) -> Self {
        Overlapped {
            internal: 0,
            internal_high: 0,
            offset: (offset & 0xFFFFFFFF) as u32,
            offset_high: (offset >> 32) as u32,
            hevent,
            kind,
        }
    }

    /// Similar to [`std::cell::UnsafeCell::get`]
    pub fn get(&self) -> *mut Overlapped {
        self as *const _ as *mut _
    }

    pub fn as_ptr(&self) -> *const OVERLAPPED {
        self as *const Self as *const _
    }

    pub fn as_mut_ptr(&mut self) -> *mut OVERLAPPED {
        self.as_ptr() as _
    }

    pub fn set_offset(&mut self, offset: u64) -> &mut Self {
        self.offset = (offset & 0xFFFFFFFF) as u32;
        self.offset_high = (offset >> 32) as u32;
        self
    }

    pub fn get_offset(&self) -> u64 {
        let mut result: u64 = self.offset as _;
        result += (self.offset_high as u64) << 32;
        result
    }
}

/// When performing OverlappedIo, the I/O operation may occasionally finish synchronously. An
/// overlapped error distinguishes this special case as seperate from a normal I/O error
#[derive(Debug)]
pub enum OverlappedError {
    /// A true I/O kernel error
    Os(i32),
    /// Some hacky error type, should not be used, however, this exists because the std::io::Error
    /// can potentially barf this up at us sometimes
    CustomIo(io::Error),
    /// an ERROR_IO_PENDING, which is normal behavior in most circumstances
    Pending,
    /// A file handle has been closed
    Eof,
}

impl error::Error for OverlappedError {}
impl fmt::Display for OverlappedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Os(raw) => write!(f, "{}", io::Error::from_raw_os_error(*raw)),
            Self::CustomIo(e) => write!(f, "{e}"),
            Self::Pending => write!(f, "I/O pending"),
            Self::Eof => write!(f, "I/O end of file"),
        }
    }
}

impl From<io::Error> for OverlappedError {
    fn from(value: io::Error) -> Self {
        match value.raw_os_error() {
            Some(raw) if raw == ERROR_IO_PENDING as i32 => Self::Pending,
            Some(raw) => Self::Os(raw as _),
            None => Self::CustomIo(value),
        }
    }
}

/// File handles such as USB devices, Network sockets, etc, will implement ReadOverlapped.  It
/// might be like AsyncRead except more appropriate for completion based IO
pub trait ReadOverlapped {
    /// perform an overlapped read, IE ReadFile or WSARead or mock or something
    fn read_overlapped(
        &mut self,
        overlapped: &mut Overlapped,
        bytes: &mut BytesMut,
    ) -> Result<usize, OverlappedError>;
}

/// Helper routines for things that implement ReadOverlapped
pub trait ReadOverlappedEx {
    /// Same as [`self::ReadOverlapped::read_overlapped`] except will notify a worker about the I/O
    /// operation.
    fn start_read<W>(
        &mut self,
        overlapped: &mut Overlapped,
        bytes: &mut BytesMut,
        worker: &W,
    ) -> Result<usize, OverlappedError>
    where
        W: ThreadpoolIoWork;

    /// Sometimes an overlapped operation will return Synchornously with number of bytes read.
    /// However, Sockets and USB devices, or other stream like IO might simply want to force a non
    /// blocking async read.  This will eat all the bytes available until there are no more bytes
    /// to be read. Leaving a pending IO operation to be fullfilled sometime in the future
    fn start_read_stream<W>(
        &mut self,
        overlapped: &mut Overlapped,
        bytes: &mut BytesMut,
        worker: &W,
    ) -> Result<usize, OverlappedError>
    where
        W: ThreadpoolIoWork;
}

impl<R> ReadOverlappedEx for R
where
    R: ReadOverlapped,
{
    /// Performs a single read operation on an underlying I/O resource. This routine will manage a
    /// threadpool and let a worker know that an I/O operation is scheduled and a callback will be
    /// called to handle the completion.
    ///
    /// If the ReadOverlapped routine completes synchronously, or if an error occurs, the
    /// threadpool will receive a cancellation and unschedule the worker to wait for the I/O event.
    /// This is required to avoid memory leaks
    fn start_read<W>(
        &mut self,
        overlapped: &mut Overlapped,
        bytes: &mut BytesMut,
        worker: &W,
    ) -> Result<usize, OverlappedError>
    where
        W: ThreadpoolIoWork,
    {
        worker.start();
        let result = self.read_overlapped(overlapped, bytes);
        match result {
            Err(OverlappedError::Pending) => Err(OverlappedError::Pending),
            Err(e) => {
                worker.cancel();
                Err(e)
            }
            Ok(n) => {
                worker.cancel();
                // Safety: ReadOverlapped successfully initialized 0..n synchronously
                unsafe { bytes.set_len(n) };
                Ok(n)
            }
        }
    }

    /// Similar to [`self::ReadOverlappedEx::start_read`] except we will drive the underlying
    /// stream until Pending is received, so that there is always a worker listening for incoming
    /// bytes
    fn start_read_stream<W>(
        &mut self,
        overlapped: &mut Overlapped,
        bytes: &mut BytesMut,
        worker: &W,
    ) -> Result<usize, OverlappedError>
    where
        W: ThreadpoolIoWork,
    {
        let mut bytes_read = 0u32;
        loop {
            let result = self.start_read(overlapped, bytes, worker);
            debug!(?result, "read stream result");
            match result {
                Err(OverlappedError::Pending) => break Ok(bytes_read as usize),
                Err(e) => break Err(e),
                Ok(n) => bytes_read += n as u32,
            }
        }
    }
}

/// File handles such as USB devices, network sockets, etc will implement WriteOverlapped. It might
/// be like AsyncRead except more appropriate for completion based IO
pub trait WriteOverlapped {
    /// Perform an overlapped write, IE WriteFile or WSAWrite.  Implementations must handle the
    /// case where the write is finished synchronously, in which case, the written bytes shall be
    /// removed from the bytes buffer and returned
    fn write_overlapped(
        &mut self,
        overlapped: &mut Overlapped,
        bytes: &mut [u8],
    ) -> Result<usize, OverlappedError>;
}
