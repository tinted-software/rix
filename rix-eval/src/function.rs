//! Function/closure implementation for Nix functions
//!
//! Functions in Nix are closures that capture their lexical environment (scope)
//! and can be applied to arguments. This module provides the data structure
//! to represent Nix functions.

use crate::{Error, Evaluator, NixValue, Result, VariableScope};
use codespan::FileId;
use rix_parser::SyntaxNode;
use rix_parser::ast::{Expr, Root};
use rix_parser::parser::parse;
use rix_parser::tokenizer::tokenize;
use rowan::ast::AstNode;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// A Nix function (closure)
///
/// Functions in Nix are closures that:
/// - Capture their lexical environment (scope) at definition time
/// - Have a parameter name (or pattern) that will be bound when applied
/// - Have a body expression that will be evaluated when the function is called
/// - Capture the file_id context at definition time (for relative imports)
///
/// # Example
///
/// ```no_run
/// use nix_eval::function::Function;
/// use nix_eval::{Evaluator, NixValue};
/// use std::collections::HashMap;
///
/// // A function like `x: x + 1` would be represented as:
/// // - parameter: "x"
/// // - body: "x + 1"
/// // Note: In practice, you'd create this from an actual Expr node
/// ```
#[derive(Debug, Clone)]
pub enum Parameter {
    Simple(String),
    Pattern {
        name: Option<String>,
        entries: Vec<(String, Option<String>)>, // Name and optional default expression text
        ellipsis: bool,
    },
}

