//! Identifier and case-convention utilities for Rust code generation.

/// Rust keywords (including reserved) that cannot be used as plain identifiers.
const RUST_KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn",
    "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "static", "struct", "trait", "true", "type", "unsafe", "use", "where", "while",
    "async", "await", "box", "try", "abstract", "become", "do", "final", "macro", "override",
    "priv", "typeof", "unsized", "virtual", "yield",
];

/// Keywords that cannot be written as raw identifiers (`r#self` is invalid).
const NO_RAW_IDENT: &[&str] = &["self", "Self", "super", "crate"];

/// Converts a schema identifier into a valid Rust identifier.
///
/// Keywords become raw identifiers (`r#type`); identifiers that cannot be raw
/// get a trailing underscore.
pub fn sanitize_ident(name: &str) -> String {
    if NO_RAW_IDENT.contains(&name) {
        format!("{name}_")
    } else if RUST_KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

/// `email_addr` -> `EmailAddr` (PascalCase; used for oneof variants/types).
pub fn pascal(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper_next = true;
    for c in name.chars() {
        if c == '_' || c == '-' || c == '.' {
            upper_next = true;
        } else if upper_next {
            out.extend(c.to_uppercase());
            upper_next = false;
        } else {
            out.push(c);
        }
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// Sanitized snake identifier for field/local names (schemas already use
/// snake_case; this guards against keywords).
pub fn field_ident(name: &str) -> String {
    sanitize_ident(name)
}

/// `user_id` -> `userId` (lowerCamelCase JSON alias, spec §14.2).
pub fn lower_camel(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper_next = false;
    for c in name.chars() {
        if c == '_' {
            upper_next = true;
        } else if upper_next {
            out.extend(c.to_uppercase());
            upper_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Flattens a nested type's scope path into a Rust type name:
/// `["Outer", "Inner"]` -> `Outer_Inner`.
pub fn flat_type_name(scope: &[String], name: &str) -> String {
    let mut out = String::new();
    for part in scope {
        out.push_str(part);
        out.push('_');
    }
    out.push_str(name);
    out
}

/// Derives the output file stem for a package: `user.v1` -> `user_v1`.
pub fn package_file_stem(package: Option<&str>) -> String {
    let base = package.unwrap_or("generated");
    base.replace(['.', '-'], "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_keywords() {
        assert_eq!(sanitize_ident("type"), "r#type");
        assert_eq!(sanitize_ident("self"), "self_");
        assert_eq!(sanitize_ident("user_id"), "user_id");
    }

    #[test]
    fn casing() {
        assert_eq!(pascal("email_addr"), "EmailAddr");
        assert_eq!(pascal("phone"), "Phone");
        assert_eq!(lower_camel("user_id"), "userId");
        assert_eq!(lower_camel("id"), "id");
    }

    #[test]
    fn flattening_and_files() {
        assert_eq!(
            flat_type_name(&["Outer".to_string()], "Inner"),
            "Outer_Inner"
        );
        assert_eq!(package_file_stem(Some("user.v1")), "user_v1");
        assert_eq!(package_file_stem(None), "generated");
    }
}
