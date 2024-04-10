//! instance

use std::{
    io::{Error, ErrorKind, Result},
    os::windows::prelude::{AsRawHandle, FromRawHandle, RawHandle},
};
use windows_sys::Win32::{
    Foundation::{FALSE, HANDLE, HMODULE},
    System::Threading::*,
};

/// A ThreadpoolCallbackInstance is passed to many of the threadpool callback functions
///
/// [See Also](https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-seteventwhencallbackreturns)
pub struct ThreadpoolCallbackInstance(PTP_CALLBACK_INSTANCE);
impl ThreadpoolCallbackInstance {
    /// Provide a hint to the threadpool that this callback may run long.  The thread pool will not
    /// limit this thread to normal limits and use another thread to service the next request. It
    /// will fail if the kernel refuses to do so.
    ///
    /// [See also]
    /// (https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-callbackmayrunlong)
    pub fn callback_may_run_long(&self) -> Result<&Self> {
        match unsafe { CallbackMayRunLong(self.0) } {
            FALSE => Err(Error::new(ErrorKind::Other, "Busy")),
            _ => Ok(self),
        }
    }

    /// Set an event when the callback returns
    ///
    /// [See also]
    /// (https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-seteventwhencallbackreturns)
    pub fn set_event_when_callback_returns(&self, event: HANDLE) -> &Self {
        unsafe { SetEventWhenCallbackReturns(self.0, event) }
        self
    }

    /// Release a semaphore when callback returns
    ///
    /// [See also]
    /// (https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-releasesemaphorewhencallbackreturns)
    pub fn release_semaphore_when_callback_returns(&self, sem: HANDLE, crel: u32) -> &Self {
        unsafe { ReleaseSemaphoreWhenCallbackReturns(self.0, sem, crel) };
        self
    }

    /// Release a mutex when callback returns
    ///
    /// [See also]
    /// (https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-releasemutexwhencallbackreturns)
    pub fn release_mutex_when_callback_returns(&self, mtx: HANDLE) -> &Self {
        unsafe { ReleaseMutexWhenCallbackReturns(self.0, mtx) };
        self
    }

    /// Leave a critical section when the callback returns
    ///
    /// [See also]
    /// (https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-leavecriticalsectionwhencallbackreturns)
    pub fn leave_critical_section_when_callback_returns(
        &self,
        pcs: *mut CRITICAL_SECTION,
    ) -> &Self {
        unsafe { LeaveCriticalSectionWhenCallbackReturns(self.0, pcs) };
        self
    }

    /// Unload a DLL when the callback returns
    ///
    /// [See also]
    /// (https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-freelibrarywhencallbackreturns)
    pub fn free_library_when_callback_returns(&self, handle: HMODULE) -> &Self {
        unsafe { FreeLibraryWhenCallbackReturns(self.0, handle) };
        self
    }
}

impl AsRawHandle for ThreadpoolCallbackInstance {
    fn as_raw_handle(&self) -> RawHandle {
        self.0 as _
    }
}

impl FromRawHandle for ThreadpoolCallbackInstance {
    unsafe fn from_raw_handle(handle: RawHandle) -> Self {
        ThreadpoolCallbackInstance(handle as _)
    }
}
