//! trait

use futures::Stream;
use std::future::Future;
mod watch;

pub use watch::{Signal, Watch};

impl<T: ?Sized> FuturesExt for T where T: Future {}

impl<T: ?Sized> StreamExt for T where T: Stream {}

pub trait FuturesExt: Future {
    fn watch(self) -> (Signal, Watch<Self>)
    where
        Self: Sized,
    {
        Watch::future(self)
    }
}

pub trait StreamExt: Stream {
    fn watch(self) -> (Signal, Watch<Self>)
    where
        Self: Sized,
    {
        Watch::stream(self)
    }
}
