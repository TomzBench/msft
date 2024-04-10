//! write

use crate::io::{Overlapped, OverlappedError};
pub use std::{
    ops::DerefMut,
    os::windows::io::{AsRawHandle, BorrowedHandle, OwnedHandle, RawHandle},
};
use windows_sys::Win32::{
    Foundation::{GetLastError, ERROR_IO_PENDING, FALSE},
    Storage::FileSystem::WriteFile,
};

#[inline(always)]
pub fn write_overlapped(
    handle: RawHandle,
    overlapped: &mut Overlapped,
    bytes: &mut [u8],
) -> Result<usize, OverlappedError> {
    let mut bytes_written = 0u32;
    let result = unsafe {
        WriteFile(
            handle as _,
            bytes.as_mut_ptr(),
            bytes.len() as _,
            &mut bytes_written,
            overlapped.as_mut_ptr(),
        )
    };
    match result {
        FALSE => match unsafe { GetLastError() } {
            ERROR_IO_PENDING => Err(OverlappedError::Pending),
            raw => Err(OverlappedError::Os(raw as _)),
        },
        _ => Ok(bytes_written as _),
    }
}
