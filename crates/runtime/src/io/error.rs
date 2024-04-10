//! drive

use crate::io::overlapped::OverlappedError;
use std::{error, fmt};

#[derive(Debug)]
pub enum SinkError {
    Overlapped(OverlappedError),
    Cancelled,
    BufferFull,
}
impl fmt::Display for SinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Overlapped(e) => e.fmt(f),
            Self::Cancelled => write!(f, "cancelled"),
            Self::BufferFull => write!(f, "io write stream buffer full"),
        }
    }
}
impl error::Error for SinkError {}

impl From<OverlappedError> for SinkError {
    fn from(value: OverlappedError) -> Self {
        Self::Overlapped(value)
    }
}

#[derive(Debug)]
pub enum SinkEncodeError<E> {
    Encode(E),
    BufferFull,
}
impl<E: error::Error> fmt::Display for SinkEncodeError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(e) => write!(f, "encoder error => {e}"),
            Self::BufferFull => write!(f, "sink buffer full"),
        }
    }
}
impl<E: error::Error> error::Error for SinkEncodeError<E> {}

#[derive(Debug)]
pub enum StreamError<E: error::Error> {
    Overlapped(OverlappedError),
    Decode(E),
    QueueFull,
}
impl<E: error::Error> error::Error for StreamError<E> {}
impl<E: error::Error> fmt::Display for StreamError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Overlapped(e) => e.fmt(f),
            Self::Decode(e) => write!(f, "decoder error => {e}"),
            Self::QueueFull => write!(f, "io read stream queue full"),
        }
    }
}
impl<E: error::Error> From<OverlappedError> for StreamError<E> {
    fn from(value: OverlappedError) -> Self {
        Self::Overlapped(value)
    }
}
