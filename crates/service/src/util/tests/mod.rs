use futures::FutureExt;

use super::guid::Guid;
use super::wait::{self, Event, EventInitialState, EventListener, EventReset, WaitError};
use super::wchar::from_wide;

#[test]
fn service_test_util_wchar_arr() {
    // UTF-16 encoding for "Unicode\0"
    let s: &[u16] = &[
        0x0055, 0x006E, 0x0069, 0x0063, 0x006F, 0x0064, 0x0065, 0x0000,
    ];
    let p = &(&s[0] as *const u16) as *const *const u16;
    let term = unsafe { from_wide(*p) };
    assert_eq!("Unicode", term);
}

#[test]
fn service_test_util_wchar() {
    let s: &[u8] = b"\x55\x00\x6E\x00\x69\x00\x63\x00\x6f\x00\x64\x00\x65\x00\x00";
    let term = unsafe { from_wide(s.as_ptr() as *const _) };
    assert_eq!("Unicode", term);
}

#[test]
fn service_test_guid_from_str() {
    let ok = Guid::new("a9214533-3f5f-475b-8140-cb96b289270b");
    assert!(ok.is_ok());
    let fail = Guid::new("foo");
    assert!(fail.is_err());
}

#[test]
fn service_test_util_wait() {
    // Create a test waker
    let waker = futures::task::noop_waker_ref();
    let mut cx = std::task::Context::from_waker(waker);

    // Create an anonymous manually resetable event
    let ev = Event::anonymous(EventReset::Manual, EventInitialState::Unset).unwrap();
    let pool = EventListener::new().unwrap();
    let mut fut = pool.start(&ev, None);

    // Make sure cannot wait twice and err InProgres
    let err = pool.restart(&ev, None);
    assert!(err.is_err());
    assert_eq!(WaitError::InProgress, err.unwrap_err());

    // Make sure we are pending
    let poll = fut.poll_unpin(&mut cx);
    assert!(poll.is_pending());

    // Make sure we set event and are no longer pending anymore
    // NOTE we set the time delay to allow kernel some time to drive our future
    ev.set().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1));
    let poll = fut.poll_unpin(&mut cx);
    assert!(poll.is_ready());

    // Reset the event and listen again. (No longer in progress)
    ev.reset().unwrap();
    let mut fut = pool.restart(&ev, None).unwrap();

    // Make sure new future is still pending
    let poll = fut.poll_unpin(&mut cx);
    assert!(poll.is_pending());

    // Make sure we set event and are no longer pending anymore
    // NOTE we set the time delay to allow kernel some time to drive our future
    ev.set().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1));
    let poll = fut.poll_unpin(&mut cx);
    assert!(poll.is_ready());
}

#[test]
fn service_test_util_oneshot() {
    // Create a test waker
    let waker = futures::task::noop_waker_ref();
    let mut cx = std::task::Context::from_waker(waker);

    // Create a channel signal
    let (sender, mut receiver) = wait::oneshot().unwrap();

    // Make sure we are pending
    let poll = receiver.poll_unpin(&mut cx);
    assert!(poll.is_pending());

    // Make sure we set event and are no longer pending anymore
    // NOTE we set the time delay to allow kernel some time to drive our future
    sender.set().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1));
    let poll = receiver.poll_unpin(&mut cx);
    assert!(poll.is_ready());
}
