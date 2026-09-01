use tpt20_rpc::stream::{TryStream, TrySink, BidiStream};

struct DummyStream;
impl TryStream for DummyStream {
    type Item = Vec<u8>;
    type Error = tpt20_rpc::ReceiveError;
    fn poll_next(self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Result<Self::Item, Self::Error>>> {
        std::task::Poll::Ready(None)
    }
}

struct DummySink;
impl TrySink for DummySink {
    type Item = Vec<u8>;
    type Error = tpt20_rpc::SendError;
    fn poll_ready(self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn start_send(self: std::pin::Pin<&mut Self>, _item: Self::Item) -> Result<(), Self::Error> {
        Ok(())
    }
    fn poll_flush(self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}

#[test]
fn stream_and_sink_are_object_safe() {
    fn assert_sink<T: TrySink<Item = Vec<u8>, Error = tpt20_rpc::SendError> + Send + Sync>(_: &T) {}
    fn assert_stream<T: TryStream<Item = Vec<u8>, Error = tpt20_rpc::ReceiveError> + Send + Sync>(_: &T) {}
    assert_sink(&DummySink);
    assert_stream(&DummyStream);
}

#[test]
fn bidi_stream_constructs() {
    let bidi = BidiStream::new(
        Box::new(DummySink),
        Box::new(DummyStream),
    );
    let _ = bidi;
}

#[test]
fn server_stream_sink_type_alias_compiles() {
    let _: tpt20_rpc::ServerStreamSink<Vec<u8>> = Box::new(DummySink);
}

#[test]
fn client_stream_source_type_alias_compiles() {
    let _: tpt20_rpc::ClientStreamSource<Vec<u8>> = Box::new(DummyStream);
}
