use super::{Overlapped, OverlappedKind};
use bytes::Buf;
use std::{ffi::c_void, mem};
use windows_sys::Win32::System::IO::{OVERLAPPED, OVERLAPPED_0, OVERLAPPED_0_0};

#[test]
fn test_threadpool_buffer() {
    let mut bytes = bytes::BytesMut::with_capacity(8);
    let slice = unsafe { std::slice::from_raw_parts_mut(bytes.as_mut_ptr(), bytes.capacity()) };
    slice.copy_from_slice(b"01234567");
    assert_eq!(b"", &bytes[..]);

    unsafe { bytes.set_len(4) };
    assert_eq!(b"0123", &bytes[..]);

    unsafe { bytes.set_len(8) };
    assert_eq!(b"01234567", &bytes[..]);
    assert_eq!(8, bytes.len());

    bytes.advance(4);
    assert_eq!(4, bytes.len());
    assert_eq!(b"4567", &bytes[..]);
}

#[test]
fn test_threadpool_overlapped() {
    let mut newtyp = Overlapped {
        internal: 0,
        internal_high: 1,
        offset: 2,
        offset_high: 3,
        hevent: 4,
        kind: OverlappedKind::Write,
    };
    let expect = OVERLAPPED {
        Internal: 0,
        InternalHigh: 1,
        Anonymous: OVERLAPPED_0 {
            Anonymous: OVERLAPPED_0_0 {
                Offset: 2,
                OffsetHigh: 3,
            },
        },
        hEvent: 4,
    };
    let from_newtyp = unsafe { *newtyp.as_ptr() };
    assert_eq!(
        mem::size_of::<OVERLAPPED>(),
        mem::size_of::<Overlapped>() - mem::size_of::<usize>()
    );
    assert_eq!(expect.Internal, from_newtyp.Internal);
    assert_eq!(expect.InternalHigh, from_newtyp.InternalHigh);
    assert_eq!(expect.hEvent, from_newtyp.hEvent);
    unsafe {
        assert_eq!(
            expect.Anonymous.Anonymous.Offset,
            from_newtyp.Anonymous.Anonymous.Offset
        );
        assert_eq!(
            expect.Anonymous.Anonymous.OffsetHigh,
            from_newtyp.Anonymous.Anonymous.OffsetHigh
        );
    }

    // Silly sanity check
    let raw: *mut c_void = &mut newtyp as *mut Overlapped as *mut _;
    let cast: &mut Overlapped = unsafe { &mut *(raw as *mut _) };
    assert_eq!(OverlappedKind::Write, cast.kind);
    assert_eq!(raw as usize, cast as *const Overlapped as usize);
}

#[test]
fn test_threadpool_overlapped_offset() {
    let mut overlapped = Overlapped::new_read(1234567890);
    assert_eq!(1234567890, overlapped.get_offset());
    overlapped.set_offset(8234567891);
    assert_eq!(8234567891, overlapped.get_offset());
}
