//! Go backend: plain structs with `Encode`/`Decode*` and the minimal
//! `tpt20_runtime.go` (generics, Go 1.21+).

use crate::model::{Card, Enum, Field, Model, Scalar, Ty};
use crate::naming::{avoid, package_ident, pascal};
use crate::{Backend, BackendError, BackendOptions, GeneratedFile};
use std::fmt::Write;
use tpt20_ir as ir;

const RUNTIME: &str = include_str!("runtime/tpt20_runtime.go");
const RPC_RUNTIME: &str = include_str!("runtime/tpt20_rpc.go");

/// Go code generator.
#[derive(Debug, Clone, Copy, Default)]
pub struct GoBackend;

impl Backend for GoBackend {
    fn name(&self) -> &'static str {
        "go"
    }

    fn generate(
        &self,
        package: &ir::PackageIr,
        options: &BackendOptions,
    ) -> Result<Vec<GeneratedFile>, BackendError> {
        let model = Model::build(package)?;
        let pkg = options
            .package_name
            .clone()
            .unwrap_or_else(|| package_ident(&model.package, "schema"));
        let mut g = Gen {
            model: &model,
            out: String::new(),
        };
        g.file(&pkg);
        let rpc = options.services && !model.services.is_empty();
        let mut files = vec![
            GeneratedFile {
                path: "go.mod".to_string(),
                // The RPC runtime uses the HTTP/2 cleartext API of Go 1.24.
                contents: format!("module {pkg}\n\ngo {}\n", if rpc { "1.24" } else { "1.21" }),
            },
            GeneratedFile {
                path: format!("{pkg}.go"),
                contents: g.out,
            },
            GeneratedFile {
                path: "tpt20_runtime.go".to_string(),
                contents: RUNTIME.replace("package PACKAGE", &format!("package {pkg}")),
            },
        ];
        if rpc {
            let mut s = Gen {
                model: &model,
                out: String::new(),
            };
            s.services(&pkg);
            files.push(GeneratedFile {
                path: format!("{pkg}_services.go"),
                contents: s.out,
            });
            files.push(GeneratedFile {
                path: "tpt20_rpc.go".to_string(),
                contents: RPC_RUNTIME.replace("package PACKAGE", &format!("package {pkg}")),
            });
        }
        Ok(files)
    }
}

struct Gen<'a> {
    model: &'a Model,
    out: String,
}

/// Exported Go name: first letter upper-cased, underscores kept (`Outer_Child`).
fn export(name: &str) -> String {
    let mut c = name.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

fn field_name(name: &str) -> String {
    avoid(
        &pascal(name),
        &["Encode", "Decode", "UnknownFields", "Reset", "String"],
    )
}

/// Message fields are already nilable, so explicit presence adds nothing.
fn norm(f: &Field) -> Card {
    match (f.card, f.ty) {
        (Card::Explicit, Ty::Message(_)) => Card::Implicit,
        (c, _) => c,
    }
}

fn go_scalar(s: Scalar) -> &'static str {
    match s {
        Scalar::Bool => "bool",
        Scalar::Int32 | Scalar::SInt32 | Scalar::SFixed32 => "int32",
        Scalar::Int64 | Scalar::SInt64 | Scalar::SFixed64 => "int64",
        Scalar::UInt32 | Scalar::Fixed32 => "uint32",
        Scalar::UInt64 | Scalar::Fixed64 => "uint64",
        Scalar::Float32 => "float32",
        Scalar::Float64 => "float64",
        Scalar::String => "string",
        Scalar::Bytes => "[]byte",
    }
}

fn class_const(s: Scalar) -> &'static str {
    use crate::model::Wire;
    match s.wire() {
        Wire::Varint => "wcVarint",
        Wire::Fixed32 => "wcFixed32",
        Wire::Fixed64 => "wcFixed64",
        Wire::Len => "wcLen",
    }
}

/// `toWInt32`, `fromWSint64`, … (packable scalars only).
fn word_fn(dir: &str, s: Scalar) -> String {
    format!("{dir}W{}", export(s.name()))
}

