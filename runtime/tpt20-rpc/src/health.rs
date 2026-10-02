//! Native health checking (spec §27.5): the `tpt20.health.v1.Health` service.
//!
//! `Check` takes `{1: service string}` (empty = the whole server) and answers
//! `{1: status varint}` using the gRPC numbering (0 UNKNOWN, 1 SERVING,
//! 2 NOT_SERVING, 3 SERVICE_UNKNOWN), so `tpt20 health` and gRPC-style
//! clients agree on the meaning.
//!
//! ```no_run
//! use tpt20_rpc::health::{HealthService, ServingStatus};
//! let (health, reporter) = HealthService::new();
//! // register `health` on a `Server`, then report status changes:
//! reporter.set_status("user.v1.UserService", ServingStatus::Serving);
//! ```

use crate::client::Channel;
use crate::context::RpcContext;
use crate::error::RpcError;
use crate::server::{ServerCall, Service};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tpt20_core::{
    DecodeError, DecoderLimits, Field, RawMessage, UnknownFieldPolicy, Value, WireClass,
};

/// Fully qualified name of the health service.
pub const SERVICE_NAME: &str = "tpt20.health.v1.Health";

/// Serving status of a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServingStatus {
    /// Status not known.
    Unknown = 0,
    /// Serving requests.
    Serving = 1,
    /// Not serving requests.
    NotServing = 2,
    /// The named service is not registered with the health service.
    ServiceUnknown = 3,
}

impl ServingStatus {
    /// Canonical upper-case name.
    pub const fn as_str(self) -> &'static str {
        match self {
            ServingStatus::Unknown => "UNKNOWN",
            ServingStatus::Serving => "SERVING",
            ServingStatus::NotServing => "NOT_SERVING",
            ServingStatus::ServiceUnknown => "SERVICE_UNKNOWN",
        }
    }

    /// Maps a wire value; out-of-range values are `Unknown`.
    pub fn from_code(code: u64) -> ServingStatus {
        match code {
            1 => ServingStatus::Serving,
            2 => ServingStatus::NotServing,
            3 => ServingStatus::ServiceUnknown,
            _ => ServingStatus::Unknown,
        }
    }
}

/// Handle for updating the statuses a [`HealthService`] reports.
#[derive(Clone, Debug)]
pub struct HealthReporter {
    statuses: Arc<RwLock<HashMap<String, ServingStatus>>>,
}

impl HealthReporter {
    /// Sets the status of `service`; the empty name is the overall server.
    pub fn set_status(&self, service: impl Into<String>, status: ServingStatus) {
        self.statuses
            .write()
            .expect("health lock")
            .insert(service.into(), status);
    }

    /// Current status as `Check` would report it.
    pub fn status(&self, service: &str) -> ServingStatus {
        let map = self.statuses.read().expect("health lock");
        match map.get(service) {
            Some(s) => *s,
            // The overall server is serving unless said otherwise; an
            // unregistered named service is unknown.
            None if service.is_empty() => ServingStatus::Serving,
            None => ServingStatus::ServiceUnknown,
        }
    }
}

/// The routable health service.
pub struct HealthService {
    reporter: HealthReporter,
}

impl HealthService {
    /// Creates the service and the reporter used to drive it.
    pub fn new() -> (HealthService, HealthReporter) {
        let reporter = HealthReporter {
            statuses: Arc::default(),
        };
        (
            HealthService {
                reporter: reporter.clone(),
            },
            reporter,
        )
    }
}

fn decode_raw(bytes: &[u8]) -> Result<RawMessage, DecodeError> {
    RawMessage::decode(
        bytes,
        &DecoderLimits::default(),
        UnknownFieldPolicy::Preserve,
    )
}

fn decode_request(bytes: &[u8]) -> Result<String, DecodeError> {
    let raw = decode_raw(bytes)?;
    match raw.fields.iter().rev().find(|f| f.field_id == 1) {
        Some(Field {
            value: Value::Len(b),
            ..
        }) => String::from_utf8(b.clone()).map_err(|_| DecodeError::InvalidUtf8),
        Some(_) => Err(DecodeError::WireClassMismatch { field_id: 1 }),
        None => Ok(String::new()),
    }
}

fn encode_response(status: &ServingStatus) -> Vec<u8> {
    let mut raw = RawMessage::new();
    raw.push(Field::new(
        1,
        WireClass::Varint,
        Value::Varint(*status as u64),
    ));
    raw.encode().expect("varint field encodes")
}

#[async_trait]
impl Service for HealthService {
    fn name(&self) -> &'static str {
        SERVICE_NAME
    }

    async fn handle(&self, method: &str, call: ServerCall) {
        match method {
            "Check" => {
                let reporter = self.reporter.clone();
                call.unary(
                    decode_request,
                    encode_response,
                    move |_ctx, service: String| async move { Ok(reporter.status(&service)) },
                )
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

/// Calls `Health/Check` on `channel`.
pub async fn check(
    channel: &Channel,
    ctx: &RpcContext,
    service: &str,
) -> Result<ServingStatus, RpcError> {
    let mut request = RawMessage::new();
    if !service.is_empty() {
        request.push(Field::new(
            1,
            WireClass::Len,
            Value::Len(service.as_bytes().to_vec()),
        ));
    }
    let request = request.encode().expect("len field encodes");
    channel
        .unary(
            &format!("{SERVICE_NAME}/Check"),
            ctx,
            request,
            |bytes: &[u8]| {
                let raw = decode_raw(bytes)?;
                Ok(match raw.fields.iter().rev().find(|f| f.field_id == 1) {
                    Some(Field {
                        value: Value::Varint(v),
                        ..
                    }) => ServingStatus::from_code(*v),
                    _ => ServingStatus::Unknown,
                })
            },
        )
        .await
}
