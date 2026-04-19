//! Function expression evaluation

use crate::error::{Error, Result};
use crate::eval::Evaluator;
use crate::eval::context::VariableScope;
use crate::function;
use crate::value::NixValue;
use rix_parser::ast::Expr;
use rowan::ast::AstNode;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

impl Evaluator {
    pub(crate) fn evaluate_lambda(
        &self,
        lambda: &rix_parser::ast::Lambda,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        // Get the parameter from the lambda
        let param = lambda.param().ok_or_else(|| Error::UnsupportedExpression {
            reason: "lambda missing parameter".to_string(),
        })?;

        let parameter = match param {
            rix_parser::ast::Param::IdentParam(ident_param) => {
                let name = ident_param
                    .ident()
                    .map(|i| i.to_string())
                    .unwrap_or_default();
                crate::function::Parameter::Simple(name)
            }
            rix_parser::ast::Param::Pattern(pattern) => {
                let name = pattern.pat_bind().and_then(|b| b.ident()).map(|i| i.to_string());
                let entries = pattern
                    .pat_entries()
                    .filter_map(|e| e.ident())
                    .map(|i| i.to_string())
                    .collect();
                let ellipsis = pattern.ellipsis_token().is_some();
                crate::function::Parameter::Pattern {
                    name,
                    entries,
                    ellipsis,
                }
            }
        };

        // Get the body expression
        let body_expr = lambda.body().ok_or_else(|| Error::UnsupportedExpression {
            reason: "lambda missing body".to_string(),
        })?;

        // Create a function closure with the current scope
        let file_id = self.current_file_id();
        let func = function::Function::new(parameter, &body_expr, scope.clone(), file_id);

        Ok(NixValue::Function(Arc::new(func)))
    }

    /// Evaluate a function application expression
    pub(crate) fn evaluate_apply(
        &self,
        apply: &rix_parser::ast::Apply,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let func_expr = apply.lambda().ok_or_else(|| Error::UnsupportedExpression {
            reason: "function application missing function".to_string(),
        })?;

        let arg_expr = apply.argument().ok_or_else(|| Error::UnsupportedExpression {
            reason: "function application missing argument".to_string(),
        })?;

        // 1. Evaluate function and argument
        let func_value = self.evaluate_expr_with_scope_impl(&func_expr, scope)?;
        let arg_value = self.evaluate_expr_with_scope_impl(&arg_expr, scope)?;

        // 2. Force function expression to ensure it's a function or a builtin string
        let func_forced = func_value.clone().force(self)?;

        match func_forced {
            NixValue::Function(func) => {
                // Call the function
                func.apply(self, arg_value)
            }
            NixValue::String(s) if s.starts_with("__builtin_func:") => {
                let builtin_name = &s["__builtin_func:".len()..];
                if let Some(builtin) = self.builtins.get(builtin_name) {
                    match builtin.call_with_evaluator(&[arg_value], self) {
                        Ok(res) => Ok(res),
                        Err(Error::UnsupportedExpression { reason })
                            if reason.contains("takes") && reason.contains("arguments") =>
                        {
                            // Trigger currying
                            let mut closure = VariableScope::new();
                            closure.insert(
                                format!("__builtin_{}", builtin_name),
                                NixValue::String(s.clone()),
                            );
                            closure.insert("__curried_first_arg".to_string(), arg_value);
                            
                            let curried = crate::function::Function::new_curried_builtin_internal(
                                crate::function::Parameter::Simple(format!("__curried_{}_arg2", builtin_name)),
                                format!("__curried_builtin_call:{}", builtin_name),
                                closure,
                                self.current_file_id(),
                            );
                            Ok(NixValue::Function(std::sync::Arc::new(curried)))
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    Err(Error::UnsupportedExpression {
                        reason: format!("unknown builtin: {}", builtin_name),
                    })
                }
            }
            NixValue::AttributeSet(attrs) => {
                // Check for __functor
                if let Some(functor) = attrs.get("__functor") {
                    let functor_forced = functor.clone().force(self)?;
                    if let NixValue::Function(f) = functor_forced {
                        // Nix __functor calls are: functor self arg
                        let partially_applied = f.apply(self, NixValue::AttributeSet(attrs))?;
                        let partially_applied_forced = partially_applied.clone().force(self)?;
                        if let NixValue::Function(f2) = partially_applied_forced {
                            f2.apply(self, arg_value)
                        } else {
                            Err(Error::UnsupportedExpression {
                                reason: "__functor result is not a function after applying self".to_string(),
                            })
                        }
                    } else {
                        Err(Error::UnsupportedExpression {
                            reason: format!("__functor is not a function: {}", functor_forced),
                        })
                    }
                } else {
                    Err(Error::UnsupportedExpression {
                        reason: format!("cannot apply non-function value: {}", func_forced),
                    })
                }
            }
            _ => Err(Error::UnsupportedExpression {
                reason: format!("cannot apply non-function value: {}", func_forced),
            }),
        }
    }
}
