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
/// Rust variant name for an enum value. `SCREAMING_SNAKE` names (no lowercase
/// letters) are kept verbatim — `KIND_A` stays `KIND_A`, never the lossy
/// `KINDA` — everything else is PascalCased.
pub fn enum_variant(name: &str) -> String {
    let screaming = !name.chars().any(|c| c.is_ascii_lowercase());
    if screaming {
        sanitize_ident(name)
    } else {
        sanitize_ident(&pascal(name))
    }
}

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

/// `GetUser` -> `get_user`, `HTTPServer` -> `http_server` (keyword-safe).
pub fn snake(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::with_capacity(name.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if c == '-' || c == '.' {
            out.push('_');
            continue;
        }
        if c.is_uppercase() {
            let prev_lower =
                i > 0 && (chars[i - 1].is_lowercase() || chars[i - 1].is_ascii_digit());
            let acronym_end = i > 0
                && chars[i - 1].is_uppercase()
                && chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if (prev_lower || acronym_end) && !out.ends_with('_') {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    sanitize_ident(&out)
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
    fn snake_case() {
        assert_eq!(snake("GetUser"), "get_user");
        assert_eq!(snake("get_user"), "get_user");
        assert_eq!(snake("HTTPServer"), "http_server");
        assert_eq!(snake("ListV2Items"), "list_v2_items");
        assert_eq!(snake("Type"), "r#type");
        assert_eq!(snake("Ping"), "ping");
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

#[cfg(test)]
mod enum_variant_tests {
    use super::enum_variant;

    #[test]
    fn screaming_snake_is_kept_and_distinct() {
        assert_eq!(enum_variant("KIND_A"), "KIND_A");
        assert_eq!(enum_variant("KINDA"), "KINDA");
        assert_ne!(enum_variant("KIND_A"), enum_variant("KINDA"));
        assert_eq!(enum_variant("SUSPENDED"), "SUSPENDED");
        assert_eq!(enum_variant("active_now"), "ActiveNow");
        assert_eq!(enum_variant("Already"), "Already");
    }
}
