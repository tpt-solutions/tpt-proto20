//! Native schema reflection over RPC (spec §27.5): the
//! `tpt20.reflection.v1.Reflection` service serves the schema descriptors a
//! server was built with, so tools can discover and decode its messages
//! without the `.tpt` files (`tpt20 reflect-remote`).
//!
//! - `ListServices({})` → `{1: repeated service name}`
//! - `GetDescriptor({1: package string})` → `{1: descriptor bytes, 2: fingerprint}`;
//!   an empty package selects the only registered one. The descriptor is the
//!   binary `TPD1` form (`tpt20_descriptor::Descriptor::from_binary`).
//!
//! Generated code exposes `DESCRIPTOR` and `SERVICE_NAMES` constants to feed
//! [`ReflectionService::register`].

use crate::client::Channel;
use crate::context::RpcContext;
use crate::error::RpcError;
use crate::server::{ServerCall, Service};
use async_trait::async_trait;
use std::sync::Arc;
use tpt20_core::{
    DecodeError, DecoderLimits, Field, RawMessage, UnknownFieldPolicy, Value, WireClass,
};

/// Fully qualified name of the reflection service.
pub const SERVICE_NAME: &str = "tpt20.reflection.v1.Reflection";

#[derive(Debug, Clone)]
struct Entry {
    package: String,
    descriptor: Vec<u8>,
    fingerprint: String,
    services: Vec<String>,
}

/// The routable reflection service.
#[derive(Debug, Clone, Default)]
pub struct ReflectionService {
    entries: Arc<Vec<Entry>>,
}

impl ReflectionService {
    /// Creates an empty service.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a package: its binary descriptor, fingerprint and the fully
    /// qualified names of the services it declares.
    pub fn register(
        mut self,
        package: impl Into<String>,
        descriptor: &[u8],
        fingerprint: impl Into<String>,
        services: &[&str],
    ) -> Self {
        let entries = Arc::make_mut(&mut self.entries);
        entries.push(Entry {
            package: package.into(),
            descriptor: descriptor.to_vec(),
            fingerprint: fingerprint.into(),
            services: services.iter().map(|s| s.to_string()).collect(),
        });
        self
    }

    fn find(&self, package: &str) -> Result<&Entry, RpcError> {
        if package.is_empty() {
            return match self.entries.as_slice() {
                [only] => Ok(only),
                [] => Err(RpcError::not_found("no descriptors registered").finish()),
                _ => Err(RpcError::invalid_argument(
                    "several packages registered; name one in `package`",
                )
                .finish()),
            };
        }
        self.entries
            .iter()
            .find(|e| e.package == package)
            .ok_or_else(|| RpcError::not_found(format!("unknown package `{package}`")).finish())
    }
}

fn decode_raw(bytes: &[u8]) -> Result<RawMessage, DecodeError> {
    RawMessage::decode(
        bytes,
        &DecoderLimits::default(),
        UnknownFieldPolicy::Preserve,
    )
}

fn identity(bytes: &[u8]) -> Result<Vec<u8>, DecodeError> {
    Ok(bytes.to_vec())
}

// `Vec` (not a slice): the response type the driver infers is `Vec<u8>`.
#[allow(clippy::ptr_arg)]
fn encode_raw(v: &Vec<u8>) -> Vec<u8> {
    v.clone()
}

#[async_trait]
impl Service for ReflectionService {
    fn name(&self) -> &'static str {
        SERVICE_NAME
    }

    async fn handle(&self, method: &str, call: ServerCall) {
        let this = self.clone();
        match method {
            "ListServices" => {
                call.unary(
                    identity,
                    encode_raw,
                    move |_ctx, _req: Vec<u8>| async move {
                        let mut out = RawMessage::new();
                        for name in this.entries.iter().flat_map(|e| &e.services) {
                            out.push(Field::new(
                                1,
                                WireClass::Len,
                                Value::Len(name.as_bytes().to_vec()),
                            ));
                        }
                        out.encode()
                            .map_err(|e| RpcError::internal(e.to_string()).finish())
                    },
                )
                .await
            }
            "GetDescriptor" => {
                call.unary(identity, encode_raw, move |_ctx, req: Vec<u8>| async move {
                    let raw = decode_raw(&req).map_err(|e| {
                        RpcError::invalid_argument(format!("bad request: {e}")).finish()
                    })?;
                    let package = match raw.fields.iter().rev().find(|f| f.field_id == 1) {
                        Some(Field {
                            value: Value::Len(b),
                            ..
                        }) => String::from_utf8_lossy(b).into_owned(),
                        _ => String::new(),
                    };
                    let entry = this.find(&package)?;
                    let mut out = RawMessage::new();
                    out.push(Field::new(
                        1,
                        WireClass::Len,
                        Value::Len(entry.descriptor.clone()),
                    ));
                    out.push(Field::new(
                        2,
                        WireClass::Len,
                        Value::Len(entry.fingerprint.clone().into_bytes()),
                    ));
                    out.encode()
                        .map_err(|e| RpcError::internal(e.to_string()).finish())
                })
                .await
            }
            other => {
                call.finish(Err(RpcError::unimplemented(format!(
                    "unknown method `{other}`"
                ))
                .finish()))
                    .await
            }
        }
    }
}

/// A descriptor fetched from a server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteDescriptor {
    /// Binary `TPD1` descriptor bytes.
    pub descriptor: Vec<u8>,
    /// The schema fingerprint the server reports.
    pub fingerprint: String,
}

/// Lists the services the server describes.
pub async fn list_services(channel: &Channel, ctx: &RpcContext) -> Result<Vec<String>, RpcError> {
    channel
        .unary(
            &format!("{SERVICE_NAME}/ListServices"),
            ctx,
            Vec::new(),
            |bytes: &[u8]| {
                let raw = decode_raw(bytes)?;
                Ok(raw
                    .fields
                    .iter()
                    .filter(|f| f.field_id == 1)
                    .filter_map(|f| match &f.value {
                        Value::Len(b) => Some(String::from_utf8_lossy(b).into_owned()),
                        _ => None,
                    })
                    .collect())
            },
        )
        .await
}

/// Fetches the descriptor of `package` (empty = the only registered one).
pub async fn get_descriptor(
    channel: &Channel,
    ctx: &RpcContext,
    package: &str,
) -> Result<RemoteDescriptor, RpcError> {
    let mut request = RawMessage::new();
    if !package.is_empty() {
        request.push(Field::new(
            1,
            WireClass::Len,
            Value::Len(package.as_bytes().to_vec()),
        ));
    }
    let request = request.encode().expect("len field encodes");
    channel
        .unary(
            &format!("{SERVICE_NAME}/GetDescriptor"),
            ctx,
            request,
            |bytes: &[u8]| {
                let raw = decode_raw(bytes)?;
                let field = |id| {
                    raw.fields
                        .iter()
                        .rev()
                        .find_map(|f| match (&f.value, f.field_id == id) {
                            (Value::Len(b), true) => Some(b.clone()),
                            _ => None,
                        })
                };
                Ok(RemoteDescriptor {
                    descriptor: field(1).ok_or(DecodeError::MalformedScalar)?,
                    fingerprint: String::from_utf8_lossy(&field(2).unwrap_or_default())
                        .into_owned(),
                })
            },
        )
        .await
}
