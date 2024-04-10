//! drive
use crate::{
    codec::{Decode, SinkEncode, SinkEncodeLen},
    io::{
        error::{SinkEncodeError, SinkError, StreamError},
        handle::ThreadpoolIoWork,
        overlapped::{
            Overlapped, OverlappedError, ReadOverlapped, ReadOverlappedEx, WriteOverlapped,
        },
    },
};
use bytes::{Buf, BytesMut};
use crossbeam::queue::ArrayQueue;
use parking_lot::Mutex;
use std::{
    cell::UnsafeCell,
    ops::DerefMut,
    task::{Context, Poll, Waker},
};
use tracing::debug;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;

pub(in crate::io) struct Drive<H, D>
where
    D: Decode,
{
    reader: ReadDriver<H, D>,
    writer: WriteDriver<H>,
}

impl<H, D> Drive<H, D>
where
    D: Decode,
{
    pub(in crate::io) fn new(reader: ReadDriver<H, D>, writer: WriteDriver<H>) -> Drive<H, D> {
        Drive { reader, writer }
    }

    pub(in crate::io) fn reader(&self) -> &ReadDriver<H, D> {
        &self.reader
    }

    pub(in crate::io) fn writer(&self) -> &WriteDriver<H> {
        &self.writer
    }
}

pub(in crate::io) struct WriteDriver<W> {
    /// Underlying socket/handle/device etc
    writer: UnsafeCell<W>,
    /// Overlapped state (owned by the kernel)
    overlapped: Overlapped,
    /// Current state of our I/O resource
    state: Mutex<WriteState>,
}