impl std::fmt::Display for Parameter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Parameter::Simple(name) => write!(f, "{}", name),
            Parameter::Pattern {
                name,
                entries,
                ellipsis,
            } => {
                let mut first = true;
                if let Some(n) = name {
                    write!(f, "{} @ ", n)?;
                }
                write!(f, "{{ ")?;
                for (entry, default) in entries {
                    if !first {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", entry)?;
                    if let Some(def) = default {
                        write!(f, " ? {}", def)?;
                    }
                    first = false;
                }
                if *ellipsis {
                    if !first {
                        write!(f, ", ")?;
                    }
                    write!(f, "...")?;
                }
                write!(f, " }}")
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Function {
    /// The parameter name (or pattern) that will be bound when the function is applied
    pub parameter: Parameter,
    /// The body expression (stored as text representation)
    ///
    /// Similar to thunks, we store the expression as text for now.
    /// In a full implementation, we'd want to store the actual AST node.
    pub(crate) body_text: String,
    /// The lexical closure (variable scope) captured at function definition time
    ///
    /// This allows the function to access variables from its surrounding scope
    /// even when called in a different context (lexical scoping).
    pub(crate) closure: VariableScope,
    /// The file_id context at function definition time (for resolving relative imports)
    ///
    /// This is critical for lazy evaluation: when a function is called and creates thunks,
    /// those thunks need to know what file the function was defined in so that relative
    /// imports work correctly.
    pub(crate) file_id: Option<FileId>,
}

impl Function {
    /// Create a new function closure
    ///
    /// # Arguments
    ///
    /// * `parameter` - The parameter name (or pattern) for this function
    /// * `body_expr` - The body expression (from rnix AST)
    /// * `closure` - The lexical closure (variable scope) at function definition time
    /// * `file_id` - The file ID at function definition time (for relative imports)
    ///
    /// # Returns
    ///
    /// A new function closure
    pub fn new(
        parameter: Parameter,
        body_expr: &Expr,
        closure: VariableScope,
        file_id: Option<FileId>,
    ) -> Self {
        // Store the body expression as text representation for now
        // In a full implementation, we'd want to store the actual AST node
        // but that requires handling lifetimes carefully
        let body_text = body_expr.syntax().text().to_string();

        Self {
            parameter,
            body_text,
            closure,
            file_id,
        }
    }

    /// Create a curried builtin function (internal constructor)
    pub(crate) fn new_curried_builtin_internal(
        parameter: Parameter,
        body_text: String,
        closure: VariableScope,
        file_id: Option<FileId>,
    ) -> Self {
        Self {
            parameter,
            body_text,
            closure,
            file_id,
        }
    }

    /// Get the parameter info
    pub fn parameter(&self) -> &Parameter {
        &self.parameter
    }

    /// Get the body expression text
    pub fn body_text(&self) -> &str {
        &self.body_text
    }

    /// Create a curried builtin function
    ///
    /// This creates a function that wraps a builtin with a partially applied argument.
    /// When the function is called, it will call the builtin with both the captured
    /// argument and the new argument.
    ///
    /// # Arguments
    ///
    /// * `builtin_name` - The name of the builtin (for debugging)
    /// * `builtin` - The builtin to wrap
    /// * `first_arg` - The first argument (already applied)
    /// * `file_id` - The file ID context
    ///
    /// # Returns
    ///
    /// A new function that, when called, will apply the builtin to (first_arg, new_arg)
    pub fn new_curried_builtin(
        builtin_name: String,
        _builtin: Box<dyn crate::Builtin>,
        first_arg: NixValue,
        file_id: Option<FileId>,
    ) -> Self {
        // Create a function that captures the first argument
        // When applied, it will call the builtin with (first_arg, new_arg)
        // We'll use a special parameter name to indicate this is a curried builtin
        let parameter = Parameter::Simple(format!("__curried_{}_arg2", builtin_name));
        let body_text = format!("__curried_builtin_call:{}", builtin_name);

        // Store the builtin and first arg in the closure
        let mut closure = VariableScope::new();
        closure.insert(
            format!("__builtin_{}", builtin_name),
            NixValue::Builtin(builtin_name.clone()),
        );
        closure.insert("__curried_first_arg".to_string(), first_arg);

        Self {
            parameter,
            body_text,
            closure,
            file_id,
        }
    }

    /// Create a curried builtin function with multiple arguments already applied
    ///
    /// This creates a function that captures multiple arguments and waits for more.
    pub fn new_curried_builtin_multi(
        builtin_name: String,
        _builtin: Box<dyn crate::Builtin>,
        args: Vec<NixValue>,
        file_id: Option<FileId>,
    ) -> Self {
        let parameter =
            Parameter::Simple(format!("__curried_{}_arg{}", builtin_name, args.len() + 1));
        let _body_text = format!("__curried_builtin_call:{}", builtin_name);

        let mut closure = VariableScope::new();
        closure.insert(
            format!("__builtin_{}", builtin_name),
            NixValue::String(format!("__builtin_func:{}", builtin_name)),
        );
        for (i, arg) in args.iter().enumerate() {
            closure.insert(format!("__curried_arg{}", i + 1), arg.clone());
        }
        closure.insert(
            "__curried_arg_count".to_string(),
            NixValue::Integer(args.len() as i64),
        );

        // Create a dummy Expr for the body (we won't actually use it for curried builtins)
        // We'll use an empty expression since the body_text is just a marker
        use rix_parser::parser::parse;
        use rix_parser::tokenizer::tokenize;
        let tokens = tokenize("null");
        let (green_node, _) = parse(tokens.into_iter());
        let syntax_node = SyntaxNode::new_root(green_node);
        let root = Root::cast(syntax_node).unwrap();
        let dummy_expr = root.expr().unwrap();

        Self::new(parameter, &dummy_expr, closure, file_id)
    }

    /// Create a curried foldl' function (2 args applied, needs list)
    ///
    /// This creates a function that captures op and nul, and when called with a list,
    /// will call foldl' with all three arguments.
    ///
    /// # Arguments
    ///
    /// * `op` - The operator function (first argument to foldl')
    /// * `nul` - The initial accumulator (second argument to foldl')
    /// * `file_id` - The file ID context
    ///
    /// # Returns
    ///
    /// A new function that, when called with a list, will call foldl' with (op, nul, list)
    pub fn new_curried_foldl(op: NixValue, nul: NixValue, file_id: Option<FileId>) -> Self {
        // Create a function that captures op and nul
        // When applied with a list, it will call foldl' with (op, nul, list)
        let parameter = Parameter::Simple("__foldl_list_arg".to_string());
        let body_text = "__curried_foldl_call".to_string();

        // Store op and nul in the closure
        let mut closure = VariableScope::new();
        closure.insert("__foldl_op".to_string(), op);
        closure.insert("__foldl_nul".to_string(), nul);

        Self {
            parameter,
            body_text,
            closure,
            file_id,
        }
    }

    /// Get a reference to the lexical closure
    pub fn closure(&self) -> &VariableScope {
        &self.closure
    }

    /// Apply this function to an argument
    ///
    /// This method evaluates the function body with the argument bound to the parameter.
    /// The function's closure is merged with the argument binding, allowing the body
    /// to access both the captured closure variables and the function parameter.
    ///
    /// **Currying Support**: If the function body evaluates to another function,
    /// that function is returned (partial application). This enables currying:
    /// `(x: y: x + y) 1` returns `y: 1 + y`, and `(x: y: x + y) 1 2` evaluates to `3`.
    ///
    /// # Arguments
    ///
    /// * `evaluator` - The evaluator to use for evaluating the function body
    /// * `argument` - The argument value to bind to the function parameter
    ///
    /// # Returns
    ///
    /// * `Ok(NixValue)` - The result of evaluating the function body (may be a function for currying)
    /// * `Err(Error)` - An error if evaluation fails
    ///
    /// # Example
    ///
    /// ```no_run
    /// use nix_eval::{Evaluator, Function, NixValue};
    /// use rix_parser::ast::Expr;
    /// use std::collections::HashMap;
    ///
    /// let evaluator = Evaluator::new();
    /// // Create a curried function: x: y: x + y
    /// // let func = Function::new("x", &body_expr, HashMap::new());
    /// // let partial = func.apply(&evaluator, NixValue::Integer(1))?; // Returns y: 1 + y
    /// // let result = partial.apply(&evaluator, NixValue::Integer(2))?; // Returns 3
    /// ```
    pub fn apply(&self, evaluator: &Evaluator, argument: NixValue) -> Result<NixValue> {
        // Check if this is a curried foldl' function (2 args applied, needs list)
        if self.body_text == "__curried_foldl_call" {
            if let (Some(op), Some(nul)) = (
                self.closure.get("__foldl_op"),
                self.closure.get("__foldl_nul"),
            ) {
                // This is a curried foldl' - call it with (op, nul, list)
                // Force the list argument
                let list_value = argument.clone().force(evaluator)?;
                let list = match list_value {
                    NixValue::List(l) => l,
                    _ => {
                        return Err(Error::UnsupportedExpression {
                            reason: format!(
                                "foldl': third argument must be a list, got {}",
                                list_value
                            ),
                        });
                    }
                };

                // Get the operator function or builtin name
                let op_value = op.clone().force(evaluator)?;
                let (op_func_opt, builtin_name_opt) = match op_value {
                    NixValue::Function(f) => (Some(f), None),
                    NixValue::Builtin(ref name) => {
                        let builtin_name = name;
                        if evaluator.get_builtin(builtin_name).is_some() {
                            (None, Some(builtin_name.to_string()))
                        } else {
                            return Err(Error::UnsupportedExpression {
                                reason: format!(
                                    "foldl': unknown builtin function: {}",
                                    builtin_name
                                ),
                            });
                        }
                    }
                    _ => {
                        return Err(Error::UnsupportedExpression {
                            reason: format!(
                                "foldl': first argument must be a function, got {}",
                                op_value
                            ),
                        });
                    }
                };

                // Fold left: start with nul, apply op to accumulator and each element
                let mut accumulator = nul.clone();
                for element in list {
                    if let Some(ref builtin_name) = builtin_name_opt {
                        // Handle builtin directly
                        if let Some(builtin) = evaluator.get_builtin(builtin_name) {
                            let accumulator_forced = accumulator.clone().force(evaluator)?;
                            let element_forced = element.clone().force(evaluator)?;
                            accumulator = builtin.call_with_evaluator(
                                &[accumulator_forced, element_forced],
                                evaluator,
                            )?;
                        } else {
                            return Err(Error::UnsupportedExpression {
                                reason: format!("foldl': builtin '{}' not found", builtin_name),
                            });
                        }
                    } else if let Some(ref op_func) = op_func_opt {
                        // Handle Nix function - foldl' calls op(acc, elem)
                        let accumulator_forced = accumulator.clone().force(evaluator)?;
                        let element_forced = element.clone().force(evaluator)?;
                        let partial = op_func.apply(evaluator, accumulator_forced)?;
                        accumulator = match partial {
                            NixValue::Function(next_func) => {
                                next_func.apply(evaluator, element_forced)?
                            }
                            _ => {
                                return Err(Error::UnsupportedExpression {
                                    reason: format!(
                                        "foldl': operator function must be curried (take 2 args), got {}",
                                        partial
                                    ),
                                });
                            }
                        };
                    }
                }

                return Ok(accumulator);
            }
        }

        // Check if this is a curried builtin function
        if self.body_text.starts_with("__curried_builtin_call:") {
            let builtin_name = &self.body_text[23..]; // Skip "__curried_builtin_call:"

            // Get the builtin and collected arguments from closure
            if let Some(builtin_marker) = self.closure.get(&format!("__builtin_{}", builtin_name)) {
                if let NixValue::Builtin(_) = builtin_marker {
                    if let Some(builtin) = evaluator.get_builtin(builtin_name) {
                        // Collect all arguments from closure
                        let mut args = Vec::new();

                        // Check if we have __curried_first_arg (old style) or __curried_arg1, __curried_arg2, etc. (new style)
                        if let Some(first_arg) = self.closure.get("__curried_first_arg") {
                            // Old style: single argument - force thunks before collecting
                            let first_arg_forced = first_arg.clone().force(evaluator)?;
                            args.push(first_arg_forced);
                            let arg_forced = argument.clone().force(evaluator)?;
                            args.push(arg_forced);
                        } else {
                            // New style: multiple arguments - force thunks before collecting
                            let arg_count = self
                                .closure
                                .get("__curried_arg_count")
                                .and_then(|v| match v {
                                    NixValue::Integer(n) => Some(n as usize),
                                    _ => None,
                                })
                                .unwrap_or(0);

                            for i in 1..=arg_count {
                                if let Some(arg) = self.closure.get(&format!("__curried_arg{}", i))
                                {
                                    let arg_forced = arg.clone().force(evaluator)?;
                                    args.push(arg_forced);
                                }
                            }
                            let arg_forced = argument.clone().force(evaluator)?;
                            args.push(arg_forced);
                        }

                        match builtin.call_with_evaluator(&args, evaluator) {
                            Ok(result) => return Ok(result),
                            Err(Error::UnsupportedExpression { reason })
                                if reason.contains("takes") && reason.contains("arguments") =>
                            {
                                // Still needs more arguments - create another curried function
                                let file_id = evaluator.current_file_id();
                                let mut closure = VariableScope::new();
                                closure.insert(
                                    format!("__builtin_{}", builtin_name),
                                    NixValue::Builtin(builtin_name.to_string()),
                                );
                                for (i, arg) in args.iter().enumerate() {
                                    closure.insert(format!("__curried_arg{}", i + 1), arg.clone());
                                }
                                closure.insert(
                                    "__curried_arg_count".to_string(),
                                    NixValue::Integer(args.len() as i64),
                                );

                                let next_curried = Function::new_curried_builtin_internal(
                                    Parameter::Simple(format!(
                                        "__curried_{}_arg{}",
                                        builtin_name,
                                        args.len() + 1
                                    )),
                                    format!("__curried_builtin_call:{}", builtin_name),
                                    closure,
                                    file_id,
                                );
                                return Ok(NixValue::Function(Arc::new(next_curried)));
                            }
                            Err(e) => return Err(e),
                        }
                    }
                }
            }
        }

        // Create a new scope that merges the closure with the argument binding
        // The parameter shadows any variable with the same name in the closure
        let mut scope = self.closure.clone();
        scope.push_lexical();
        match &self.parameter {
            Parameter::Simple(name) => {
                scope.insert(name.clone(), argument);
            }
            Parameter::Pattern {
                name,
                entries,
                ellipsis,
            } => {
                // If it's a pattern, we need to bind the entries
                let arg_forced = argument.clone().force(evaluator)?;
                let attrs = match arg_forced {
                    NixValue::AttributeSet(a) => a,
                    _ => {
                        return Err(Error::UnsupportedExpression {
                            reason: format!(
                                "function expected attribute set as argument, got {}",
                                arg_forced
                            ),
                        });
                    }
                };

                // Create a shared recursive map for the arguments
                // This allows default expressions to refer to other arguments in the same pattern
                let mut arg_bindings = HashMap::new();
                let shared_arg_map = Arc::new(Mutex::new(HashMap::new()));

                // Create a recursive scope that includes these arguments
                let mut rec_scope = self.closure.clone();
                rec_scope.push_recursive(shared_arg_map.clone());

                // Handle the @ name if it exists
                if let Some(n) = name {
                    arg_bindings.insert(n.clone(), argument.clone());
                    shared_arg_map
                        .lock()
                        .unwrap()
                        .insert(n.clone(), argument.clone());
                }

                for (entry_name, default_text) in entries {
                    if let Some(val) = attrs.get(entry_name) {
                        arg_bindings.insert(entry_name.clone(), val.clone());
                    } else if let Some(text) = default_text {
                        // Use default value (evaluated lazily in the recursive scope)
                        let thunk = crate::thunk::Thunk::new_from_text(
                            text.clone(),
                            rec_scope.clone(),
                            self.file_id,
                        );
                        arg_bindings.insert(entry_name.clone(), NixValue::Thunk(Arc::new(thunk)));
                    } else if !ellipsis {
                        // Missing required argument and no ellipsis
                        return Err(Error::UnsupportedExpression {
                            reason: format!(
                                "function expected argument '{}' but it was not provided",
                                entry_name
                            ),
                        });
                    }
                }

                // Check for unexpected arguments if no ellipsis
                if !ellipsis {
                    for entry_name in attrs.keys() {
                        if !entries.iter().any(|(n, _)| n == entry_name) {
                            return Err(Error::UnsupportedExpression {
                                reason: format!(
                                    "function called with unexpected argument '{}'",
                                    entry_name
                                ),
                            });
                        }
                    }
                }

                // Update shared_arg_map for recursion and merge into scope
                {
                    let mut map = shared_arg_map.lock().unwrap();
                    for (k, v) in &arg_bindings {
                        map.insert(k.clone(), v.clone());
                        scope.insert(k.clone(), v.clone());
                    }
                }

                if let Some(name) = name {
                    scope.insert(name.clone(), argument);
                }
            }
        }

        // Parse the body expression text back into an AST node
        let tokens = tokenize(&self.body_text);
        let (green_node, errors) = parse(tokens.into_iter());

        if !errors.is_empty() {
            let error_msgs: Vec<String> = errors.iter().map(|e| format!("{:?}", e)).collect();
            return Err(Error::ParseError {
                reason: error_msgs.join(", "),
            });
        }

        let syntax_node = SyntaxNode::new_root(green_node);
        let root = Root::cast(syntax_node).ok_or(Error::AstConversionError)?;

        let body_expr = root.expr().ok_or(Error::NoExpression)?;

        // Restore the file_id context when calling the function
        // This is critical for relative imports within function bodies to work correctly
        // Push context with the function's file_id
        evaluator.push_context(self.file_id, scope.clone());

        // Evaluate the body expression using the merged scope
        let result = evaluator.evaluate_expr_with_scope(&body_expr, &scope);

        // Pop context (restore previous context)
        evaluator.pop_context();

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_creation() {
        // This test is a placeholder - we'll need actual Expr nodes from rnix
        // For now, we'll test the structure
        let _scope = VariableScope::new();
        // In a real test, we'd parse an expression and create a function
        // let body_expr = parse("x + 1").unwrap();
        // let func = Function::new("x", &body_expr, scope);
        // assert_eq!(func.parameter(), "x");
    }

    #[test]
    fn test_function_apply() {
        // Placeholder test - will be expanded when we have actual Expr nodes
    }
}
