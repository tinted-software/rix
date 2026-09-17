//! Thunk implementation for lazy evaluation
//!
//! A thunk represents a delayed computation in Nix. It stores the expression
//! to evaluate and its lexical closure (scope), allowing lazy evaluation
//! where expressions are only evaluated when their values are needed.

use crate::{Error, Evaluator, NixValue, Result, VariableScope};
use codespan::FileId;
use rix_parser::SyntaxNode;
use rix_parser::ast::{Expr, Root};
use rix_parser::parser::parse;
use rix_parser::tokenizer::tokenize;
use rowan::ast::AstNode;
use std::cell::Cell;
use std::sync::{Arc, Mutex};

thread_local! {
    /// Offset applied to AST text ranges when creating thunks, so that thunks
    /// created while evaluating a re-parsed expression still report spans in
    /// their original file coordinates. Maintained by [`Evaluator::set_span_base`].
    static SPAN_BASE: Cell<usize> = const { Cell::new(0) };
}

/// Set the thread-local span base, returning the previous value.
pub(crate) fn set_thread_span_base(base: usize) -> usize {
    SPAN_BASE.with(|c| c.replace(base))
}

/// Get the current thread-local span base.
pub(crate) fn thread_span_base() -> usize {
    SPAN_BASE.with(|c| c.get())
}

/// Represents the state of a thunk during evaluation
#[derive(Debug, Clone, PartialEq)]
pub enum ThunkState {
    /// Thunk has not been evaluated yet
    Suspended,
    /// Thunk is currently being evaluated (blackhole marker for infinite recursion detection)
    Evaluating,
    /// Thunk has been evaluated and the result is cached
    Evaluated,
}

/// A thunk represents a delayed computation in Nix
///
/// Thunks are the foundation of lazy evaluation. They store:
/// - The expression to evaluate (from the rnix AST)
/// - The lexical closure (variable scope) at the time of thunk creation
/// - The file_id context at thunk creation time (for relative imports)
/// - The evaluation state (suspended, evaluating, or evaluated with cached result)
///
/// # Example
///
/// ```no_run
/// use nix_eval::Thunk;
/// use nix_eval::VariableScope;
///
/// // Create a thunk for a simple expression
/// let scope = VariableScope::new();
/// // Note: In practice, you'd create this from an actual Expr node
/// ```
#[derive(Debug, Clone)]
pub struct Thunk {
    /// The expression to evaluate (stored as the syntax node)
    /// We store the syntax node text representation for now, as Expr has lifetime constraints
    expression_text: String,
    /// The lexical closure (variable scope) at thunk creation time
    pub(crate) closure: VariableScope,
    /// The file_id context at thunk creation time (for resolving relative imports)
    /// This is critical for lazy evaluation: when a thunk is forced, it needs to know
    /// what file it was created in so that relative imports work correctly.
    file_id: Option<FileId>,
    /// Absolute byte offset of the start of this thunk's expression within its source file.
    /// Used to translate spans from the re-parsed expression text back into file
    /// coordinates when reporting errors.
    span_start: usize,
    /// Absolute byte offset of the end of this thunk's expression within its source file.
    span_end: usize,
    /// The current state of the thunk
    state: Arc<Mutex<ThunkState>>,
    /// Cached result after evaluation (None if not yet evaluated)
    cached_value: Arc<Mutex<Option<NixValue>>>,
}

impl Thunk {
    /// Create a new suspended thunk
    ///
    /// # Arguments
    ///
    /// * `expr` - The expression to evaluate (from rnix AST)
    /// * `closure` - The lexical closure (variable scope) at thunk creation
    /// * `file_id` - The file ID at thunk creation (for relative imports)
    ///
    /// # Returns
    ///
    /// A new thunk in the Suspended state
    pub fn new(expr: &Expr, closure: VariableScope, file_id: Option<FileId>) -> Self {
        // Store the expression as text representation for now
        // In a full implementation, we'd want to store the actual AST node
        // but that requires handling lifetimes carefully
        let range = expr.syntax().text_range();
        let expression_text = expr.syntax().text().to_string();
        let base = thread_span_base();
        let start = base + usize::from(range.start());
        let end = base + usize::from(range.end());

        Self {
            expression_text,
            closure,
            file_id,
            span_start: start,
            span_end: end,
            state: Arc::new(Mutex::new(ThunkState::Suspended)),
            cached_value: Arc::new(Mutex::new(None)),
        }
    }

