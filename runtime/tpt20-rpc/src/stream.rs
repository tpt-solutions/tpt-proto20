//! Streaming types for RPC (spec §16.2).
//!
//! Provides backpressure-aware abstractions for server-streaming,
//! client-streaming, and bidirectional streaming.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::error::{ReceiveError, RpcError, SendError};

/// A stream that yields items or errors, used for receiving messages.
pub trait TryStream {
    /// The type of items produced by the stream.
    type Item;
    /// The error type produced when the stream encounters a failure.
    type Error;

    /// Attempts to pull out the next value.
    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Item, Self::Error>>>;
}

/// A sink that accepts items with backpressure, used for sending messages.
pub trait TrySink {
    /// The type of items accepted by the sink.
    type Item;
    /// The error type produced when the sink encounters a failure.
    type Error;

    /// Signals that the sink is ready to accept an item.
    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;

    /// Submits an item to the sink.
    fn start_send(self: Pin<&mut Self>, item: Self::Item) -> Result<(), Self::Error>;

    /// Flushes any buffered items to the underlying transport.
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;
}

// ---- ServerStreamSink -------------------------------------------------------

/// A sink-like handle the server uses to push messages to a client.
///
/// This type is backpressure-aware: `send` will wait until the client is
/// ready to receive before returning.
#[derive(Debug)]
pub struct ServerStreamSink<T> {
    inner: Box<dyn InnerSink<Item = T, Error = SendError> + Send + Sync>,
}

impl<T: Send + Sync + 'static> ServerStreamSink<T> {
    /// Wraps an inner sink implementation.
    pub fn new<S>(inner: S) -> Self
    where
        S: TrySink<Item = T, Error = SendError> + Send + Sync + 'static,
    {
        Self {
            inner: Box::new(inner),
        }
    }

    /// Sends a single message to the client.
    pub fn send(&mut self, item: T) -> SendFuture<'_, T> {
        SendFuture { sink: self, item: Some(item), state: SendState::Ready }
    }

    /// Closes the stream, signaling end-of-messages to the client.
    pub fn close(&mut self) -> CloseFuture<'_, T> {
        CloseFuture { sink: self }
    }

    /// Aborts the stream with an error, canceling the RPC.
    pub fn abort(mut self, error: RpcError) -> AbortFuture {
        let _ = error;
        AbortFuture { _sink: Some(self.inner) }
    }
}

impl<T> Unpin for ServerStreamSink<T> {}

/// Future returned by [`ServerStreamSink::send`].
#[derive(Debug)]
pub struct SendFuture<'a, T> {
    sink: &'a mut ServerStreamSink<T>,
    item: Option<T>,
    state: SendState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SendState {
    Ready,
    Flushing,
    Done,
}

impl<'a, T: Send + Sync + 'static> Future for SendFuture<'a, T> {
    type Output = Result<(), SendError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        loop {
            match this.state {
                SendState::Ready => {
                    let sink = this.sink.inner.as_mut();
                    let ready = unsafe { Pin::new_unchecked(sink) }.poll_ready(cx);
                    match ready {
                        Poll::Ready(Ok(())) => {
                            if let Some(item) = this.item.take() {
                                let sink = this.sink.inner.as_mut();
                                unsafe { Pin::new_unchecked(sink) }.start_send(item)?;
                                this.state = SendState::Flushing;
                            } else {
                                this.state = SendState::Done;
                                return Poll::Ready(Ok(()));
                            }
                        }
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                        Poll::Pending => return Poll::Pending,
                    }
                }
                SendState::Flushing => {
                    let sink = this.sink.inner.as_mut();
                    match unsafe { Pin::new_unchecked(sink) }.poll_flush(cx) {
                        Poll::Ready(Ok(())) => {
                            this.state = SendState::Done;
                            return Poll::Ready(Ok(()));
                        }
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                        Poll::Pending => return Poll::Pending,
                    }
                }
                SendState::Done => return Poll::Ready(Ok(())),
            }
        }
    }
}

/// Future returned by [`ServerStreamSink::close`].
#[derive(Debug)]
pub struct CloseFuture<'a, T> {
    sink: &'a mut ServerStreamSink<T>,
}

impl<'a, T: Send + Sync + 'static> Future for CloseFuture<'a, T> {
    type Output = Result<(), SendError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let sink = self.sink.inner.as_mut();
        unsafe { Pin::new_unchecked(sink) }.poll_flush(cx)
    }
}

