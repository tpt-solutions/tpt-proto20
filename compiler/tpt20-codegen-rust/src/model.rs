//! Type registry built from the package IR: resolves dotted type paths to
//! flattened Rust type names and their kinds.

use std::collections::HashMap;

use crate::scalars::{scalar_info, ScalarInfo};
use crate::WireClass;
use tpt20_ir as ir;

/// The kind of a referenced type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    /// A known scalar type.
    Scalar(ScalarInfo),
    /// A message type.
    Message,
    /// An enum type; openness drives unknown-value handling.
    Enum {
        /// Whether unknown values are preserved (open) or rejected (closed).
        open: bool,
    },
}

/// Registry of all declared types in a package plus scalar knowledge.
#[derive(Debug, Default)]
pub struct Model {
    /// Dotted path ("Outer.Inner") -> flat Rust type name ("Outer_Inner").
    pub names: HashMap<String, String>,
    /// Dotted path -> kind.
    pub kinds: HashMap<String, TypeKind>,
}

impl Model {
    /// Walks all messages/enums (including nested) building the registry.
    pub fn build(pkg: &ir::PackageIr) -> Model {
        let mut m = Model::default();
        for msg in &pkg.messages {
            m.add_message(&[], msg);
        }
        for e in &pkg.enums {
            let path = join(&[], &e.name);
            m.names.insert(path.clone(), flat_name(&[], &e.name));
            m.kinds.insert(path, TypeKind::Enum { open: e.open });
        }
        m
    }

    fn add_message(&mut self, scope: &[String], msg: &ir::MessageIr) {
        let path = join(scope, &msg.name);
        let flat = flat_name(scope, &msg.name);
        self.names.insert(path.clone(), flat);
        self.kinds.insert(path, TypeKind::Message);
        let mut inner: Vec<String> = scope.to_vec();
        inner.push(msg.name.clone());
        for nested in &msg.messages {
            self.add_message(&inner, nested);
        }
        for e in &msg.enums {
            let p = join(&inner, &e.name);
            self.names.insert(p.clone(), flat_name(&inner, &e.name));
            self.kinds.insert(p, TypeKind::Enum { open: e.open });
        }
    }

    /// Resolves a type reference relative to its lexical scope. Tries the full
    /// scoped path first, then the bare path, then a unique suffix match so a
    /// bare `Child` resolves to its enclosing `Outer.Child`.
    pub fn resolve(&self, scope: &[String], path: &[String]) -> Option<(&str, TypeKind)> {
        let candidates = [
            join(scope, &path.join(".")),
            join(&[], &path.join(".")),
        ];
        for key in &candidates {
            if let (Some(name), Some(kind)) = (self.names.get(key), self.kinds.get(key)) {
                return Some((name.as_str(), *kind));
            }
        }
        // Suffix fallback: shortest key ending in `.Path` (deterministic).
        let suffix = format!(".{}", path.join("."));
        let mut best: Option<&String> = None;
        for key in self.names.keys() {
            if key.ends_with(&suffix) {
                match best {
                    Some(b) if b.len() <= key.len() => {}
                    _ => best = Some(key),
                }
            }
        }
        best.and_then(|key| {
            let name = self.names.get(key)?;
            let kind = self.kinds.get(key)?;
            Some((name.as_str(), *kind))
        })
    }

    /// Wire class and rust facts for a resolved kind.
    pub fn wire_class(kind: TypeKind) -> Option<WireClass> {
        match kind {
            TypeKind::Scalar(info) => Some(info.class),
            TypeKind::Message => Some(WireClass::Len),
            TypeKind::Enum { .. } => Some(WireClass::Varint),
        }
    }
}

fn join(scope: &[String], last: &str) -> String {
    let mut parts: Vec<&str> = scope.iter().map(String::as_str).collect();
    parts.push(last);
    parts.join(".")
}

fn flat_name(scope: &[String], name: &str) -> String {
    crate::naming::flat_type_name(scope, name)
}

/// Returns true when the dotted path refers to a known scalar type.
pub fn is_scalar_path(path: &[String]) -> bool {
    path.len() == 1
        && path
            .first()
            .is_some_and(|p| scalar_info(p).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt20_ir as ir;

    fn pkg() -> ir::PackageIr {
        ir::PackageIr {
            messages: vec![ir::MessageIr {
                name: "Outer".into(),
                fields: vec![],
                oneofs: vec![],
                messages: vec![ir::MessageIr {
                    name: "Inner".into(),
                    fields: vec![],
                    oneofs: vec![],
                    messages: vec![],
                    enums: vec![],
                    reserved: vec![],
                    annotations: vec![],
                    span: Default::default(),
                }],
                enums: vec![],
                reserved: vec![],
                annotations: vec![],
                span: Default::default(),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn resolves_scoped_and_bare_paths() {
        let m = Model::build(&pkg());
        assert_eq!(
            m.resolve(&["Outer".to_string()], &["Inner".to_string()]),
            Some(("Outer_Inner", TypeKind::Message))
        );
        assert!(matches!(
            m.resolve(&[], &["Outer".to_string()]),
            Some(("Outer", TypeKind::Message))
        ));
    }

    #[test]
    fn scalars_are_not_registered_but_known() {
        let m = Model::build(&pkg());
        assert_eq!(m.resolve(&[], &["string".to_string()]), None);
        assert!(is_scalar_path(&["int64".to_string()]));
        assert!(!is_scalar_path(&["User".to_string()]));
    }
}