    /// Create a new thunk from expression text
    pub fn new_from_text(
        expression_text: String,
        closure: VariableScope,
        file_id: Option<FileId>,
    ) -> Self {
        // Synthetic expressions have no source location of their own; retain the
        // current span base so nested evaluation keeps correct file coordinates.
        let base = thread_span_base();
        Self {
            expression_text,
            closure,
            file_id,
            span_start: base,
            span_end: base,
            state: Arc::new(Mutex::new(ThunkState::Suspended)),
            cached_value: Arc::new(Mutex::new(None)),
        }
    }

    /// Absolute byte range of this thunk's expression within its source file
    pub fn span(&self) -> (usize, usize) {
        (self.span_start, self.span_end)
    }

    /// Get the expression text stored in this thunk
    ///
    /// This is a temporary solution. In a full implementation, we'd return
    /// the actual Expr AST node for evaluation.
    pub fn expression_text(&self) -> &str {
        &self.expression_text
    }

    /// Get a reference to the lexical closure
    pub fn closure(&self) -> &VariableScope {
        &self.closure
    }

    /// Get the current state of the thunk
    pub fn state(&self) -> ThunkState {
        self.state.lock().unwrap().clone()
    }

    /// Check if the thunk is suspended (not yet evaluated)
    pub fn is_suspended(&self) -> bool {
        matches!(self.state(), ThunkState::Suspended)
    }

    /// Check if the thunk is currently being evaluated
    pub fn is_evaluating(&self) -> bool {
        matches!(self.state(), ThunkState::Evaluating)
    }

    /// Check if the thunk has been evaluated
    pub fn is_evaluated(&self) -> bool {
        matches!(self.state(), ThunkState::Evaluated)
    }

