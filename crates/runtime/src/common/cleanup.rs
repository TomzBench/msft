//! cleanup
use std::{
    ffi::c_void,
    os::windows::prelude::{AsRawHandle, RawHandle},
};
use windows_sys::Win32::{Foundation::TRUE, System::Threading::*};

/// A Cleanup group
///
/// NOTE: Ownership is currently a footgun atm.
///       - If you expect a cleanup group to clean your waitable handles, then you
///         must into_inner your HANDLES and only work with RAW handles, so that no
///         drop functions are called. Because if a HANDLE wrapped by a unit type
///         with a drop function drops the handle, and the cleanup group is also
///         going to drop the handle, then you have double free error.
///       - If you do not rely on a Cleanup group, then you must allow handle
///         drop functions to run in order to free the handles.
///       - In summary, only deal with raw HANDLES (as opposed to something like
///         an Event(HANDLE)) if you plan on dealing with a Cleanup Group.
///         use handle.into_raw() to mem forget and allow Cleanup group to close
///         the handle.
///
/// [See also]
/// (https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-createthreadpoolcleanupgroup)
pub struct ThreadpoolCleanupGroup(PTP_CLEANUP_GROUP, *mut c_void);

impl ThreadpoolCleanupGroup {
    /// Create a threadpool cleanup group
    ///
    /// [See Also]
    /// (https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-seteventwhencallbackreturns)
    pub fn new() -> Self {
        unsafe { Self(CreateThreadpoolCleanupGroup(), std::ptr::null_mut()) }
    }

    pub fn with_context(mut self, ctx: *mut c_void) -> Self {
        self.1 = ctx;
        self
    }
}

impl Drop for ThreadpoolCleanupGroup {
    fn drop(&mut self) {
        unsafe { CloseThreadpoolCleanupGroupMembers(self.0, TRUE, self.1) };
        unsafe { CloseThreadpoolCleanupGroup(self.0) };
    }
}

impl AsRawHandle for ThreadpoolCleanupGroup {
    fn as_raw_handle(&self) -> RawHandle {
        self.0 as _
    }
}
