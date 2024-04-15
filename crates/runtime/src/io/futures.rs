//! futures
use crate::{
    codec::Decode,
    io::{
        drive::{ReadDriver, WriteDriver},
        error::{SinkError, StreamError},
        overlapped::ReadOverlapped,
    },
};
use futures::{Future, Stream};
use std::{
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll},
};

pub struct DecodeStream<'drive, R, D>
where
    R: ReadOverlapped,
    D: Decode,
{
    /// Shared state between the stream and the threadpool
    drive: &'drive ReadDriver<R, D>,
    /// Stream ended, because we overflowed the queue, or the remote stream has been closed
    overflow: AtomicBool,
}

impl<'drive, R, D> DecodeStream<'drive, R, D>
where
    R: ReadOverlapped,
    D: Decode,
{
    pub(in crate::io) fn new(drive: &'drive ReadDriver<R, D>) -> Self {
        Self {
            drive,
            overflow: AtomicBool::new(false),
        }
    }
}

impl<R, D> Stream for DecodeStream<'_, R, D>
where
    R: ReadOverlapped,
    D: Decode,
{
    type Item = Result<D::Item, StreamError<D::Error>>;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let next = self.drive.poll_next(cx);
        if let Poll::Ready(Some(Err(StreamError::QueueFull))) = next {
            self.overflow.store(true, Ordering::SeqCst);
            Poll::Ready(None)
        } else {
            next
        }
    }
}

pub struct Flush<'drive, W> {
    drive: &'drive WriteDriver<W>,
}

impl<'drive, W> Flush<'drive, W> {
    pub(in crate::io) fn new(drive: &'drive WriteDriver<W>) -> Flush<'drive, W> {
        Flush { drive }
    }
}

impl<'drive, W> Future for Flush<'drive, W> {
    type Output = Result<(), SinkError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.drive.poll(cx)
    }
}
