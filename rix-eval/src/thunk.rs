//! Thunk implementation for lazy evaluation
//!
//! A thunk represents a delayed computation in Nix. It stores the expression
//! to evaluate and its lexical closure (scope), allowing lazy evaluation
//! where expressions are only evaluated when their values are needed.

use crate::{Error, Evaluator, NixValue, Result, VariableScope};
use codespan::FileId;
use parking_lot::{Condvar, Mutex};
use rix_parser::SyntaxNode;
use rix_parser::ast::{Expr, Root};
use rix_parser::parser::parse;
use rix_parser::tokenizer::tokenize;
use rowan::ast::AstNode;
use std::sync::Arc;
use std::thread::ThreadId;

/// Internal representation of a thunk's evaluation state
#[derive(Debug, Clone)]
enum InternalThunkState {
    /// Thunk has not been evaluated yet
    Suspended,
    /// Thunk is currently being evaluated by the thread with the given ThreadId
    Pending(ThreadId),
    /// Thunk is being evaluated by another thread and other threads are awaiting completion
    Awaited(ThreadId),
    /// Thunk has been successfully evaluated
    Evaluated,
    /// Thunk evaluation failed with an error
    Failed(Error),
}

/// Represents the public state of a thunk
#[derive(Debug, Clone, PartialEq)]
pub enum ThunkState {
    /// Thunk has not been evaluated yet
    Suspended,
    /// Thunk is currently being evaluated (by any thread)
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
    /// The current state of the thunk
    state: Arc<Mutex<InternalThunkState>>,
    /// Condvar for threads waiting on thunk evaluation
    condvar: Arc<Condvar>,
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
        let expression_text = expr.syntax().text().to_string();

        Self {
            expression_text,
            closure,
            file_id,
            state: Arc::new(Mutex::new(InternalThunkState::Suspended)),
            condvar: Arc::new(Condvar::new()),
            cached_value: Arc::new(Mutex::new(None)),
        }
    }

    /// Create a new thunk from expression text
    pub fn new_from_text(
        expression_text: String,
        closure: VariableScope,
        file_id: Option<FileId>,
    ) -> Self {
        Self {
            expression_text,
            closure,
            file_id,
            state: Arc::new(Mutex::new(InternalThunkState::Suspended)),
            condvar: Arc::new(Condvar::new()),
            cached_value: Arc::new(Mutex::new(None)),
        }
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
        match *self.state.lock() {
            InternalThunkState::Suspended => ThunkState::Suspended,
            InternalThunkState::Pending(_) | InternalThunkState::Awaited(_) => {
                ThunkState::Evaluating
            }
            InternalThunkState::Evaluated | InternalThunkState::Failed(_) => ThunkState::Evaluated,
        }
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
        let current_thread = std::thread::current().id();
        let mut state_guard = self.state.lock();

        loop {
            match &*state_guard {
                InternalThunkState::Evaluated => {
                    drop(state_guard);
                    let value_guard = self.cached_value.lock();
                    return value_guard
                        .clone()
                        .ok_or_else(|| Error::UnsupportedExpression {
                            reason: "thunk marked as evaluated but no cached value found"
                                .to_string(),
                        });
                }
                InternalThunkState::Failed(err) => {
                    return Err(err.clone());
                }
                InternalThunkState::Pending(owner) | InternalThunkState::Awaited(owner) => {
                    if *owner == current_thread {
                        // Same thread re-entered the thunk: infinite recursion (blackhole)
                        drop(state_guard);
                        return Err(Error::InfiniteRecursion);
                    }
                    // Different thread is evaluating: mark as Awaited and wait on Condvar
                    *state_guard = InternalThunkState::Awaited(*owner);
                    self.condvar.wait(&mut state_guard);
                    // Loop again to check if Evaluated or Failed
                }
                InternalThunkState::Suspended => {
                    // Mark as Pending with current thread as owner
                    *state_guard = InternalThunkState::Pending(current_thread);
                    drop(state_guard);

                    // Evaluate the thunk expression
                    let eval_res = self.evaluate_internal(evaluator);

                    let mut state_guard = self.state.lock();
                    let was_awaited = matches!(*state_guard, InternalThunkState::Awaited(_));

                    match eval_res {
                        Ok(final_value) => {
                            *self.cached_value.lock() = Some(final_value.clone());
                            *state_guard = InternalThunkState::Evaluated;
                            if was_awaited {
                                self.condvar.notify_all();
                            }
                            return Ok(final_value);
                        }
                        Err(err) => {
                            *state_guard = InternalThunkState::Failed(err.clone());
                            if was_awaited {
                                self.condvar.notify_all();
                            }
                            return Err(err);
                        }
                    }
                }
            }
        }
    }

    fn evaluate_internal(&self, evaluator: &Evaluator) -> Result<NixValue> {
        // Parse the expression text back into an AST node
        let tokens = tokenize(&self.expression_text);
        let (green_node, errors) = parse(tokens.into_iter());

        if !errors.is_empty() {
            let error_msgs: Vec<String> = errors.iter().map(|e| format!("{:?}", e)).collect();
            return Err(Error::ParseError {
                reason: error_msgs.join(", "),
            });
        }

        let syntax_node = SyntaxNode::new_root(green_node);
        let root = Root::cast(syntax_node).ok_or(Error::AstConversionError)?;
        let expr = root.expr().ok_or(Error::NoExpression)?;

        // Restore the file_id context when forcing the thunk
        evaluator.push_context(self.file_id, self.closure.clone());

        // Evaluate the expression using the thunk's closure as the scope
        let result = evaluator.evaluate_expr_with_scope(&expr, &self.closure);

        // Pop context (restore previous context)
        evaluator.pop_context();

        let value = result?;
        // If the result is itself a thunk, force it recursively.
        // This detects infinite recursion (blackhole) when a thunk evaluates to itself.
        if let NixValue::Thunk(inner) = &value {
            inner.force(evaluator)
        } else {
            Ok(value)
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
