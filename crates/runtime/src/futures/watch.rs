//! signal
use futures::{ready, Stream};
use parking_lot::Mutex;
use pin_project_lite::pin_project;
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

#[derive(Debug, Default)]
pub struct Inner {
    signal: bool,
    waker: Option<Waker>,
}

impl Inner {
    fn signal(&mut self) {
        self.signal = true;
        if let Some(waker) = self.waker.as_ref() {
            waker.wake_by_ref()
        }
    }
}

#[derive(Debug, Default)]
pub struct Signal {
    shared: Arc<Mutex<Inner>>,
}

impl Future for Signal {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut shared = self.shared.lock();
        let new_waker = cx.waker();

        // Are we signalled yet
        match shared.signal {
            true => Poll::Ready(()),
            false => {
                // Some waker accounting
                shared.waker = match shared.waker.take() {
                    Some(old_waker) => match old_waker.will_wake(new_waker) {
                        true => Some(old_waker),
                        false => Some(new_waker.clone()),
                    },
                    None => Some(new_waker.clone()),
                };
                Poll::Pending
            }
        }
    }
}

pin_project! {
    #[project = WatchProj]
    #[project_replace = WatchProjReplace]
    #[derive(Debug)]
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    pub enum Watch<I> {
        Incomplete {
            #[pin]
            inner: I,
            signal: Arc<Mutex<Inner>>
        },
        Complete,
    }
}

impl<Fut> Watch<Fut>
where
    Fut: Future,
{
    pub(in crate::futures) fn future(inner: Fut) -> (Signal, Watch<Fut>) {
        let signal = Signal::default();
        let watch = Watch::Incomplete {
            inner,
            signal: Arc::clone(&signal.shared),
        };
        (signal, watch)
    }
}

impl<St> Watch<St>
where
    St: Stream,
{
    pub(in crate::futures) fn stream(inner: St) -> (Signal, Watch<St>) {
        let signal = Signal::default();
        let watch = Watch::Incomplete {
            inner,
            signal: Arc::clone(&signal.shared),
        };
        (signal, watch)
    }
}

impl<I> Watch<I> {
    /// Return a reference to the future if the future has not completed yet
    pub fn inner(&self) -> Option<&I> {
        match self {
            Watch::Incomplete { inner, .. } => Some(&inner),
            _ => None,
        }
    }
}

impl<F> Future for Watch<F>
where
    F: Future,
{
    type Output = F::Output;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.as_mut().project() {
            WatchProj::Incomplete { inner, .. } => {
                let output = ready!(inner.poll(cx));
                match self.project_replace(Watch::Complete) {
                    WatchProjReplace::Incomplete { signal, .. } => {
                        signal.lock().signal();
                        Poll::Ready(output)
                    }
                    WatchProjReplace::Complete => unreachable!(),
                }
            }
            WatchProj::Complete => {
                panic!("Watch must not be polled after it returned `Poll::Ready`")
            }
        }
    }
}

impl<S> Stream for Watch<S>
where
    S: Stream,
{
    type Item = S::Item;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.as_mut().project() {
            WatchProj::Incomplete { inner, .. } => match inner.poll_next(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Some(item)) => Poll::Ready(Some(item)),
                Poll::Ready(None) => match self.project_replace(Watch::Complete) {
                    WatchProjReplace::Incomplete { signal, .. } => {
                        signal.lock().signal();
                        Poll::Ready(None)
                    }
                    WatchProjReplace::Complete => unreachable!(),
                },
            },
            WatchProj::Complete => {
                panic!("Watch must not be polled after stream has finished")
            }
        }
    }
}