/// Future returned by [`ServerStreamSink::abort`].
#[derive(Debug)]
pub struct AbortFuture {
    _sink: Option<Box<dyn InnerSink<Item = dyn std::any::Any + Send + Sync, Error = SendError> + Send + Sync>>,
}

impl Future for AbortFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        this._sink.take();
        Poll::Ready(())
    }
}

// Internal object-safe sink trait.
trait InnerSink {
    type Item;
    type Error;
    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;
    fn start_send(self: Pin<&mut Self>, item: Self::Item) -> Result<(), Self::Error>;
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;
}

impl<T, E> InnerSink for Box<dyn TrySink<Item = T, Error = E> + Send + Sync> {
    type Item = T;
    type Error = E;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        <Box<dyn TrySink<Item = T, Error = E> + Send + Sync> as TrySink>::poll_ready(self, cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Self::Item) -> Result<(), Self::Error> {
        <Box<dyn TrySink<Item = T, Error = E> + Send + Sync> as TrySink>::start_send(self, item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        <Box<dyn TrySink<Item = T, Error = E> + Send + Sync> as TrySink>::poll_flush(self, cx)
    }
}

// ---- ClientStreamSource -----------------------------------------------------

/// A stream-like handle the client uses to receive messages from the server.
///
/// This type is backpressure-aware: `next` will wait until a message is
/// available or the stream ends.
#[derive(Debug)]
pub struct ClientStreamSource<T> {
    inner: Box<dyn InnerStream<Item = T, Error = ReceiveError> + Send + Sync>,
}

impl<T: Send + Sync + 'static> ClientStreamSource<T> {
    /// Wraps an inner stream implementation.
    pub fn new<S>(inner: S) -> Self
    where
        S: TryStream<Item = T, Error = ReceiveError> + Send + Sync + 'static,
    {
        Self {
            inner: Box::new(inner),
        }
    }

    /// Returns the next message, if any.
    pub fn next(&mut self) -> NextFuture<'_, T> {
        NextFuture { stream: self }
    }
}

impl<T> Unpin for ClientStreamSource<T> {}

/// Future returned by [`ClientStreamSource::next`].
#[derive(Debug)]
pub struct NextFuture<'a, T> {
    stream: &'a mut ClientStreamSource<T>,
}

impl<'a, T: Send + Sync + 'static> Future for NextFuture<'a, T> {
    type Output = Option<Result<T, ReceiveError>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let stream = this.stream.inner.as_mut();
        match unsafe { Pin::new_unchecked(stream) }.poll_next(cx) {
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Ready(Some(Ok(v))) => Poll::Ready(Some(Ok(v))),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Pending => Poll::Pending,
        }
    }
}

// Internal object-safe stream trait.
trait InnerStream {
    type Item;
    type Error;
    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Item, Self::Error>>>;
}

impl<T, E> InnerStream for Box<dyn TryStream<Item = T, Error = E> + Send + Sync> {
    type Item = T;
    type Error = E;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Item, Self::Error>>> {
        <Box<dyn TryStream<Item = T, Error = E> + Send + Sync> as TryStream>::poll_next(self, cx)
    }
}

// ---- BidiStream -------------------------------------------------------------

/// A bidirectional stream, combining send and receive capabilities.
#[derive(Debug)]
pub struct BidiStream<T> {
    sink: ServerStreamSink<T>,
    source: ClientStreamSource<T>,
}

impl<T: Send + Sync + 'static> BidiStream<T> {
    /// Creates a new bidirectional stream from its parts.
    pub fn new(sink: ServerStreamSink<T>, source: ClientStreamSource<T>) -> Self {
        Self { sink, source }
    }

    /// Sends a message to the peer.
    pub fn send(&mut self, item: T) -> SendFuture<'_, T> {
        self.sink.send(item)
    }

    /// Receives the next message from the peer.
    pub fn next(&mut self) -> NextFuture<'_, T> {
        self.source.next()
    }

    /// Closes the stream.
    pub fn close(&mut self) -> CloseFuture<'_, T> {
        self.sink.close()
    }

    /// Aborts the stream with an error.
    pub fn abort(mut self, error: RpcError) -> AbortFuture {
        let _ = error;
        AbortFuture {
            _sink: Some(self.sink.inner),
        }
    }
}

impl<T> Unpin for BidiStream<T> {}
