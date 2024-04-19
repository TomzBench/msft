//! futures
use crate::{
    codec::Decode,
    io::{
        drive::Drive,
        error::{SinkError, StreamError},
        overlapped::ReadOverlapped,
    },
};
use futures::{Future, Stream};
use std::{
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll},
};

pub struct DecodeStream<R, D>
where
    R: ReadOverlapped,
    D: Decode,
{
    /// Shared state between the stream and the threadpool
    drive: Arc<Drive<R, D>>,
    /// Stream ended, because we overflowed the queue, or the remote stream has been closed
    overflow: AtomicBool,
}

impl<R, D> DecodeStream<R, D>
where
    R: ReadOverlapped,
    D: Decode,
{
    pub(in crate::io) fn new(drive: Arc<Drive<R, D>>) -> Self {
        Self {
            drive,
            overflow: AtomicBool::new(false),
        }
    }
}

impl<R, D> Stream for DecodeStream<R, D>
where
    R: ReadOverlapped,
    D: Decode,
{
    type Item = Result<D::Item, StreamError<D::Error>>;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let next = self.drive.reader().poll_next(cx);
        if let Poll::Ready(Some(Err(StreamError::QueueFull))) = next {
            self.overflow.store(true, Ordering::SeqCst);
            Poll::Ready(None)
        } else {
            next
        }
    }
}

pub struct Flush<W, D>
where
    D: Decode,
{
    drive: Arc<Drive<W, D>>,
}

impl<W, D> Flush<W, D>
where
    D: Decode,
{
    pub(in crate::io) fn new(drive: Arc<Drive<W, D>>) -> Flush<W, D> {
        Flush { drive }
    }
}

impl<W, D> Future for Flush<W, D>
where
    D: Decode,
{
    type Output = Result<(), SinkError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.drive.writer().poll(cx)
    }
}
