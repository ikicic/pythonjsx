//! Compilation error types.

use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileErrorSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
pub struct CompileError {
    pub message: String,
    pub range: Range<usize>,
    pub severity: CompileErrorSeverity,
}

impl CompileError {
    pub fn error(message: impl Into<String>, range: Range<usize>) -> Self {
        Self {
            message: message.into(),
            range,
            severity: CompileErrorSeverity::Error,
        }
    }

    pub fn warning(message: impl Into<String>, range: Range<usize>) -> Self {
        Self {
            message: message.into(),
            range,
            severity: CompileErrorSeverity::Warning,
        }
    }
}
