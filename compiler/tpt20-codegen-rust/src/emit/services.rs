//! Service code generation (spec §12.6): server trait, server wrapper and
//! client stub per `service`.

use super::Emitter;
use crate::naming;
use std::fmt::Write;
use tpt20_ir as ir;

/// The four RPC shapes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Unary,
    ServerStreaming,
    ClientStreaming,
    Bidi,
}

impl Shape {
    fn of(m: &ir::MethodIr) -> Shape {
        match (m.request_streaming, m.response_streaming) {
            (false, false) => Shape::Unary,
            (false, true) => Shape::ServerStreaming,
            (true, false) => Shape::ClientStreaming,
            (true, true) => Shape::Bidi,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Shape::Unary => "unary",
            Shape::ServerStreaming => "server streaming",
            Shape::ClientStreaming => "client streaming",
            Shape::Bidi => "bidirectional streaming",
        }
    }
}

impl<'a> Emitter<'a> {
    pub(super) fn emit_services(&mut self) {
        let rpc = self.opts.rpc_crate.clone();
        let _ = write!(
            self.out,
            "\n// ---------------------------------------------------------------------------\n// Services (require the `{rpc}` crate)\n// ---------------------------------------------------------------------------\nuse {rpc} as __rpc;\n"
        );
        let services = self.pkg.services.clone();
        for svc in &services {
            self.emit_service(svc);
        }
        self.emit_descriptor_consts();
    }

    /// Embeds the package descriptor so servers can expose it through the
    /// reflection service.
    fn emit_descriptor_consts(&mut self) {
        let mut descriptor = tpt20_descriptor::Descriptor::new(self.pkg.clone());
        let fingerprint = self
            .pkg
            .fingerprint
            .clone()
            .unwrap_or_else(|| descriptor.compute_fingerprint());
        let Ok(bytes) = descriptor.to_binary() else {
            return;
        };
        let package = self.pkg.name.clone().unwrap_or_default();
        let names: Vec<String> = self
            .pkg
            .services
            .iter()
            .map(|s| {
                if package.is_empty() {
                    s.name.clone()
                } else {
                    format!("{package}.{}", s.name)
                }
            })
            .collect();
        let mut o = String::new();
        let _ = write!(
            o,
            "\n/// Package name of this schema.\npub const PACKAGE: &str = {package:?};\n\n/// Schema fingerprint.\npub const FINGERPRINT: &str = {fingerprint:?};\n\n/// Fully qualified names of the services declared by this schema.\npub const SERVICE_NAMES: &[&str] = &[{}];\n\n/// Binary (`TPD1`) descriptor of this schema, for `__rpc::reflection::ReflectionService::register`.\npub const DESCRIPTOR: &[u8] = &[\n",
            names.iter().map(|n| format!("{n:?}")).collect::<Vec<_>>().join(", ")
        );
        for chunk in bytes.chunks(24) {
            o.push_str("    ");
            for b in chunk {
                let _ = write!(o, "{b}, ");
            }
            o.push('\n');
        }
        o.push_str("];\n");
        self.out.push_str(&o);
    }

