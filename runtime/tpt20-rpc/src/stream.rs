//! Streaming types for RPC (spec §16.2).

use std::pin::Pin;
use std::task::{Context, Poll};
use crate::error::{ReceiveError, SendError};

pub trait TryStream {
    type Item;
    type Error;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Self::Item, Self::Error>>>;
}

pub trait TrySink {
    type Item;
    type Error;
    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;
    fn start_send(self: Pin<&mut Self>, item: Self::Item) -> Result<(), Self::Error>;
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;
}

pub type ServerStreamSink<T> = Box<dyn TrySink<Item = T, Error = SendError> + Send + Sync>;
pub type ClientStreamSource<T> = Box<dyn TryStream<Item = T, Error = ReceiveError> + Send + Sync>;

pub struct BidiStream<T> {
    pub sink: ServerStreamSink<T>,
    pub source: ClientStreamSource<T>,
}

impl<T> BidiStream<T> {
    pub fn new(sink: ServerStreamSink<T>, source: ClientStreamSource<T>) -> Self {
        Self { sink, source }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn traits_are_object_safe() {
        fn assert_sink<T: TrySink<Item = i32, Error = SendError> + Send + Sync>(_: &T) {}
        fn assert_stream<T: TryStream<Item = i32, Error = ReceiveError> + Send + Sync>(_: &T) {}
    }
}
