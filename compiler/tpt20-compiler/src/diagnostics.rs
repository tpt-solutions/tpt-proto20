//! Diagnostics engine for the tpt20 compiler (spec §7.3).
//!
//! Every diagnostic carries a file path, line, column, span, severity, a
//! stable error code, a human-readable explanation, and an optional suggested
//! fix. The rendered format matches the example in the design document:
//!
//! ```text
//! error[E0042]: field ID 3 was removed without reservation
//!   --> schema/user.v1.tpt:12:3
//!   |
//!   = help: reserve field ID 3 to preserve compatibility
//! ```

use std::fmt;

/// Diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// A hard error that prevents compilation.
    Error,
    /// A non-fatal warning.
    Warning,
    /// Informational note.
    Info,
}

impl Severity {
    /// The keyword used when rendering this severity.
    pub fn keyword(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

/// A single compiler diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Diagnostic {
    /// Severity of the diagnostic.
    pub severity: Severity,
    /// Stable error code, e.g. `"E0042"`.
    pub code: &'static str,
    /// Human-readable explanation.
    pub message: String,
    /// Originating file path, if known.
    pub file: Option<String>,
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number.
    pub column: usize,
    /// Source span as `(line, column)` of the diagnostic's start.
    pub span: (usize, usize),
    /// Optional suggested fix.
    pub help: Option<String>,
}

impl Diagnostic {
    /// Creates an error diagnostic with the given code and message.
    pub fn error(code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            code,
            message: message.into(),
            file: None,
            line: 0,
            column: 0,
            span: (0, 0),
            help: None,
        }
    }

    /// Creates a warning diagnostic.
    pub fn warning(code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            code,
            message: message.into(),
            file: None,
            line: 0,
            column: 0,
            span: (0, 0),
            help: None,
        }
    }

    /// Creates an info diagnostic.
    pub fn info(code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Info,
            code,
            message: message.into(),
            file: None,
            line: 0,
            column: 0,
            span: (0, 0),
            help: None,
        }
    }

    /// Attaches a source location.
    pub fn at(mut self, line: usize, column: usize) -> Diagnostic {
        self.line = line;
        self.column = column;
        self.span = (line, column);
        self
    }

    /// Attaches the originating file path.
    pub fn in_file(mut self, file: impl Into<String>) -> Diagnostic {
        self.file = Some(file.into());
        self
    }

    /// Attaches a suggested fix.
    pub fn with_help(mut self, help: impl Into<String>) -> Diagnostic {
        self.help = Some(help.into());
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let loc = match &self.file {
            Some(file) => format!("{}:{}:{}", file, self.line, self.column),
            None => format!("{}:{}", self.line, self.column),
        };
        writeln!(
            f,
            "{}[{}]: {}",
            self.severity.keyword(),
            self.code,
            self.message
        )?;
        writeln!(f, "  --> {loc}")?;
        writeln!(f, "  |")?;
        if let Some(help) = &self.help {
            writeln!(f, "  = help: {help}")?;
        }
        Ok(())
    }
}

/// Renders a list of diagnostics in order.
pub fn render_all(diagnostics: &[Diagnostic]) -> String {
    let mut out = String::new();
    for d in diagnostics {
        out.push_str(&d.to_string());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_spec_example_format() {
        let d = Diagnostic::error("E0042", "field ID 3 was removed without reservation")
            .in_file("schema/user.v1.tpt")
            .at(12, 3)
            .with_help("reserve field ID 3 to preserve compatibility");
        let rendered = d.to_string();
        assert!(rendered.contains("error[E0042]: field ID 3 was removed without reservation"));
        assert!(rendered.contains("--> schema/user.v1.tpt:12:3"));
        assert!(rendered.contains("= help: reserve field ID 3 to preserve compatibility"));
    }
}
