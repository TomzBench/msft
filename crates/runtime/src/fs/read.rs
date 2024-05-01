//! read

use crate::io::{Overlapped, OverlappedError};
use bytes::BytesMut;
pub use std::{
    ops::DerefMut,
    os::windows::io::{AsRawHandle, BorrowedHandle, OwnedHandle, RawHandle},
};
use tracing::debug;
use windows_sys::Win32::{
    Foundation::{GetLastError, ERROR_IO_PENDING, FALSE},
    Storage::FileSystem::ReadFile,
};

#[inline(always)]
pub fn read_overlapped(
    handle: RawHandle,
    overlapped: &mut Overlapped,
    bytes: &mut BytesMut,
) -> Result<usize, OverlappedError> {
    let mut bytes_read = 0u32;
    let result = unsafe {
        ReadFile(
            handle as _,
            bytes.as_mut_ptr(),
            bytes.capacity() as _,
            &mut bytes_read,
            overlapped.as_mut_ptr(),
        )
    };
    debug!(
        len = bytes.len(),
        cap = bytes.capacity(),
        bytes_read,
        result
    );
    match result {
        FALSE => match unsafe { GetLastError() } {
            ERROR_IO_PENDING => Err(OverlappedError::Pending),
            raw => Err(OverlappedError::Os(raw as _)),
        },
        _ => Ok(bytes_read as _),
    }
}
