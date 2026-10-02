//! Generated code for `greeter.tpt` plus the example service implementation.

use futures::StreamExt;
use tpt20_rpc::{async_trait, BoxStream, RpcContext, RpcError};

/// Code generated from `greeter.tpt`.
#[allow(unused)]
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/greeter.rs"));
}

use generated::{Greeter, HelloReply, HelloRequest};

/// The schema source, for the text-format demo.
pub const SCHEMA: &str = include_str!("../greeter.tpt");

/// A trivial `Greeter` implementation.
pub struct GreeterImpl;

fn greeting(name: &str, language: &str) -> String {
    match language {
        "fr" => format!("Bonjour, {name}!"),
        "es" => format!("¡Hola, {name}!"),
        _ => format!("Hello, {name}!"),
    }
}

#[async_trait]
impl Greeter for GreeterImpl {
    async fn say_hello(
        &self,
        ctx: &RpcContext,
        request: HelloRequest,
    ) -> Result<HelloReply, RpcError> {
        if request.name.is_empty() {
            return Err(RpcError::invalid_argument("name must not be empty").finish());
        }
        // Metadata sent by the client is visible to the handler.
        let lang = ctx.metadata().get_first_text("x-lang").unwrap_or("en");
        Ok(HelloReply {
            greeting: greeting(&request.name, lang),
            lucky_number: Some(request.name.len() as i32),
            ..Default::default()
        })
    }

    async fn say_hello_in_each(
        &self,
        _ctx: &RpcContext,
        request: HelloRequest,
    ) -> Result<BoxStream<'static, Result<HelloReply, RpcError>>, RpcError> {
        let name = request.name;
        Ok(futures::stream::iter(request.languages)
            .map(move |l| {
                Ok(HelloReply {
                    greeting: greeting(&name, &l),
                    ..Default::default()
                })
            })
            .boxed())
    }
}
