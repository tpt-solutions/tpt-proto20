//! Schema compatibility detector (spec §20).
//!
//! Compares two compiled IR packages and classifies every difference as
//! [`ChangeClass::Safe`], [`ChangeClass::Warning`], or
//! [`ChangeClass::Breaking`], producing a report in the format used by the
//! `tpt20 diff` CLI (spec §21.4).

use tpt20_ir as ir;

/// Compatibility classification of a schema change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeClass {
    /// Backward-compatible change.
    Safe,
    /// Compatible but worth reviewing.
    Warning,
    /// Breaks backward compatibility.
    Breaking,
}

impl ChangeClass {
    /// The uppercase label used when rendering the report.
    pub fn label(self) -> &'static str {
        match self {
            ChangeClass::Safe => "SAFE",
            ChangeClass::Warning => "WARNING",
            ChangeClass::Breaking => "BREAKING",
        }
    }
}

/// A single compatibility change with its classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatChange {
    /// Classification of the change.
    pub class: ChangeClass,
    /// Human-readable description.
    pub message: String,
}

impl CompatChange {
    /// Renders the change in `tpt20 diff` format (`CLASS  message`).
    pub fn render(&self) -> String {
        format!("{:<8} {}", self.class.label(), self.message)
    }
}

fn id_reserved(id: u32, reserved: &[ir::ReservedIr]) -> bool {
    for r in reserved {
        for rid in &r.ids {
            match rid {
                ir::ReservedIdIr::Single(n) => {
                    if *n == id {
                        return true;
                    }
                }
                ir::ReservedIdIr::Range(lo, hi) => {
                    if id >= *lo && id <= *hi {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn diff_fields(
    old: &[ir::FieldIr],
    new: &[ir::FieldIr],
    reserved: &[ir::ReservedIr],
    out: &mut Vec<CompatChange>,
) {
    use std::collections::HashMap;
    let old_by_id: HashMap<u32, &ir::FieldIr> = old.iter().map(|f| (f.id, f)).collect();
    let new_by_id: HashMap<u32, &ir::FieldIr> = new.iter().map(|f| (f.id, f)).collect();

    for f in new {
        match old_by_id.get(&f.id) {
            None => {
                out.push(CompatChange {
                    class: ChangeClass::Safe,
                    message: format!("added field {} {}", f.id, f.name),
                });
            }
            Some(o) => {
                if o.name != f.name {
                    out.push(CompatChange {
                        class: ChangeClass::Warning,
                        message: format!("renamed field {} to {}", o.name, f.name),
                    });
                }
                if o.label != f.label {
                    out.push(CompatChange {
                        class: ChangeClass::Breaking,
                        message: format!(
                            "changed type of field {} ({}{})",
                            f.id,
                            o.label.unwrap_type().name(),
                            if matches!(o.label, ir::FieldLabelIr::Repeated(_)) {
                                " (repeated)"
                            } else {
                                ""
                            }
                        ),
                    });
                }
            }
        }
    }

    for f in old {
        if !new_by_id.contains_key(&f.id) {
            if id_reserved(f.id, reserved) {
                out.push(CompatChange {
                    class: ChangeClass::Safe,
                    message: format!("removed field {} (id {} reserved)", f.name, f.id),
                });
            } else {
                out.push(CompatChange {
                    class: ChangeClass::Breaking,
                    message: format!("removed field {} without reservation", f.id),
                });
            }
        }
    }
}

fn diff_enums(old: &ir::EnumIr, new: &ir::EnumIr, out: &mut Vec<CompatChange>) {
    use std::collections::HashMap;
    let old_by_num: HashMap<i32, &ir::EnumValueIr> =
        old.values.iter().map(|v| (v.number, v)).collect();
    let new_by_num: HashMap<i32, &ir::EnumValueIr> =
        new.values.iter().map(|v| (v.number, v)).collect();

    for v in &new.values {
        match old_by_num.get(&v.number) {
            None => {
                let class = if new.open {
                    ChangeClass::Safe
                } else {
                    ChangeClass::Warning
                };
                out.push(CompatChange {
                    class,
                    message: format!("added enum value {} {} to {}", v.number, v.name, new.name),
                });
            }
            Some(o) => {
                if o.name != v.name {
                    out.push(CompatChange {
                        class: ChangeClass::Warning,
                        message: format!(
                            "renamed enum value {} to {} in {}",
                            o.name, v.name, new.name
                        ),
                    });
                }
            }
        }
    }
    for v in &old.values {
        if !new_by_num.contains_key(&v.number) {
            out.push(CompatChange {
                class: ChangeClass::Warning,
                message: format!(
                    "removed enum value {} {} from {}",
                    v.number, v.name, old.name
                ),
            });
        }
    }
}

fn diff_messages(old: &ir::MessageIr, new: &ir::MessageIr, out: &mut Vec<CompatChange>) {
    diff_fields(&old.fields, &new.fields, &new.reserved, out);
    // Recurse into nested messages by name.
    for nm in &new.messages {
        if let Some(om) = old.messages.iter().find(|m| m.name == nm.name) {
            diff_messages(om, nm, out);
        } else {
            out.push(CompatChange {
                class: ChangeClass::Safe,
                message: format!("added message {}", nm.name),
            });
        }
    }
    for om in &old.messages {
        if !new.messages.iter().any(|m| m.name == om.name) {
            out.push(CompatChange {
                class: ChangeClass::Breaking,
                message: format!("removed message {}", om.name),
            });
        }
    }
    // Nested enums.
    for ne in &new.enums {
        if let Some(oe) = old.enums.iter().find(|e| e.name == ne.name) {
            diff_enums(oe, ne, out);
        } else {
            out.push(CompatChange {
                class: ChangeClass::Safe,
                message: format!("added enum {} in {}", ne.name, new.name),
            });
        }
    }
}

fn diff_services(old: &ir::ServiceIr, new: &ir::ServiceIr, out: &mut Vec<CompatChange>) {
    use std::collections::HashMap;
    let old_by_name: HashMap<&str, &ir::MethodIr> =
        old.methods.iter().map(|m| (m.name.as_str(), m)).collect();
    let new_by_name: HashMap<&str, &ir::MethodIr> =
        new.methods.iter().map(|m| (m.name.as_str(), m)).collect();

    for m in &new.methods {
        // A new method with an identical signature to an old, differently
        // named method is treated as a rename (warning) rather than an add.
        let renamed = old.methods.iter().any(|om| {
            om.name != m.name
                && om.request == m.request
                && om.response == m.response
                && om.request_streaming == m.request_streaming
                && om.response_streaming == m.response_streaming
        });
        match old_by_name.get(m.name.as_str()) {
            None if renamed => out.push(CompatChange {
                class: ChangeClass::Warning,
                message: format!("renamed method to {}", m.name),
            }),
            None => out.push(CompatChange {
                class: ChangeClass::Safe,
                message: format!("added method {}", m.name),
            }),
            Some(o) => {
                if o.request != m.request || o.response != m.response {
                    out.push(CompatChange {
                        class: ChangeClass::Breaking,
                        message: format!(
                            "incompatible request/response type change for method {}",
                            m.name
                        ),
                    });
                }
                if o.request_streaming != m.request_streaming
                    || o.response_streaming != m.response_streaming
                {
                    out.push(CompatChange {
                        class: ChangeClass::Breaking,
                        message: format!("changed streaming direction for method {}", m.name),
                    });
                }
            }
        }
    }
    for m in &old.methods {
        if !new_by_name.contains_key(m.name.as_str()) {
            out.push(CompatChange {
                class: ChangeClass::Breaking,
                message: format!("removed method {} without compatibility policy", m.name),
            });
        }
    }
}

/// Compares two compiled IR packages and returns the classified change list.
pub fn diff(old: &ir::PackageIr, new: &ir::PackageIr) -> Vec<CompatChange> {
    use std::collections::HashMap;
    let mut out = Vec::new();

    let old_msgs: HashMap<&str, &ir::MessageIr> =
        old.messages.iter().map(|m| (m.name.as_str(), m)).collect();
    for nm in &new.messages {
        match old_msgs.get(nm.name.as_str()) {
            Some(om) => diff_messages(om, nm, &mut out),
            None => out.push(CompatChange {
                class: ChangeClass::Safe,
                message: format!("added message {}", nm.name),
            }),
        }
    }
    for om in &old.messages {
        if !new.messages.iter().any(|m| m.name == om.name) {
            out.push(CompatChange {
                class: ChangeClass::Breaking,
                message: format!("removed message {}", om.name),
            });
        }
    }

    let old_enums: HashMap<&str, &ir::EnumIr> =
        old.enums.iter().map(|e| (e.name.as_str(), e)).collect();
    for ne in &new.enums {
        match old_enums.get(ne.name.as_str()) {
            Some(oe) => diff_enums(oe, ne, &mut out),
            None => out.push(CompatChange {
                class: ChangeClass::Safe,
                message: format!("added enum {}", ne.name),
            }),
        }
    }

    let old_svcs: HashMap<&str, &ir::ServiceIr> =
        old.services.iter().map(|s| (s.name.as_str(), s)).collect();
    for ns in &new.services {
        match old_svcs.get(ns.name.as_str()) {
            Some(os) => diff_services(os, ns, &mut out),
            None => out.push(CompatChange {
                class: ChangeClass::Safe,
                message: format!("added service {}", ns.name),
            }),
        }
    }
    for os in &old.services {
        if !new.services.iter().any(|s| s.name == os.name) {
            out.push(CompatChange {
                class: ChangeClass::Breaking,
                message: format!("removed service {}", os.name),
            });
        }
    }

    out
}

/// Renders a compatibility report, one change per line.
pub fn render_report(changes: &[CompatChange]) -> String {
    changes
        .iter()
        .map(|c| c.render())
        .collect::<Vec<_>>()
        .join("\n")
}
