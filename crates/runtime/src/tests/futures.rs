//! futures

use crate::futures::{FuturesExt as TestFuturesExt, StreamExt as TestStreamExt};
use futures::{future::poll_fn, stream::poll_fn as poll_next_fn, FutureExt, StreamExt};
use std::{
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
    task::Poll,
};

#[test]
fn test_threadpool_future_watch() {
    // Create a test waker
    let waker = futures::task::noop_waker_ref();
    let mut cx = std::task::Context::from_waker(waker);

    // A mock future
    let ready = AtomicBool::new(false);
    let mock = poll_fn(|_cx| match ready.load(Ordering::SeqCst) {
        true => Poll::Ready(42),
        false => Poll::Pending,
    });

    // Thing under test
    let (mut signal, mut fut) = mock.watch();

    // Everything is pending...
    assert!(fut.poll_unpin(&mut cx).is_pending());
    assert!(signal.poll_unpin(&mut cx).is_pending());

    // Future is Ready.  signal will mirror ready after future is polled
    ready.store(true, Ordering::SeqCst);
    assert!(signal.poll_unpin(&mut cx).is_pending());
    assert_eq!(Poll::Ready(42), fut.poll_unpin(&mut cx));
    assert_eq!(Poll::Ready(()), signal.poll_unpin(&mut cx));
}

#[test]
fn test_threadpool_stream_watch() {
    // Create a test waker
    let waker = futures::task::noop_waker_ref();
    let mut cx = std::task::Context::from_waker(waker);

    // A mock stream
    let ready = AtomicU8::new(0);
    let mock = poll_next_fn(|_cx| match ready.load(Ordering::SeqCst) {
        0 => Poll::Pending,
        1 => Poll::Ready(Some(42)),
        _ => Poll::Ready(None),
    });

    // Thing under test
    let (mut signal, mut st) = mock.watch();

    // Everything is pending...
    assert!(st.poll_next_unpin(&mut cx).is_pending());
    assert!(signal.poll_unpin(&mut cx).is_pending());

    // Get an item out of stream, ensure signal still pending
    ready.store(1, Ordering::SeqCst);
    assert_eq!(Poll::Ready(Some(42)), st.poll_next_unpin(&mut cx));
    assert_eq!(Poll::Pending, signal.poll_unpin(&mut cx));

    // Get another item out of stream, ensure signal still pending
    ready.store(1, Ordering::SeqCst);
    assert_eq!(Poll::Ready(Some(42)), st.poll_next_unpin(&mut cx));
    assert_eq!(Poll::Pending, signal.poll_unpin(&mut cx));

    // more pending
    ready.store(0, Ordering::SeqCst);
    assert_eq!(Poll::Pending, st.poll_next_unpin(&mut cx));
    assert_eq!(Poll::Pending, signal.poll_unpin(&mut cx));

    // more stream
    ready.store(1, Ordering::SeqCst);
    assert_eq!(Poll::Ready(Some(42)), st.poll_next_unpin(&mut cx));
    assert_eq!(Poll::Pending, signal.poll_unpin(&mut cx));

    // All done
    ready.store(2, Ordering::SeqCst);
    assert!(signal.poll_unpin(&mut cx).is_pending());
    assert_eq!(Poll::Ready(None), st.poll_next_unpin(&mut cx));
    assert_eq!(Poll::Ready(()), signal.poll_unpin(&mut cx));
}