    fn emit_service(&mut self, svc: &ir::ServiceIr) {
        let pkg = self.pkg.name.clone().unwrap_or_default();
        let full = if pkg.is_empty() {
            svc.name.clone()
        } else {
            format!("{pkg}.{}", svc.name)
        };
        let name = naming::pascal(&svc.name);

        struct M {
            path: String,
            ident: String,
            shape: Shape,
            req: String,
            resp: String,
            wire: String,
        }
        let methods: Vec<M> = svc
            .methods
            .iter()
            .map(|m| M {
                path: format!("{full}/{}", m.name),
                ident: naming::snake(&m.name),
                shape: Shape::of(m),
                req: self.owned_type(&[], &m.request.path),
                resp: self.owned_type(&[], &m.response.path),
                wire: m.name.clone(),
            })
            .collect();

        let stream = |t: &str| format!("__rpc::BoxStream<'static, Result<{t}, __rpc::RpcError>>");

        // ---- server trait --------------------------------------------------
        let mut o = String::new();
        let _ = write!(
            o,
            "\n/// Server trait for `{full}`.\n///\n/// Implement it and serve it with `{name}Server`:\n/// `__rpc::Server::new().add_service({name}Server::new(my_impl))`.\n#[__rpc::async_trait]\npub trait {name}: Send + Sync + 'static {{\n"
        );
        for m in &methods {
            let sig = match m.shape {
                Shape::Unary => format!(
                    "async fn {}(&self, ctx: &__rpc::RpcContext, request: {}) -> Result<{}, __rpc::RpcError>;",
                    m.ident, m.req, m.resp
                ),
                Shape::ServerStreaming => format!(
                    "async fn {}(&self, ctx: &__rpc::RpcContext, request: {}) -> Result<{}, __rpc::RpcError>;",
                    m.ident,
                    m.req,
                    stream(&m.resp)
                ),
                Shape::ClientStreaming => format!(
                    "async fn {}(&self, ctx: &__rpc::RpcContext, requests: {}) -> Result<{}, __rpc::RpcError>;",
                    m.ident,
                    stream(&m.req),
                    m.resp
                ),
                Shape::Bidi => format!(
                    "async fn {}(&self, ctx: &__rpc::RpcContext, requests: {}) -> Result<{}, __rpc::RpcError>;",
                    m.ident,
                    stream(&m.req),
                    stream(&m.resp)
                ),
            };
            let _ = write!(
                o,
                "    /// `{}` ({}).\n    {sig}\n",
                m.wire,
                m.shape.label()
            );
        }
        o.push_str("}\n");

        // ---- server wrapper ------------------------------------------------
        let _ = write!(
            o,
            "\n/// Adapts a `{name}` implementation to a routable `__rpc::Service`.\npub struct {name}Server<S> {{\n    inner: std::sync::Arc<S>,\n}}\n\nimpl<S: {name}> {name}Server<S> {{\n    /// Fully qualified service name used in method paths.\n    pub const NAME: &'static str = \"{full}\";\n\n    /// Wraps a service implementation.\n    pub fn new(inner: S) -> Self {{\n        Self {{ inner: std::sync::Arc::new(inner) }}\n    }}\n\n    /// Wraps a shared service implementation.\n    pub fn from_arc(inner: std::sync::Arc<S>) -> Self {{\n        Self {{ inner }}\n    }}\n}}\n\n#[__rpc::async_trait]\nimpl<S: {name}> __rpc::Service for {name}Server<S> {{\n    fn name(&self) -> &'static str {{\n        Self::NAME\n    }}\n\n    async fn handle(&self, method: &str, call: __rpc::ServerCall) {{\n        match method {{\n"
        );
        for m in &methods {
            let (driver, arg, inv) = match m.shape {
                Shape::Unary => ("unary", "req", "req"),
                Shape::ServerStreaming => ("server_streaming", "req", "req"),
                Shape::ClientStreaming => ("client_streaming", "reqs", "reqs"),
                Shape::Bidi => ("bidi", "reqs", "reqs"),
            };
            let _ = write!(
                o,
                "            \"{wire}\" => {{\n                let inner = self.inner.clone();\n                call.{driver}(\n                    {req}::decode,\n                    {resp}::encode,\n                    move |ctx, {arg}| async move {{ inner.{ident}(&ctx, {inv}).await }},\n                )\n                .await\n            }}\n",
                wire = m.wire,
                req = m.req,
                resp = m.resp,
                ident = m.ident,
            );
        }
        o.push_str(
            "            other => {\n                call.finish(Err(__rpc::RpcError::unimplemented(format!(\"unknown method `{other}`\")).finish()))\n                    .await\n            }\n        }\n    }\n}\n",
        );

        // ---- client stub ---------------------------------------------------
        let _ = write!(
            o,
            "\n/// Client stub for `{full}` over a `__rpc::Channel`.\n///\n/// Every call takes a `__rpc::RpcContext` carrying the call's metadata,\n/// deadline and cancellation token.\n#[derive(Clone, Debug)]\npub struct {name}Client {{\n    channel: __rpc::Channel,\n}}\n\nimpl {name}Client {{\n    /// Fully qualified service name used in method paths.\n    pub const NAME: &'static str = \"{full}\";\n\n    /// Creates a client over `channel`.\n    pub fn new(channel: __rpc::Channel) -> Self {{\n        Self {{ channel }}\n    }}\n"
        );
        for m in &methods {
            let body = match m.shape {
                Shape::Unary => format!(
                    "    /// `{wire}` ({label}).\n    pub async fn {ident}(&self, ctx: &__rpc::RpcContext, request: &{req}) -> Result<{resp}, __rpc::RpcError> {{\n        self.channel\n            .unary(\"{path}\", ctx, request.encode(), {resp}::decode)\n            .await\n    }}\n",
                    wire = m.wire, label = m.shape.label(), ident = m.ident, req = m.req, resp = m.resp, path = m.path
                ),
                Shape::ServerStreaming => format!(
                    "    /// `{wire}` ({label}).\n    pub async fn {ident}(&self, ctx: &__rpc::RpcContext, request: &{req}) -> Result<{rs}, __rpc::RpcError> {{\n        self.channel\n            .server_streaming(\"{path}\", ctx, request.encode(), {resp}::decode)\n            .await\n    }}\n",
                    wire = m.wire, label = m.shape.label(), ident = m.ident, req = m.req, resp = m.resp, path = m.path, rs = stream(&m.resp)
                ),
                Shape::ClientStreaming => format!(
                    "    /// `{wire}` ({label}).\n    pub async fn {ident}(\n        &self,\n        ctx: &__rpc::RpcContext,\n        requests: impl __rpc::futures::Stream<Item = {req}> + Send + 'static,\n    ) -> Result<{resp}, __rpc::RpcError> {{\n        use __rpc::futures::StreamExt;\n        self.channel\n            .client_streaming(\"{path}\", ctx, requests.map(|m| m.encode()).boxed(), {resp}::decode)\n            .await\n    }}\n",
                    wire = m.wire, label = m.shape.label(), ident = m.ident, req = m.req, resp = m.resp, path = m.path
                ),
                Shape::Bidi => format!(
                    "    /// `{wire}` ({label}).\n    pub async fn {ident}(\n        &self,\n        ctx: &__rpc::RpcContext,\n        requests: impl __rpc::futures::Stream<Item = {req}> + Send + 'static,\n    ) -> Result<{rs}, __rpc::RpcError> {{\n        use __rpc::futures::StreamExt;\n        self.channel\n            .bidi(\"{path}\", ctx, requests.map(|m| m.encode()).boxed(), {resp}::decode)\n            .await\n    }}\n",
                    wire = m.wire, label = m.shape.label(), ident = m.ident, req = m.req, resp = m.resp, path = m.path, rs = stream(&m.resp)
                ),
            };
            o.push_str(&body);
        }
        o.push_str("}\n");
        self.out.push_str(&o);
    }
}
