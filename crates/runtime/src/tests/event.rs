use std::os::windows::io::AsRawHandle;

use crate::{
    event::{Event, EventInitialState, EventReset},
    wait::{WaitError, WaitPool},
};
use futures::FutureExt;

#[test]
fn threadpool_test_event() {
    // Create a test waker
    let waker = futures::task::noop_waker_ref();
    let mut cx = std::task::Context::from_waker(waker);

    // Create an anonymous manually resetable event
    let ev = crate::event::anonymous(EventReset::Manual, EventInitialState::Unset).unwrap();
    let mut pool = WaitPool::new().unwrap();
    let mut fut = pool.start(ev.as_raw_handle() as _, None);

    // Make sure cannot wait twice and err InProgres
    let err = pool.restart(ev.as_raw_handle() as _, None);
    assert!(err.is_err());
    assert_eq!(WaitError::InProgress, err.unwrap_err());

    // Make sure we are pending
    let poll = fut.poll_unpin(&mut cx);
    assert!(poll.is_pending());

    // Make sure we set event and are no longer pending anymore
    // NOTE we set the time delay to allow kernel some time to drive our future
    ev.set().unwrap();
    std::thread::sleep(std::time::Duration::from_nanos(1));
    let poll = fut.poll_unpin(&mut cx);
    assert!(poll.is_ready());

    // Reset the event and listen again. (No longer in progress)
    ev.reset().unwrap();
    let mut fut = pool.restart(ev.as_raw_handle() as _, None).unwrap();

    // Make sure new future is still pending
    let poll = fut.poll_unpin(&mut cx);
    assert!(poll.is_pending());

    // Make sure we set event and are no longer pending anymore
    // NOTE we set the time delay to allow kernel some time to drive our future
    ev.set().unwrap();
    std::thread::sleep(std::time::Duration::from_nanos(1));
    let poll = fut.poll_unpin(&mut cx);
    assert!(poll.is_ready());
}
