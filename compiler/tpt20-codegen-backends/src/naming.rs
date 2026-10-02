//! Identifier helpers shared by the backends.

/// `email_addr` → `EmailAddr` (only the first letter of each `_`/`-`/`.`
/// separated part is upper-cased; the rest is kept).
pub fn pascal(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut up = true;
    for c in name.chars() {
        if c == '_' || c == '-' || c == '.' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// `EmailAddr` / `email_addr` → `emailAddr`.
pub fn camel(name: &str) -> String {
    let p = pascal(name);
    let mut chars = p.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => p,
    }
}

/// Appends `_` to names that collide with `reserved`.
pub fn avoid(name: &str, reserved: &[&str]) -> String {
    if reserved.contains(&name) {
        format!("{name}_")
    } else {
        name.to_string()
    }
}

/// Derives a module/package name from a schema package (`demo.v1` → `demo_v1`).
pub fn package_ident(package: &str, fallback: &str) -> String {
    let s: String = package
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if s.is_empty() {
        fallback.to_string()
    } else if s.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{s}")
    } else {
        s
    }
}
