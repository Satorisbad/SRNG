#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl Diagnostic {
    pub fn error(code: &'static str, message: impl Into<String>, line: usize, column: usize) -> Self {
        Self { severity: Severity::Error, code, message: message.into(), line, column }
    }

    pub fn warning(code: &'static str, message: impl Into<String>, line: usize, column: usize) -> Self {
        Self { severity: Severity::Warning, code, message: message.into(), line, column }
    }
}
