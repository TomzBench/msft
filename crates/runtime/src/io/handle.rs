//! This module basically represents I/O types that may either be owned (and will prevent the I/O
//! resource from dropping). Or "Raw" which allows usage via the C API. Forexample, a
//! [`super::OwnedThreadpoolIoHandle`] and [`super::RawThreadpoolIoHandle`] expose the same API
//! methods but may be called from C context or from rust context

use super::{
    Overlapped, OverlappedError, ReadOverlapped, ThreadpoolCallbackEnvironment, WaitPending,
    WriteOverlapped,
};
// use crate::io::stream::stream_system_callback;
use bytes::BytesMut;
use std::{ffi::c_void, io, marker::PhantomData, os::windows::prelude::*};
use windows_sys::Win32::{
    Foundation::HANDLE,
    System::Threading::{
        CancelThreadpoolIo, CloseThreadpoolIo, CreateThreadpoolIo, StartThreadpoolIo,
        WaitForThreadpoolIoCallbacks, PTP_CALLBACK_INSTANCE, PTP_IO,
    },
};

macro_rules! impl_raw_handle {
    ($name:ident) => {
        impl std::os::windows::prelude::AsRawHandle for $name {
            fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
                self.0 as _
            }
        }

        impl std::os::windows::prelude::FromRawHandle for $name {
            unsafe fn from_raw_handle(handle: std::os::windows::io::RawHandle) -> Self {
                Self(handle as _)
            }
        }

        // Windows handles can be shared across threads
        unsafe impl Send for $name {}
        unsafe impl Sync for $name {}
    };
}

macro_rules! impl_fs_handle {
    ($handle:ty) => {
        impl ReadOverlapped for $handle {
            fn read_overlapped(
                &mut self,
                overlapped: &mut Overlapped,
                bytes: &mut BytesMut,
            ) -> Result<usize, OverlappedError> {
                crate::fs::read::read_overlapped(self.as_raw_handle(), overlapped, bytes)
            }
        }

        impl WriteOverlapped for $handle {
            fn write_overlapped(
                &mut self,
                overlapped: &mut Overlapped,
                bytes: &mut [u8],
            ) -> Result<usize, OverlappedError> {
                crate::fs::write::write_overlapped(self.as_raw_handle(), overlapped, bytes)
            }
        }

        impl AsRawIo for $handle {
            type Raw = RawFileHandle;

            fn as_raw_io(&self) -> Self::Raw {
                RawFileHandle(self.as_raw_io_handle() as _)
            }
        }

        impl AsRawIoHandle for $handle {
            fn as_raw_io_handle(&self) -> HANDLE {
                self.as_raw_handle() as _
            }
        }

        // impl Io for $handle {}
    };
}

macro_rules! impl_threadpool_io_work {
    ($name:ty) => {
        impl ThreadpoolIoWork for $name {
            fn start(&self) -> &Self {
                unsafe { StartThreadpoolIo(self.as_raw_handle() as _) };
                self
            }

            fn wait(&self, pending: WaitPending) -> &Self {
                unsafe { WaitForThreadpoolIoCallbacks(self.as_raw_handle() as _, pending as _) };
                self
            }

            fn cancel(&self) -> &Self {
                unsafe { CancelThreadpoolIo(self.as_raw_handle() as _) };
                self
            }
        }
    };
}

/// Similar to a [`std::os::windows::io::OwnedHandle`] except for a
/// [`std::os::windows::io::RawHandle`] for which we can impl our traits
#[derive(Copy, Clone)]
pub struct RawFileHandle(RawHandle);

impl AsRawHandle for RawFileHandle {
    fn as_raw_handle(&self) -> RawHandle {
        self.0 as _
    }
}

// Windows handles can be shared across threads
unsafe impl Send for RawFileHandle {}
unsafe impl Sync for RawFileHandle {}

pub trait AsRawIoHandle {
    fn as_raw_io_handle(&self) -> HANDLE;
}

pub trait AsRawIo {
    type Raw: ReadOverlapped + WriteOverlapped + Copy + AsRawIoHandle;
    fn as_raw_io(&self) -> Self::Raw;
}

