//! Log redaction utilities to prevent sensitive data from appearing in logs.

/// A wrapper that displays as `[REDACTED]` in debug/display output.
#[derive(Clone)]
pub struct Redacted<T>(T);

impl<T: std::fmt::Debug> std::fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[REDACTED]")
    }
}

impl<T: std::fmt::Display> std::fmt::Display for Redacted<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[REDACTED]")
    }
}

impl<T> Redacted<T> {
    pub fn new(value: T) -> Self {
        Self(value)
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

/// A string that is redacted in logs.
pub type Secret = Redacted<String>;