    /// Force evaluation of the thunk
    ///
    /// This method evaluates the thunk's expression using the stored lexical closure.
    /// If the thunk has already been evaluated, it returns the cached result.
    /// If the thunk is currently being evaluated (blackhole), it returns an error.
    ///
    /// # Arguments
    ///
    /// * `evaluator` - The evaluator to use for evaluating the expression
    ///
    /// # Returns
    ///
    /// * `Ok(NixValue)` - The evaluated value
    /// * `Err(Error)` - An error if evaluation fails or if infinite recursion is detected
    ///
    /// # Example
    ///
    /// ```no_run
    /// use nix_eval::{Evaluator, Thunk};
    /// use rix_parser::ast::Expr;
    /// use std::collections::HashMap;
    ///
    /// let evaluator = Evaluator::new();
    /// // Create a thunk and force it
    /// // let thunk = Thunk::new(&expr, HashMap::new());
    /// // let value = thunk.force(&evaluator)?;
    /// ```
    pub fn force(&self, evaluator: &Evaluator) -> Result<NixValue> {
        // Check current state
        let mut state_guard = self.state.lock().unwrap();

        match *state_guard {
            ThunkState::Evaluated => {
                // Already evaluated - return cached value (memoization)
                // This is the fast path: if the thunk has been evaluated before,
                // we return the cached result without re-evaluating.
                drop(state_guard);
                let value_guard = self.cached_value.lock().unwrap();
                value_guard
                    .clone()
                    .ok_or_else(|| Error::UnsupportedExpression {
                        reason: "thunk marked as evaluated but no cached value found".to_string(),
                    })
            }
            ThunkState::Evaluating => {
                // Blackhole detected - infinite recursion
                // This occurs when a thunk tries to evaluate itself while already
                // being evaluated. The Evaluating state acts as a "blackhole" marker
                // that prevents stack overflow by detecting this condition early.
                drop(state_guard);
                Err(Error::InfiniteRecursion)
            }
            ThunkState::Suspended => {
                // Set blackhole marker before evaluation
                // This prevents infinite recursion: if this thunk tries to evaluate
                // itself (directly or indirectly) while we're evaluating it, we'll
                // detect the Evaluating state and return an error instead of
                // causing a stack overflow.
                *state_guard = ThunkState::Evaluating;
                drop(state_guard);

                // Parse the expression text back into an AST node
                let tokens = tokenize(&self.expression_text);
                let (green_node, errors) = parse(tokens.into_iter());

                if !errors.is_empty() {
                    let error_msgs: Vec<String> =
                        errors.iter().map(|e| format!("{:?}", e)).collect();
                    // Reset state on error
                    *self.state.lock().unwrap() = ThunkState::Suspended;
                    return Err(Error::ParseError {
                        reason: error_msgs.join(", "),
                    });
                }

                let syntax_node = SyntaxNode::new_root(green_node);
                let root = Root::cast(syntax_node).ok_or_else(|| {
                    // Reset state on error
                    *self.state.lock().unwrap() = ThunkState::Suspended;
                    Error::AstConversionError
                })?;

                let expr = root.expr().ok_or_else(|| {
                    // Reset state on error
                    *self.state.lock().unwrap() = ThunkState::Suspended;
                    Error::NoExpression
                })?;

                // Restore the file_id context when forcing the thunk
                // This is critical for relative imports within thunks to work correctly
                // Push context with the thunk's file_id and closure
                evaluator.push_context(self.file_id, self.closure.clone());

                // Translate spans produced while evaluating the re-parsed
                // expression text back into file coordinates.
                let prev_base = evaluator.span_base();
                evaluator.set_span_base(self.span_start);

                // Evaluate the expression using the thunk's closure as the scope
                // Note: For let bindings, if a variable is not found or is Null in the closure,
                // the identifier lookup in evaluate_expr_with_scope_impl will check the context stack
                let result = evaluator
                    .evaluate_expr_with_scope(&expr, &self.closure)
                    .map_err(|error| {
                        error.with_span(crate::error::Span::new(
                            self.file_id,
                            self.span_start,
                            self.span_end,
                        ))
                    });

                evaluator.set_span_base(prev_base);

                // Pop context (restore previous context)
                evaluator.pop_context();

                // Update state and cache result (memoization)
                // Once evaluated, the result is cached so subsequent calls to force()
                // will return the cached value without re-evaluation.
                match result {
                    Ok(value) => {
                        // Cache this indirection before forcing it.  The target may refer
                        // back to this thunk through a recursive scope; leaving this
                        // thunk blackholed until after the target is forced turns such
                        // valid aliases into false infinite-recursion errors.
                        if let NixValue::Thunk(inner) = &value {
                            {
                                let mut state_guard = self.state.lock().unwrap();
                                let mut value_guard = self.cached_value.lock().unwrap();
                                *state_guard = ThunkState::Evaluated;
                                *value_guard = Some(value.clone());
                            }

                            match inner.force(evaluator) {
                                Ok(final_value) => {
                                    *self.cached_value.lock().unwrap() = Some(final_value.clone());
                                    Ok(final_value)
                                }
                                Err(error) => {
                                    *self.state.lock().unwrap() = ThunkState::Suspended;
                                    *self.cached_value.lock().unwrap() = None;
                                    Err(error)
                                }
                            }
                        } else {
                            let mut state_guard = self.state.lock().unwrap();
                            let mut value_guard = self.cached_value.lock().unwrap();
                            *state_guard = ThunkState::Evaluated;
                            *value_guard = Some(value.clone());
                            Ok(value)
                        }
                    }
                    Err(e) => {
                        // Reset state on error so the thunk can be retried
                        *self.state.lock().unwrap() = ThunkState::Suspended;
                        Err(e)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thunk_creation() {
        // This test is a placeholder - we'll need actual Expr nodes from rnix
        // For now, we'll test the structure
        let _scope: VariableScope = VariableScope::new();
        // In a real test, we'd parse an expression and create a thunk
        // let expr = parse("42").unwrap();
        // let thunk = Thunk::new(&expr, scope);
        // assert!(thunk.is_suspended());
    }

    #[test]
    fn test_thunk_state() {
        let _scope: VariableScope = VariableScope::new();
        // Placeholder test - will be expanded when we have actual Expr nodes
    }
}
