use serde::{Deserialize, Serialize};

/// A lint warning or error produced during schema validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintDiagnostic {
    pub severity: LintSeverity,
    pub message: String,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LintSeverity {
    Warning,
    Error,
}

/// Result of a schema lint run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintResult {
    pub diagnostics: Vec<LintDiagnostic>,
    pub valid: bool,
}

impl LintResult {
    pub fn valid() -> Self {
        Self {
            diagnostics: vec![],
            valid: true,
        }
    }

    pub fn with_diagnostics(diagnostics: Vec<LintDiagnostic>) -> Self {
        let has_errors = diagnostics
            .iter()
            .any(|d| d.severity == LintSeverity::Error);
        Self {
            diagnostics,
            valid: !has_errors,
        }
    }
}