/// TODO could probably deprecate this and pass concrete threadpool to OverlappedEx
pub trait ThreadpoolIoWork {
    /// Submit work to a threadpool worker
    ///
    /// https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-startthreadpoolio
    fn start(&self) -> &Self;

    /// Wait for threadpool io worker callbacks to complete (optionally cancel any pending
    /// callbacks)
    ///
    /// https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-waitforthreadpooliocallbacks
    fn wait(&self, pending: WaitPending) -> &Self;

    /// Undo a submit to work to a threadpool worker (cancel)
    ///
    /// https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-cancelthreadpoolio
    fn cancel(&self) -> &Self;
}

/// A handle to a ThreadpoolIo worker. A ThreadpoolIo worker can be one of 4 kinds. A read,
/// read_stream, write, and write_stream.
///
/// Unlike a [`self::RawThreadpoolIoHandle`], an OwnedThreadpoolIoHandle owns the resource and will
/// close the ThreadpoolIo resource on drop
/// See also:
/// (https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-createthreadpoolio)
pub struct OwnedThreadpoolIoHandle(PTP_IO);
impl OwnedThreadpoolIoHandle {
    pub fn new(
        maybe_env: Option<ThreadpoolCallbackEnvironment>,
        handle: HANDLE,
        callback: unsafe extern "system" fn(
            PTP_CALLBACK_INSTANCE,
            *mut c_void,
            *mut c_void,
            u32,
            usize,
            PTP_IO,
        ),
        cx: *mut c_void,
    ) -> io::Result<Self> {
        let env = maybe_env
            .map(|env| &env as *const _)
            .unwrap_or_else(std::ptr::null);
        match unsafe { CreateThreadpoolIo(handle as _, Some(callback), cx as _, env as _) } {
            0 => Err(io::Error::last_os_error()),
            handle => Ok(OwnedThreadpoolIoHandle(handle)),
        }
    }

    /// Similar to [`std::os::windows::io::OwnedHandle::as_handle`] but for
    /// [`OwnedThreadpoolIoHandle`]
    pub fn as_handle(&self) -> BorrowedThreadpoolIoHandle<'_> {
        BorrowedThreadpoolIoHandle {
            handle: self.0,
            _phantom: PhantomData,
        }
    }
}
impl Drop for OwnedThreadpoolIoHandle {
    fn drop(&mut self) {
        self.wait(WaitPending::Cancel);
        unsafe { CloseThreadpoolIo(self.as_raw_handle() as _) };
    }
}

pub struct BorrowedThreadpoolIoHandle<'handle> {
    handle: PTP_IO,
    _phantom: PhantomData<&'handle OwnedThreadpoolIoHandle>,
}

impl BorrowedThreadpoolIoHandle<'_> {
    /// Similar to [`std::os::windows::io::OwnedHandle::as_handle`] but for
    /// [`OwnedThreadpoolIoHandle`]
    pub fn as_handle(&self) -> BorrowedThreadpoolIoHandle<'_> {
        BorrowedThreadpoolIoHandle {
            handle: self.handle,
            _phantom: PhantomData,
        }
    }
}

impl AsRawHandle for BorrowedThreadpoolIoHandle<'_> {
    fn as_raw_handle(&self) -> RawHandle {
        self.handle as _
    }
}

// Windows handles can be shared across threads
unsafe impl Send for BorrowedThreadpoolIoHandle<'_> {}
unsafe impl Sync for BorrowedThreadpoolIoHandle<'_> {}

/// A borrowed handle to a ThreadpoolIo worker. Unlike a [`self::OwnedThreadpoolIoHandle`], a
/// RawThreadpoolIoHandle will not close the ThreadpoolIo resource on drop. This is useful as the
/// handle is shared by the kernel callbacks, which must not drop.
pub struct RawThreadpoolIoHandle(PTP_IO);

impl_fs_handle!(OwnedHandle);
impl_fs_handle!(BorrowedHandle<'_>);
impl_fs_handle!(RawFileHandle);

impl_raw_handle!(OwnedThreadpoolIoHandle);
impl_raw_handle!(RawThreadpoolIoHandle);

impl_threadpool_io_work!(OwnedThreadpoolIoHandle);
impl_threadpool_io_work!(BorrowedThreadpoolIoHandle<'_>);
impl_threadpool_io_work!(RawThreadpoolIoHandle);