impl<W> WriteDriver<W>
where
    W: WriteOverlapped,
{
    /// Create a new WriteDriver instance
    pub fn new(writer: W) -> Self {
        WriteDriver {
            writer: UnsafeCell::new(writer),
            overlapped: Overlapped::new_write(0),
            state: Mutex::new(WriteState::Inert),
        }
    }

    pub(in crate::io) fn completion<P: ThreadpoolIoWork>(
        &self,
        io_result: u32,
        bytes_transferred: usize,
        pool: &P,
    ) {
        let mut state = self.state.lock();
        match std::mem::replace(state.deref_mut(), WriteState::Inert) {
            WriteState::Inert => panic!("WriteState::Inert cannot call write_complete"),
            WriteState::Complete(_, _) => panic!("WriteState::Complete cannot call write_complete"),
            WriteState::Writing(waker, mut bytes) | WriteState::Flushing(waker, mut bytes) => {
                match io_result {
                    ERROR_SUCCESS => {
                        bytes.advance(bytes_transferred);
                        if bytes.len() > 0 {
                            // Safety: Only 1 completions are possible at a time
                            unsafe { self.flush(pool) };
                        } else {
                            state.wake_with_result(waker, Ok(()))
                        }
                    }
                    code => {
                        state.wake_with_result(waker, Err(OverlappedError::Os(code as _).into()))
                    }
                }
            }
        }
    }

    /// Write bytes to I/O resource until pending, or until no more bytes can be sent
    ///
    /// Safety: Caller must have exclusive access to drive. Exclusive access can only be guarenteed
    /// if only 1 write at a time.
    pub(in crate::io) unsafe fn flush<P>(&self, pool: &P)
    where
        P: ThreadpoolIoWork,
    {
        let mut state = self.state.lock();
        match std::mem::replace(state.deref_mut(), WriteState::Inert) {
            WriteState::Inert => panic!("WriteState::Inert cannot call flush"),
            WriteState::Complete(_, _) => panic!("WriteState::Complete cannot call flush"),
            WriteState::Flushing(waker, mut bytes) | WriteState::Writing(waker, mut bytes) => {
                let writer = &mut *self.writer.get();
                let overlapped = &mut *(self.overlapped.get());
                loop {
                    pool.start();
                    let result = writer.write_overlapped(overlapped, &mut bytes);
                    match result {
                        Err(OverlappedError::Pending) => {
                            *state = WriteState::Flushing(waker, bytes);
                            break;
                        }
                        Err(e) => {
                            pool.cancel();
                            *state = WriteState::Complete(waker, Err(e.into()));
                            break;
                        }
                        Ok(n) => {
                            pool.cancel();
                            bytes.advance(n);
                            if bytes.len() == 0 {
                                *state = WriteState::Complete(waker, Ok(()));
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    pub(in crate::io) fn push_encodable<Item>(
        &self,
        item: &Item,
    ) -> Result<(), SinkEncodeError<Item::Error>>
    where
        Item: SinkEncode + SinkEncodeLen,
    {
        let mut state = self.state.lock();
        match state.deref_mut() {
            WriteState::Inert => panic!("WriteState::Inert cannot call push_encodable"),
            WriteState::Flushing(_, _) => panic!("WriteState::Flushing cannot call push_encodable"),
            WriteState::Complete(_, _) => panic!("WriteState::Complete cannot call push_encodable"),
            WriteState::Writing(_, ref mut bytes) => {
                if item.sink_encode_len() < bytes.capacity() - bytes.len() {
                    item.sink_encode(bytes).map_err(SinkEncodeError::Encode)
                } else {
                    Err(SinkEncodeError::BufferFull)
                }
            }
        }
    }
}

impl<W> WriteDriver<W> {
    pub(in crate::io) fn buffer(&self, bytes: BytesMut) {
        *self.state.lock() = WriteState::Writing(None, bytes);
    }

    pub(in crate::io) fn poll(&self, cx: &mut Context<'_>) -> Poll<Result<(), SinkError>> {
        let new_waker = cx.waker();
        let mut state = self.state.lock();
        match std::mem::replace(state.deref_mut(), WriteState::Inert) {
            WriteState::Flushing(Some(old_waker), bytes) if old_waker.will_wake(new_waker) => {
                *state = WriteState::Flushing(Some(old_waker), bytes);
                Poll::Pending
            }
            WriteState::Flushing(None, bytes) | WriteState::Flushing(Some(_), bytes) => {
                *state = WriteState::Flushing(Some(new_waker.clone()), bytes);
                Poll::Pending
            }
            WriteState::Complete(_, result) => Poll::Ready(result),
            _ => unreachable!(),
        }
    }
}

enum WriteState {
    Inert,
    Writing(Option<Waker>, BytesMut),
    Flushing(Option<Waker>, BytesMut),
    Complete(Option<Waker>, Result<(), SinkError>),
}
impl WriteState {
    fn wake_with_result(&mut self, waker: Option<Waker>, result: Result<(), SinkError>) {
        *self = WriteState::Complete(waker, result);
        if let WriteState::Complete(Some(waker), _) = self {
            waker.wake_by_ref()
        }
    }
}

/// This is the data that is passed to the kernel callbacks as a raw pointer.
pub(in crate::io) struct ReadDriver<R, D>
where
    D: Decode,
{
    reader: UnsafeCell<Reader<R, D>>,
    /// Shared state between the stream and the kernel. See: [`self::WakeBytes`]
    /// Threadpool callbacks will wake the future/stream when data is ready
    waker: Mutex<Option<Waker>>,
    /// An array of completions. We must buffer completions because we are a stream and multipe
    /// completions may occur faster than the stream can read them.
    queue: ArrayQueue<Result<D::Item, StreamError<D::Error>>>,
}

impl<R, D> ReadDriver<R, D>
where
    R: ReadOverlapped,
    D: Decode,
{
    /// Create a new instance of shared state between stream and threadpool.
    pub fn new(handle: R, decoder: D, capacity: usize, queue: usize) -> Self {
        Self {
            waker: Mutex::new(None),
            queue: ArrayQueue::new(queue),
            reader: UnsafeCell::new(Reader {
                handle,
                decoder,
                buf: BytesMut::with_capacity(capacity),
                overlapped: Overlapped::new_read(0),
            }),
        }
    }

    /// TODO We call this in a loop and might overflow queue and panic
    fn wake_with_result(&self, result: Result<D::Item, StreamError<D::Error>>) {
        debug_assert!(self.queue.capacity() > 1);
        self.queue.force_push(result);
        if self.queue.capacity() == 1 {
            self.queue.force_push(Err(StreamError::QueueFull));
        }
        if let Some(waker) = self.waker.lock().as_ref() {
            waker.wake_by_ref()
        }
    }

    /// Process a completion event
    ///
    /// Safety: caller must have exclusive access to the Drive. This can only be guarenteed
    /// when only one kernel callback is allowed to run at a time and the routine is only called
    /// from the kernel callback.
    ///
    /// Safety: the byte buffer between 0..bytes_transferred must have been initialized "some how"
    /// IE by the kernel via ReadOverlapped
    pub(in crate::io) unsafe fn completion<W: ThreadpoolIoWork>(
        &self,
        io_result: u32,
        bytes_transferred: usize,
        pool: &W,
    ) {
        let reader = &mut *self.reader.get();
        match io_result {
            ERROR_SUCCESS => {
                // Update buffer with newly initialized bytes
                reader.completion(bytes_transferred);

                // Decode the new bytes and populate queue with decoded items
                while let Poll::Ready(result) = reader.decode() {
                    self.wake_with_result(result);
                }

                // Start next read
                self.start(pool);
            }
            code => self.wake_with_result(Err(OverlappedError::Os(code as _).into())),
        }
    }

    /// Start a read
    ///
    /// Safety: caller must have exclusive access to the Drive. This can only be guarenteed
    /// when only one kernel callback is allowed to run at a time, it must be the first call to
    /// start, or subsequent calls to start must wait for all kernel callbacks to complete.
    pub(in crate::io) unsafe fn start<P>(&self, pool: &P)
    where
        P: ThreadpoolIoWork,
    {
        let capacity = self.queue.capacity();
        debug!(capacity, "driving read");
        let reader = &mut *self.reader.get();
        match reader.start(pool) {
            Err(StreamError::Overlapped(OverlappedError::Pending)) => debug!("read pending"),
            Err(e) => self.wake_with_result(Err(e)),
            Ok(count) => {
                // Update buffer (again) with syncronously parsed bytes
                reader.completion(count);
                // Decode the new bytes (again) with syncronously received bytes and
                // populate queue with decoded items
                while let Poll::Ready(result) = reader.decode() {
                    self.wake_with_result(result);
                }
            }
        }
    }
}

impl<R, D> ReadDriver<R, D>
where
    D: Decode,
{
    pub(in crate::io) fn poll_next(
        &self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<D::Item, StreamError<D::Error>>>> {
        let new_waker = cx.waker();
        let mut waker = self.waker.lock();
        match waker.take() {
            None => *waker = Some(new_waker.clone()),
            Some(old_waker) => {
                if old_waker.will_wake(new_waker) {
                    *waker = Some(old_waker);
                } else {
                    *waker = Some(new_waker.clone());
                }
            }
        }
        match self.queue.pop() {
            Some(Err(StreamError::Overlapped(OverlappedError::Eof))) => Poll::Ready(None),
            Some(Err(e)) => Poll::Ready(Some(Err(e))),
            Some(Ok(item)) => Poll::Ready(Some(Ok(item))),
            None => Poll::Pending,
        }
    }
}

struct Reader<R, D> {
    /// Underlying I/O resource
    handle: R,
    /// application callback per completion to delimit incoming bytes
    decoder: D,
    /// The threadpool will share the buffer with the future/stream
    buf: BytesMut,
    /// The overlapped state (index position, etc)
    overlapped: Overlapped,
}

impl<R, D> Reader<R, D>
where
    R: ReadOverlapped,
    D: Decode,
{
    /// Safety: See [`ReadDriver::start`]
    unsafe fn start<P>(&mut self, pool: &P) -> Result<usize, StreamError<D::Error>>
    where
        P: ThreadpoolIoWork,
    {
        let overlapped = &mut self.overlapped;
        self.handle
            .start_read_stream(overlapped, &mut self.buf, pool)
            .map_err(StreamError::from)
    }

    /// Update buffer with initialized bytes
    ///
    /// Safety: Bytes 0..bytes_transferred must be initialized from the kernel
    unsafe fn completion(&mut self, bytes_transferred: usize) {
        self.buf.set_len(bytes_transferred)
    }

    /// Decode some bytes
    fn decode(&mut self) -> Poll<Result<D::Item, StreamError<D::Error>>> {
        match self.decoder.decode(&mut self.buf) {
            Ok(Some(item)) => Poll::Ready(Ok(item)),
            Ok(None) => Poll::Pending,
            Err(e) => Poll::Ready(Err(StreamError::Decode(e))),
        }
    }
}
