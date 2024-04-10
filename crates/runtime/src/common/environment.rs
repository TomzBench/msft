//! environment
use std::{
    io::{Error, Result},
    mem,
    os::windows::prelude::{AsRawHandle, FromRawHandle, RawHandle},
};
use windows_sys::Win32::{
    Foundation::{FALSE, HMODULE, TRUE},
    System::Threading::*,
};

/// Threadpool
///
/// [See also]
/// (https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-createthreadpool)
pub struct ThreadpoolHandle(PTP_POOL);
impl ThreadpoolHandle {
    /// Create a threadpool
    ///
    /// [See also]
    /// (https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-createthreadpool)
    pub fn new() -> Result<Self> {
        match unsafe { CreateThreadpool(std::ptr::null_mut()) } {
            0 => Err(Error::last_os_error()),
            handle => Ok(Self(handle)),
        }
    }

    /// Set the stack sizes for the threadpool
    ///
    /// [See also]
    /// (https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-setthreadpoolstackinformation)
    pub fn set_stack_size(&self, reserve: usize, commit: usize) -> Result<&Self> {
        let stack = TP_POOL_STACK_INFORMATION {
            StackReserve: reserve,
            StackCommit: commit,
        };
        match unsafe { SetThreadpoolStackInformation(self.0, &stack as *const _ as _) } {
            FALSE => Err(Error::last_os_error()),
            _ => Ok(self),
        }
    }

    /// Set the minimum and maximum number of threads
    ///
    /// [See also]
    /// (https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-setthreadpoolthreadminimum)
    pub fn min_threads(&self, min: u32) -> Result<&Self> {
        unsafe {
            match SetThreadpoolThreadMinimum(self.0, min) {
                TRUE => Ok(self),
                _ => Err(Error::last_os_error()),
            }
        }
    }

    /// Set the minimum and maximum number of threads
    ///
    /// [See also]
    /// (https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-setthreadpoolthreadmaximum)
    pub fn max_threads(&self, max: u32) -> &Self {
        unsafe { SetThreadpoolThreadMaximum(self.0, max) };
        self
    }

    /// Helper function to create a new thread pool environment associated with this threadpool
    pub fn new_environment(&self) -> ThreadpoolCallbackEnvironment {
        ThreadpoolCallbackEnvironment::new().with_pool(self.as_raw_handle() as _)
    }
}

impl Drop for ThreadpoolHandle {
    /// [See also]
    /// (https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-closethreadpool)
    fn drop(&mut self) {
        unsafe { CloseThreadpool(self.0) }
    }
}

impl AsRawHandle for ThreadpoolHandle {
    fn as_raw_handle(&self) -> RawHandle {
        self.0 as _
    }
}

impl FromRawHandle for ThreadpoolHandle {
    unsafe fn from_raw_handle(handle: RawHandle) -> Self {
        ThreadpoolHandle(handle as _)
    }
}

#[repr(i32)]
pub enum ThreadpoolPriority {
    Low = TP_CALLBACK_PRIORITY_LOW,
    Normal = TP_CALLBACK_PRIORITY_NORMAL,
    High = TP_CALLBACK_PRIORITY_HIGH,
}
impl ThreadpoolPriority {
    pub fn raw(&self) -> i32 {
        // safety: https://doc.rust-lang.org/reference/items/enumerations.html#pointer-casting
        // If the enumeration specifies a primitive representation, then the discriminant may
        // be reliably accessed via unsafe pointer casting:
        unsafe { *(self as *const Self as *const i32) }
    }
}

