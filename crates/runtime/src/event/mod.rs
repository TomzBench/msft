//! event.rs

use windows_sys::Win32::{
    Foundation::{FALSE, HANDLE, TRUE, WAIT_ABANDONED, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{CreateEventW, ResetEvent, SetEvent, WaitForSingleObject, INFINITE},
};

use std::{
    error,
    ffi::OsString,
    fmt, io,
    os::windows::{
        io::{
            AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, HandleOrNull, OwnedHandle,
            RawHandle,
        },
        prelude::*,
    },
    time::Duration,
};

/// See [`OwnedEventHandle::anonymous`]
pub fn anonymous(reset: EventReset, state: EventInitialState) -> io::Result<OwnedEventHandle> {
    OwnedEventHandle::anonymous(reset, state)
}

/// See [`OwnedEventHandle::named`]
pub fn named<O>(
    name: O,
    reset: EventReset,
    state: EventInitialState,
) -> io::Result<OwnedEventHandle>
where
    O: Into<OsString>,
{
    OwnedEventHandle::named(name, reset, state)
}

/// The Win32 Event API is impled internally for Shared and Borrowed Event handles
/// See OwnedEventHandle::new for details
pub trait Event {
    /// Sets the specified event object to the signaled state.
    ///
    /// [See
    /// also](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-setevent)
    fn set(&self) -> std::io::Result<()>;

    /// Sets the specified event object to the nonsignaled state.
    /// [See
    /// also](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-resetevent)
    fn reset(&self) -> std::io::Result<()>;

    /// Wait for event with optional timeout
    ///
    /// [see also]
    /// (https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitforsingleobject)
    fn wait(&self, duration: Option<std::time::Duration>) -> Result<(), EventError>;
}

#[derive(Debug)]
pub enum EventError {
    Abandoned,
    Failed,
    Timeout,
    Io(io::Error),
}

impl fmt::Display for EventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventError::Abandoned => write!(f, "Event abandoned"),
            EventError::Failed => write!(f, "Event failed"),
            EventError::Timeout => write!(f, "Event timeout"),
            EventError::Io(e) => write!(f, "Event io error => {e}"),
        }
    }
}

impl error::Error for EventError {}

/// Windows CreateEvent creation argument
///
/// If this parameter is TRUE, the function creates a manual-reset event object,
/// which requires the use of the ResetEvent function to set the event state to
/// nonsignaled. If this parameter is FALSE, the function creates an auto-reset
/// event object, and the system automatically resets the event state to
/// nonsignaled after a single waiting thread has been released.
#[repr(i32)]
#[derive(PartialEq)]
pub enum EventReset {
    Manual = TRUE,
    Automatic = FALSE,
}

/// Windows CreateEvent creation argument
///
/// If this parameter is TRUE, the initial state of the event object is
/// signaled; otherwise, it is nonsignaled.
#[repr(i32)]
pub enum EventInitialState {
    Set = TRUE,
    Unset = FALSE,
}

/// Like [`OwnedHandle`] except extended with Event api
pub struct OwnedEventHandle(OwnedHandle);

impl OwnedEventHandle {
    /// Create a system event
    ///
    /// [CreateEventW](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-createeventa)
    pub fn named<O>(
        name: O,
        reset: EventReset,
        state: EventInitialState,
    ) -> io::Result<OwnedEventHandle>
    where
        O: Into<OsString>,
    {
        let kernel_name = name
            .into()
            .encode_wide()
            .chain(Some(0).into_iter())
            .collect::<Vec<_>>();
        Self::new_raw(kernel_name.as_ptr() as _, reset, state)
    }

    /// Create a system event with out a name
    ///
    /// [CreateEventW](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-createeventa)
    pub fn anonymous(reset: EventReset, state: EventInitialState) -> io::Result<OwnedEventHandle> {
        Self::new_raw(std::ptr::null(), reset, state)
    }

    pub fn new_raw(
        name: *const u16,
        reset: EventReset,
        state: EventInitialState,
    ) -> io::Result<OwnedEventHandle> {
        unsafe {
            let raw = CreateEventW(std::ptr::null(), reset as _, state as _, name);
            let handle = HandleOrNull::from_raw_handle(raw as _);
            OwnedHandle::try_from(handle).map_err(|_| io::Error::last_os_error())
        }
        .map(Self)
    }

    pub fn as_handle(&self) -> BorrowedEventHandle<'_> {
        BorrowedEventHandle(self.0.as_handle())
    }

    pub fn borrow_raw(&self) -> RawEventHandle {
        self.as_handle().borrow_raw()
    }
}

impl AsRawHandle for OwnedEventHandle {
    fn as_raw_handle(&self) -> RawHandle {
        self.0.as_raw_handle()
    }
}

impl FromRawHandle for OwnedEventHandle {
    unsafe fn from_raw_handle(handle: RawHandle) -> Self {
        Self(OwnedHandle::from_raw_handle(handle))
    }
}

/// Like [`BorrowedHandle`] except extended with Event api
pub struct BorrowedEventHandle<'handle>(BorrowedHandle<'handle>);

impl<'handle> BorrowedEventHandle<'handle> {
    pub fn borrow_raw(&self) -> RawEventHandle {
        RawEventHandle(self.as_raw_handle() as _)
    }
}

impl AsRawHandle for BorrowedEventHandle<'_> {
    fn as_raw_handle(&self) -> RawHandle {
        self.0.as_raw_handle()
    }
}

/// A weak reference to a handle, usually only useful in context of unsafe code
pub struct RawEventHandle(HANDLE);

impl AsRawHandle for RawEventHandle {
    fn as_raw_handle(&self) -> RawHandle {
        self.0 as _
    }
}

macro_rules! impl_event {
    ($handle:ty) => {
        impl Event for $handle {
            fn set(&self) -> io::Result<()> {
                self::set(self.as_raw_handle() as _)
            }

            fn reset(&self) -> io::Result<()> {
                self::reset(self.as_raw_handle() as _)
            }

            fn wait(&self, duration: Option<Duration>) -> Result<(), EventError> {
                self::wait(self.as_raw_handle() as _, duration)
            }
        }
    };
}

impl_event!(OwnedEventHandle);
impl_event!(BorrowedEventHandle<'_>);
impl_event!(RawEventHandle);

#[inline(always)]
fn set(handle: HANDLE) -> io::Result<()> {
    match unsafe { SetEvent(handle) } {
        FALSE => Err(io::Error::last_os_error()),
        _ => Ok(()),
    }
}

#[inline(always)]
fn reset(handle: HANDLE) -> io::Result<()> {
    match unsafe { ResetEvent(handle) } {
        FALSE => Err(io::Error::last_os_error()),
        _ => Ok(()),
    }
}

#[inline(always)]
fn wait(handle: HANDLE, duration: Option<Duration>) -> Result<(), EventError> {
    let dur: u32 = duration.map(|d| d.as_millis() as _).unwrap_or(INFINITE);
    match unsafe { WaitForSingleObject(handle, dur as _) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_ABANDONED => Err(EventError::Abandoned),
        WAIT_FAILED => Err(EventError::Failed),
        WAIT_TIMEOUT => Err(EventError::Timeout),
        _ => Err(EventError::Io(io::Error::last_os_error())),
    }
}
