//! Error types for Nix evaluation
//!
//! All errors follow the "cannot" prefix convention for user-facing messages.

use codespan::FileId;
use thiserror::Error;

/// Source location span for an expression or error
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub file_id: Option<FileId>,
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(file_id: Option<FileId>, start: usize, end: usize) -> Self {
        Self {
            file_id,
            start,
            end,
        }
    }

    pub fn to_range(&self) -> std::ops::Range<usize> {
        self.start..self.end
    }
}

/// Error type for Nix evaluation
///
/// All errors follow the "cannot" prefix convention for user-facing messages.
///
/// # Example
///
/// ```no_run
/// use nix_eval::{Evaluator, Error};
///
/// let evaluator = Evaluator::new();
/// match evaluator.evaluate("invalid syntax {") {
///     Err(Error::ParseError { reason }) => {
///         println!("Parse error: {}", reason);
///     }
///     _ => {}
/// }
/// ```
#[derive(Debug, Error)]
pub enum Error {
    /// Parse error occurred when tokenizing or parsing the Nix expression
    #[error("cannot parse nix expression: {reason}")]
    ParseError { reason: String },

    /// Failed to convert parsed syntax tree to AST root
    #[error("cannot convert to AST root")]
    AstConversionError,

    /// No expression found in the parsed input
    #[error("no expression found")]
    NoExpression,

    /// Expression type is not supported by the evaluator
    ///
    /// This typically occurs when encountering expressions like function calls,
    /// variable references, or other advanced Nix features that are not yet implemented.
    #[error("cannot evaluate unsupported expression type: {reason}")]
    UnsupportedExpression { reason: String },

    /// A builtin function failed at runtime (bad arguments, fetch failures,
    /// hash mismatches, etc.). Unlike [`Error::UnsupportedExpression`], the
    /// message is presented to the user verbatim without an internal prefix.
    #[error("{reason}")]
    EvaluationError { reason: String },

    /// Literal value could not be parsed or is unsupported
    #[error("cannot evaluate unsupported literal: {literal}")]
    UnsupportedLiteral { literal: String },

    /// Infinite recursion detected (blackhole)
    ///
    /// This error occurs when a thunk tries to evaluate itself while it's already
    /// being evaluated, indicating infinite recursion. The blackhole marker prevents
    /// stack overflow by detecting this condition early.
    #[error("infinite recursion detected: thunk is already being evaluated (blackhole)")]
    InfiniteRecursion,

    /// Recursion limit exceeded
    #[error("recursion limit exceeded: expression is too deeply nested")]
    RecursionLimitExceeded,

    /// IO error occurred during file operations
    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),

    /// Evaluation error with a source location.
    #[error("{error}")]
    SpannedError { error: Box<Error>, span: Span },
}

impl Error {
    /// Attach an evaluation frame to this error.
    pub fn with_span(self, span: Span) -> Self {
        Self::SpannedError {
            error: Box::new(self),
            span,
        }
    }

    /// Source spans from the innermost failure to its outer evaluation frames.
    pub fn spans(&self) -> Vec<Span> {
        let mut spans = Vec::new();
        let mut error = self;
        while let Self::SpannedError { error: inner, span } = error {
            spans.push(*span);
            error = inner;
        }
        spans.reverse();
        spans
    }

    /// Return the innermost (most specific) source span associated with this error.
    pub fn span(&self) -> Option<Span> {
        self.spans().into_iter().next()
    }

    /// Render this error using annotate-snippets and the given evaluator
    pub fn render_annotated(&self, evaluator: &crate::Evaluator) -> String {
        evaluator.render_error(self)
    }
}

/// Result type alias for the library
///
/// All library functions return `Result<T, Error>` where `T` is the success type.
///
/// # Example
///
/// ```no_run
/// use nix_eval::{Evaluator, Result};
///
/// fn evaluate_safely(expr: &str) -> Result<()> {
///     let evaluator = Evaluator::new();
///     let _value = evaluator.evaluate(expr)?;
///     Ok(())
/// }
/// ```
pub type Result<T> = std::result::Result<T, Error>;