impl Gen<'_> {
    fn w(&mut self, s: &str) {
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn tname(&self, idx: usize) -> String {
        export(&self.model.messages[idx].flat)
    }

    fn ename(&self, idx: usize) -> String {
        export(&self.model.enums[idx].flat)
    }

    fn go_ty(&self, ty: Ty) -> String {
        match ty {
            Ty::Scalar(s) => go_scalar(s).to_string(),
            Ty::Enum(i) => self.ename(i),
            Ty::Message(i) => format!("*{}", self.tname(i)),
        }
    }

    fn valid_fn(&self, ty: Ty) -> String {
        match ty {
            Ty::Enum(i) if !self.model.enums[i].open => format!("{}_Valid", self.ename(i)),
            _ => "nil".into(),
        }
    }

    fn field_decl(&self, f: &Field) -> String {
        let t = self.go_ty(f.ty);
        match norm(f) {
            Card::Implicit => t,
            Card::Explicit => format!("*{t}"),
            Card::Repeated => format!("[]{t}"),
            Card::Map(k) => format!("map[{}]{t}", go_scalar(k)),
        }
    }

    fn file(&mut self, pkg: &str) {
        self.w("// Code generated by tpt20-codegen-backends (go). DO NOT EDIT.");
        let _ = writeln!(self.out, "// Schema package: {}", self.model.package);
        let _ = write!(self.out, "\npackage {pkg}\n\n");
        for e in &self.model.enums {
            self.enum_type(e);
        }
        for (i, _) in self.model.messages.iter().enumerate() {
            self.message(i);
        }
    }

    fn enum_type(&mut self, e: &Enum) {
        let name = export(&e.flat);
        let _ = write!(
            self.out,
            "\n// {name} is enum `{}` ({}).\ntype {name} int32\n\nconst (\n",
            e.full_name,
            if e.open { "open" } else { "closed" }
        );
        for v in &e.values {
            let _ = writeln!(self.out, "\t{name}_{} {name} = {}", v.name, v.number);
        }
        self.w(")");
        if !e.open {
            let cases: Vec<String> = e.values.iter().map(|v| v.number.to_string()).collect();
            let body = if cases.is_empty() {
                "false".to_string()
            } else {
                format!(
                    "switch n {{ case {}: return true }}; return false",
                    cases.join(", ")
                )
            };
            let _ = write!(
                self.out,
                "\n// {name}_Valid reports whether n is a declared value of the closed enum.\nfunc {name}_Valid(n int32) bool {{ {body} }}\n"
            );
            // `body` above is a statement list when non-empty; make it well formed.
        }
    }

    fn message(&mut self, idx: usize) {
        let m = &self.model.messages[idx];
        let name = export(&m.flat);
        // Oneof interfaces and variants.
        for o in &m.oneofs {
            let iface = format!("is{name}_{}", pascal(&o.name));
            let _ = write!(
                self.out,
                "\n// {iface} is implemented by the variants of oneof `{}.{}`.\ntype {iface} interface{{ {iface}() }}\n",
                m.full_name, o.name
            );
            for f in &o.fields {
                let v = format!("{name}_{}_{}", pascal(&o.name), pascal(&f.name));
                let _ = write!(
                    self.out,
                    "\n// {v} is the `{}` variant (field {}).\ntype {v} struct{{ Value {} }}\n\nfunc (*{v}) {iface}() {{}}\n",
                    f.name,
                    f.id,
                    self.go_ty(f.ty)
                );
            }
        }
        let _ = write!(
            self.out,
            "\n// {name} is message `{}`.\ntype {name} struct {{\n",
            m.full_name
        );
        for f in &m.fields {
            let _ = writeln!(self.out, "\t{} {}", field_name(&f.name), self.field_decl(f));
        }
        for o in &m.oneofs {
            let _ = writeln!(
                self.out,
                "\t{} is{name}_{}",
                field_name(&o.name),
                pascal(&o.name)
            );
        }
        self.w("\t// UnknownFields holds fields this schema does not know; they are re-encoded.");
        self.w("\tUnknownFields []Tpt20RawField");
        self.w("}");
        self.encode(idx);
        self.decode(idx);
    }

    /// Statement(s) writing `v` (a value of the field's Go type) as field `id`.
    fn put(&self, id: u32, ty: Ty, v: &str) -> String {
        match ty {
            Ty::Scalar(s) => match s {
                Scalar::String => format!("w.PutLen({id}, []byte({v}))"),
                Scalar::Bytes => format!("w.PutLen({id}, {v})"),
                _ => format!(
                    "w.PutWord({id}, {}, {}({v}))",
                    class_const(s),
                    word_fn("to", s)
                ),
            },
            Ty::Enum(_) => format!("w.PutEnum({id}, int32({v}))"),
            Ty::Message(_) => format!("w.PutLen({id}, {v}.Encode())"),
        }
    }

    fn encode(&mut self, idx: usize) {
        let m = &self.model.messages[idx];
        let name = export(&m.flat);
        let _ = write!(
            self.out,
            "\n// Encode serializes the message to the native wire format.\nfunc (m *{name}) Encode() []byte {{\n\tvar w Tpt20Writer\n"
        );
        for f in &m.fields {
            let a = format!("m.{}", field_name(&f.name));
            match norm(f) {
                Card::Implicit => {
                    let cond = match f.ty {
                        Ty::Scalar(Scalar::Bool) => a.clone(),
                        Ty::Scalar(Scalar::String | Scalar::Bytes) => format!("len({a}) > 0"),
                        Ty::Scalar(_) | Ty::Enum(_) => format!("{a} != 0"),
                        Ty::Message(_) => format!("{a} != nil"),
                    };
                    let _ = writeln!(
                        self.out,
                        "\tif {cond} {{\n\t\t{}\n\t}}",
                        self.put(f.id, f.ty, &a)
                    );
                }
                Card::Explicit => {
                    let _ = writeln!(
                        self.out,
                        "\tif {a} != nil {{\n\t\t{}\n\t}}",
                        self.put(f.id, f.ty, &format!("*{a}"))
                    );
                }
                Card::Repeated => match f.ty {
                    Ty::Scalar(s) if s.packable() => {
                        let _ = writeln!(
                            self.out,
                            "\tif len({a}) > 0 {{\n\t\trtPutPacked(&w, {}, {}, {a}, {})\n\t}}",
                            f.id,
                            class_const(s),
                            word_fn("to", s)
                        );
                    }
                    Ty::Enum(_) => {
                        let _ = writeln!(
                            self.out,
                            "\tif len({a}) > 0 {{\n\t\tTpt20PutEnumPacked(&w, {}, {a})\n\t}}",
                            f.id
                        );
                    }
                    _ => {
                        let _ = writeln!(
                            self.out,
                            "\tfor _, v := range {a} {{\n\t\t{}\n\t}}",
                            self.put(f.id, f.ty, "v")
                        );
                    }
                },
                Card::Map(k) => {
                    let keys = if k == Scalar::Bool {
                        format!("rtMapKeysBool({a})")
                    } else {
                        format!("rtMapKeys({a})")
                    };
                    let _ = writeln!(
                        self.out,
                        "\tfor _, k := range {keys} {{\n\t\tvar e Tpt20Writer\n\t\t{}\n\t\t{}\n\t\tw.PutLen({}, e.Bytes())\n\t}}",
                        self.put(1, Ty::Scalar(k), "k").replace("w.", "e."),
                        self.put(2, f.ty, &format!("{a}[k]")).replace("w.", "e."),
                        f.id
                    );
                }
            }
        }
        for o in &m.oneofs {
            let a = format!("m.{}", field_name(&o.name));
            let _ = writeln!(self.out, "\tswitch v := {a}.(type) {{");
            for f in &o.fields {
                let variant = format!("{name}_{}_{}", pascal(&o.name), pascal(&f.name));
                let _ = writeln!(
                    self.out,
                    "\tcase *{variant}:\n\t\t{}",
                    self.put(f.id, f.ty, "v.Value")
                );
            }
            self.w("\t}");
        }
        self.w("\tw.PutUnknown(m.UnknownFields)");
        self.w("\treturn w.Bytes()");
        self.w("}");
    }

    /// Statements decoding field occurrence `f` into local `v`; leaves `v` set.
    fn get(&self, ty: Ty, depth: &str) -> String {
        match ty {
            Ty::Scalar(s) => match s {
                Scalar::String => "v, err := Tpt20GetString(f, l)".into(),
                Scalar::Bytes => "v, err := Tpt20GetBytes(f, l)".into(),
                _ => format!(
                    "v, err := rtGetWord(f, {}, {})",
                    class_const(s),
                    word_fn("from", s)
                ),
            },
            Ty::Enum(i) => format!(
                "n, err := Tpt20GetEnum(f, {})\n\t\tv := {}(n)",
                self.valid_fn(ty),
                self.ename(i)
            ),
            Ty::Message(i) => format!(
                "b, err := Tpt20GetLen(f)\n\t\tif err != nil {{\n\t\t\treturn err\n\t\t}}\n\t\tv := &{}{{}}\n\t\terr = v.decode(b, l, {depth})",
                self.tname(i)
            ),
        }
    }

    fn decode(&mut self, idx: usize) {
        let m = &self.model.messages[idx];
        let name = export(&m.flat);
        let _ = write!(
            self.out,
            "\n// Decode{name} parses bytes with the default limits.\nfunc Decode{name}(data []byte) (*{name}, error) {{\n\treturn Decode{name}WithLimits(data, Tpt20DefaultLimits())\n}}\n\n// Decode{name}WithLimits parses bytes under explicit limits.\nfunc Decode{name}WithLimits(data []byte, l *Tpt20Limits) (*{name}, error) {{\n\tm := &{name}{{}}\n\tif err := m.decode(data, l, 1); err != nil {{\n\t\treturn nil, err\n\t}}\n\treturn m, nil\n}}\n"
        );
        let _ = write!(
            self.out,
            "\nfunc (m *{name}) decode(data []byte, l *Tpt20Limits, depth int) error {{\n\tif err := rtCheckDepth(depth, l); err != nil {{\n\t\treturn err\n\t}}\n\tfields, err := rtParseFields(data, l)\n\tif err != nil {{\n\t\treturn err\n\t}}\n\tunknown := 0\n\tfor _, f := range fields {{\n\t\tswitch f.ID {{\n"
        );
        for f in &m.fields {
            let a = format!("m.{}", field_name(&f.name));
            let _ = writeln!(self.out, "\t\tcase {}:", f.id);
            match norm(f) {
                Card::Implicit | Card::Explicit => {
                    let g = self.get(f.ty, "depth + 1");
                    let assign = if norm(f) == Card::Explicit {
                        format!("{a} = &v")
                    } else {
                        format!("{a} = v")
                    };
                    let _ = writeln!(
                        self.out,
                        "\t\t{g}\n\t\tif err != nil {{\n\t\t\treturn err\n\t\t}}\n\t\t{assign}"
                    );
                }
                Card::Repeated => match f.ty {
                    Ty::Scalar(s) if s.packable() => {
                        let _ = writeln!(
                            self.out,
                            "\t\tvs, err := rtGetPacked(f, {}, {}, l)\n\t\tif err != nil {{\n\t\t\treturn err\n\t\t}}\n\t\t{a} = append({a}, vs...)\n\t\tif err := rtCheckRepeated(len({a}), l); err != nil {{\n\t\t\treturn err\n\t\t}}",
                            class_const(s),
                            word_fn("from", s)
                        );
                    }
                    Ty::Enum(i) => {
                        let _ = writeln!(
                            self.out,
                            "\t\tvs, err := Tpt20GetEnumPacked(f, {}, l)\n\t\tif err != nil {{\n\t\t\treturn err\n\t\t}}\n\t\tfor _, n := range vs {{\n\t\t\t{a} = append({a}, {}(n))\n\t\t}}\n\t\tif err := rtCheckRepeated(len({a}), l); err != nil {{\n\t\t\treturn err\n\t\t}}",
                            self.valid_fn(f.ty),
                            self.ename(i)
                        );
                    }
                    _ => {
                        let g = self.get(f.ty, "depth + 1");
                        let _ = writeln!(
                            self.out,
                            "\t\t{g}\n\t\tif err != nil {{\n\t\t\treturn err\n\t\t}}\n\t\t{a} = append({a}, v)\n\t\tif err := rtCheckRepeated(len({a}), l); err != nil {{\n\t\t\treturn err\n\t\t}}"
                        );
                    }
                },
                Card::Map(k) => self.decode_map(&a, f, k),
            }
        }
        for o in &m.oneofs {
            for f in &o.fields {
                let variant = format!("{name}_{}_{}", pascal(&o.name), pascal(&f.name));
                let g = self.get(f.ty, "depth + 1");
                let _ = writeln!(
                    self.out,
                    "\t\tcase {}:\n\t\t{g}\n\t\tif err != nil {{\n\t\t\treturn err\n\t\t}}\n\t\tm.{} = &{variant}{{Value: v}}",
                    f.id,
                    field_name(&o.name)
                );
            }
        }
        self.w("\t\tdefault:");
        self.w("\t\t\tunknown += rtUnknownSize(f)");
        self.w("\t\t\tif unknown > l.MaxUnknownFieldBytes {");
        self.w("\t\t\t\treturn rtErr(\"payload exceeded configured byte limit\")");
        self.w("\t\t\t}");
        self.w("\t\t\tf.Bytes = append([]byte(nil), f.Bytes...)");
        self.w("\t\t\tm.UnknownFields = append(m.UnknownFields, f)");
        self.w("\t\t}");
        self.w("\t}");
        self.w("\treturn nil");
        self.w("}");
    }

    fn decode_map(&mut self, a: &str, f: &Field, k: Scalar) {
        let kget = match k {
            Scalar::String => "Tpt20GetString(*ke, l)".to_string(),
            _ => format!("rtGetWord(*ke, {}, {})", class_const(k), word_fn("from", k)),
        };
        let vget = match f.ty {
            Ty::Scalar(Scalar::String) => "Tpt20GetString(*ve, l)".to_string(),
            Ty::Scalar(Scalar::Bytes) => "Tpt20GetBytes(*ve, l)".to_string(),
            Ty::Scalar(s) => format!("rtGetWord(*ve, {}, {})", class_const(s), word_fn("from", s)),
            Ty::Enum(i) => format!(
                "rtEnumOf[{}](*ve, {})",
                self.ename(i),
                self.valid_fn(f.ty)
            ),
            Ty::Message(i) => format!("rtMessageOf(*ve, l, depth+1, func(b []byte) (*{t}, error) {{ v := &{t}{{}}; return v, v.decode(b, l, depth+1) }})", t = self.tname(i)),
        };
        let vempty = match f.ty {
            Ty::Message(i) => format!(
                "{{\n\t\t\tv = &{t}{{}}\n\t\t\tif err := v.decode(nil, l, depth+1); err != nil {{\n\t\t\t\treturn err\n\t\t\t}}\n\t\t}}",
                t = self.tname(i)
            ),
            _ => "{\n\t\t\t_ = v\n\t\t}".to_string(),
        };
        let kt = go_scalar(k);
        let vt = self.go_ty(f.ty);
        let _ = writeln!(
            self.out,
            "\t\tb, err := Tpt20GetLen(f)\n\t\tif err != nil {{\n\t\t\treturn err\n\t\t}}\n\t\tke, ve, err := Tpt20MapEntry(b, l)\n\t\tif err != nil {{\n\t\t\treturn err\n\t\t}}\n\t\tvar key {kt}\n\t\tif ke != nil {{\n\t\t\tkey, err = {kget}\n\t\t\tif err != nil {{\n\t\t\t\treturn err\n\t\t\t}}\n\t\t}}\n\t\tvar v {vt}\n\t\tif ve != nil {{\n\t\t\tv, err = {vget}\n\t\t\tif err != nil {{\n\t\t\t\treturn err\n\t\t\t}}\n\t\t}} else {vempty}\n\t\tif {a} == nil {{\n\t\t\t{a} = map[{kt}]{vt}{{}}\n\t\t}}\n\t\t{a}[key] = v\n\t\tif err := rtCheckMap(len({a}), l); err != nil {{\n\t\t\treturn err\n\t\t}}"
        );
    }

    fn full_service_name(&self, s: &crate::model::Service) -> String {
        if self.model.package.is_empty() {
            s.name.clone()
        } else {
            format!("{}.{}", self.model.package, s.name)
        }
    }

    fn services(&mut self, pkg: &str) {
        self.w("// Code generated by tpt20-codegen-backends (go). DO NOT EDIT.");
        let _ = write!(
            self.out,
            "\npackage {pkg}\n\nimport (\n\t\"context\"\n\t\"io\"\n)\n\nvar (\n\t_ = context.Background\n\t_ = io.EOF\n)\n"
        );
        for s in &self.model.services {
            self.service(s);
        }
    }

    fn service(&mut self, s: &crate::model::Service) {
        let name = export(&s.name);
        let full = self.full_service_name(s);
        // Handler interface.
        let _ = write!(
            self.out,
            "\n// {name}Handler is implemented by servers of service `{full}`.\ntype {name}Handler interface {{\n"
        );
        for m in &s.methods {
            let mname = pascal(&m.name);
            let q = self.tname(m.request);
            let r = self.tname(m.response);
            let sig = match (m.client_streaming, m.server_streaming) {
                (false, false) => format!(
                    "{mname}(ctx context.Context, md Tpt20Metadata, req *{q}) (*{r}, error)"
                ),
                (false, true) => format!(
                    "{mname}(ctx context.Context, md Tpt20Metadata, req *{q}, stream *Tpt20ServerStream[*{q}, *{r}]) error"
                ),
                (true, false) => format!(
                    "{mname}(ctx context.Context, md Tpt20Metadata, stream *Tpt20ServerStream[*{q}, *{r}]) (*{r}, error)"
                ),
                (true, true) => format!(
                    "{mname}(ctx context.Context, md Tpt20Metadata, stream *Tpt20ServerStream[*{q}, *{r}]) error"
                ),
            };
            let _ = writeln!(self.out, "\t{sig}");
        }
        self.w("}");
        // Registration + dispatch.
        let _ = write!(
            self.out,
            "\n// Register{name} adds the service to a server.\nfunc Register{name}(s *Tpt20Server, h {name}Handler) {{\n\ts.Register(&{lname}Service{{h}})\n}}\n\ntype {lname}Service struct{{ h {name}Handler }}\n\nfunc (*{lname}Service) Tpt20ServiceName() string {{ return {full:?} }}\n\nfunc (s *{lname}Service) Tpt20Handle(ctx context.Context, method string, call *Tpt20ServerCall) error {{\n\tswitch method {{\n",
            lname = name.to_lowercase()
        );
        for m in &s.methods {
            let mname = pascal(&m.name);
            let q = self.tname(m.request);
            let r = self.tname(m.response);
            let stream = format!(
                "Tpt20NewServerStream(call, Decode{q}, func(m *{r}) []byte {{ return m.Encode() }})"
            );
            let body = match (m.client_streaming, m.server_streaming) {
                (false, false) => format!(
                    "b, err := call.Recv()\n\t\tif err != nil {{\n\t\t\treturn err\n\t\t}}\n\t\treq, err := Decode{q}(b)\n\t\tif err != nil {{\n\t\t\treturn Tpt20Error(Tpt20InvalidArgument, \"invalid request message: \"+err.Error())\n\t\t}}\n\t\tresp, err := s.h.{mname}(ctx, call.Metadata, req)\n\t\tif err != nil {{\n\t\t\treturn err\n\t\t}}\n\t\treturn call.Send(resp.Encode())"
                ),
                (false, true) => format!(
                    "b, err := call.Recv()\n\t\tif err != nil {{\n\t\t\treturn err\n\t\t}}\n\t\treq, err := Decode{q}(b)\n\t\tif err != nil {{\n\t\t\treturn Tpt20Error(Tpt20InvalidArgument, \"invalid request message: \"+err.Error())\n\t\t}}\n\t\treturn s.h.{mname}(ctx, call.Metadata, req, {stream})"
                ),
                (true, false) => format!(
                    "resp, err := s.h.{mname}(ctx, call.Metadata, {stream})\n\t\tif err != nil {{\n\t\t\treturn err\n\t\t}}\n\t\treturn call.Send(resp.Encode())"
                ),
                (true, true) => format!("return s.h.{mname}(ctx, call.Metadata, {stream})"),
            };
            let _ = writeln!(self.out, "\tcase {:?}:\n\t\t{body}", m.name);
        }
        self.w("\tdefault:");
        self.w("\t\treturn Tpt20Error(Tpt20Unimplemented, \"unknown method `\"+method+\"`\")");
        self.w("\t}");
        self.w("}");
        // Client.
        let _ = write!(
            self.out,
            "\n// {name}Client calls service `{full}`.\ntype {name}Client struct{{ ch *Tpt20Channel }}\n\n// New{name}Client wraps a channel.\nfunc New{name}Client(ch *Tpt20Channel) *{name}Client {{ return &{name}Client{{ch}} }}\n"
        );
        for m in &s.methods {
            let mname = pascal(&m.name);
            let q = self.tname(m.request);
            let r = self.tname(m.response);
            let path = format!("{full}/{}", m.name);
            let enc = format!("func(m *{q}) []byte {{ return m.Encode() }}");
            match (m.client_streaming, m.server_streaming) {
                (false, false) => {
                    let _ = write!(
                        self.out,
                        "\nfunc (c *{name}Client) {mname}(ctx context.Context, md Tpt20Metadata, req *{q}) (*{r}, error) {{\n\tb, err := c.ch.Unary(ctx, {path:?}, md, req.Encode())\n\tif err != nil {{\n\t\treturn nil, err\n\t}}\n\tresp, err := Decode{r}(b)\n\tif err != nil {{\n\t\treturn nil, Tpt20Error(Tpt20Internal, \"invalid response message: \"+err.Error())\n\t}}\n\treturn resp, nil\n}}\n"
                    );
                }
                (false, true) => {
                    let _ = write!(
                        self.out,
                        "\nfunc (c *{name}Client) {mname}(ctx context.Context, md Tpt20Metadata, req *{q}) (*Tpt20Stream[*{q}, *{r}], error) {{\n\tcall, err := c.ch.Start(ctx, {path:?}, md)\n\tif err != nil {{\n\t\treturn nil, err\n\t}}\n\tgo func() {{\n\t\t_ = call.Send(req.Encode())\n\t\t_ = call.CloseSend()\n\t}}()\n\treturn Tpt20NewStream(call, {enc}, Decode{r}), nil\n}}\n"
                    );
                }
                (_, _) => {
                    let _ = write!(
                        self.out,
                        "\nfunc (c *{name}Client) {mname}(ctx context.Context, md Tpt20Metadata) (*Tpt20Stream[*{q}, *{r}], error) {{\n\tcall, err := c.ch.Start(ctx, {path:?}, md)\n\tif err != nil {{\n\t\treturn nil, err\n\t}}\n\treturn Tpt20NewStream(call, {enc}, Decode{r}), nil\n}}\n"
                    );
                }
            }
        }
    }
}