/// A ThreadpoolCallbackEnvironment
///
/// [See alse]
/// (https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-initializethreadpoolenvironment)
pub struct ThreadpoolCallbackEnvironment(TP_CALLBACK_ENVIRON_V3);
impl ThreadpoolCallbackEnvironment {
    /// Initialize a default ThreadpoolCallbackEnvironment
    ///
    /// NOTE: This is a macro function (not exported from windows_sys crate).
    ///
    /// [See also]
    /// (https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-initializethreadpoolenvironment)
    #[inline(always)]
    pub fn new() -> Self {
        let mut env = unsafe { mem::zeroed::<TP_CALLBACK_ENVIRON_V3>() };
        env.Version = 3;
        env.CallbackPriority = TP_CALLBACK_PRIORITY_NORMAL;
        env.Size = mem::size_of::<TP_CALLBACK_ENVIRON_V3>() as _;
        Self(env)
    }

    pub fn as_raw(&self) -> *const TP_CALLBACK_ENVIRON_V3 {
        &self.0 as _
    }

    /// Set the threadpool callback pool. If no pool is set, then the default threadpool is used.
    ///
    /// NOTE: This is a macro function (not exported from windows_sys crate).
    ///
    /// [See also]
    /// (https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setthreadpoolcallbackpool)
    #[inline(always)]
    pub fn with_pool(mut self, pool: PTP_POOL) -> Self {
        self.0.Pool = pool as _;
        self
    }

    /// Set the threadpool priority in relation to other threads in this pool
    ///
    /// NOTE: This is a macro function (not exported from windows_sys crate).
    ///
    /// [See also]
    /// https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setthreadpoolcallbackpriority
    #[inline(always)]
    pub fn with_priority(mut self, prio: ThreadpoolPriority) -> Self {
        self.0.CallbackPriority = prio.raw();
        self
    }

    /// Indicate that the threadpool may run long
    ///
    /// NOTE: This is a macro function (not exported from windows_sys crate).
    ///
    /// [See also]
    /// https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setthreadpoolcallbackpriority
    #[inline(always)]
    pub fn runs_long(mut self) -> Self {
        self.0.u.s._bitfield = 1;
        self
    }

    /// Ensures that the specified DLL remains loaded as long as there are outstanding callbacks
    ///
    /// NOTE: This is a macro function (not exported from windows_sys crate).
    ///
    /// [See also]
    /// https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setthreadpoolcallbacklibrary
    #[inline(always)]
    pub fn from_dll(mut self, handle: HMODULE) -> Self {
        self.0.RaceDll = handle as _;
        self
    }

    /// Associates the specified cleanup group with the specified callback environment
    ///
    /// NOTE: This is a macro function (not exported from windows_sys crate).
    ///
    /// [See also]
    /// https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setthreadpoolcallbackcleanupgroup
    #[inline(always)]
    pub fn with_cleanup_group(
        mut self,
        group: PTP_CLEANUP_GROUP,
        cancel_callback: PTP_CLEANUP_GROUP_CANCEL_CALLBACK,
    ) -> Self {
        self.0.CleanupGroup = group;
        self.0.CleanupGroupCancelCallback = cancel_callback;
        self
    }
}

impl Drop for ThreadpoolCallbackEnvironment {
    /// NOTE: This is a macro function (not exported from windows_sys crate).
    ///
    /// [See also]
    /// (https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-destroythreadpoolenvironment)
    fn drop(&mut self) {
        // NOTE: DestroyThreadpoolEnvironment currently doesn't do anything but may change in
        // future release
    }
}

impl AsRef<TP_CALLBACK_ENVIRON_V3> for ThreadpoolCallbackEnvironment {
    fn as_ref(&self) -> &TP_CALLBACK_ENVIRON_V3 {
        &self.0
    }
}

impl From<TP_CALLBACK_ENVIRON_V3> for ThreadpoolCallbackEnvironment {
    fn from(value: TP_CALLBACK_ENVIRON_V3) -> Self {
        ThreadpoolCallbackEnvironment(value)
    }
}

impl From<ThreadpoolCallbackEnvironment> for TP_CALLBACK_ENVIRON_V3 {
    fn from(value: ThreadpoolCallbackEnvironment) -> Self {
        value.0
    }
}
