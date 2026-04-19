//! Function expression evaluation

use crate::error::{Error, Result};
use crate::eval::Evaluator;
use crate::eval::context::VariableScope;
use crate::function;
use crate::value::NixValue;
use rowan::ast::AstNode;
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
                let name = pattern
                    .pat_bind()
                    .and_then(|b| b.ident())
                    .map(|i| i.to_string());
                let entries = pattern
                    .pat_entries()
                    .map(|e| {
                        let name = e.ident().map(|i| i.to_string()).unwrap_or_default();
                        let default = e.default().map(|d| d.syntax().text().to_string());
                        (name, default)
                    })
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

        let arg_expr = apply
            .argument()
            .ok_or_else(|| Error::UnsupportedExpression {
                reason: "function application missing argument".to_string(),
            })?;

        // 1. Evaluate function
        let func_value = self.evaluate_expr_with_scope_impl(&func_expr, scope)?;

        // 2. Create a thunk for the argument to ensure lazy evaluation
        let file_id = self.current_file_id();
        let arg_value = NixValue::Thunk(Arc::new(crate::thunk::Thunk::new(
            &arg_expr,
            scope.clone(),
            file_id,
        )));

        // 3. Use NixValue::apply helper
        func_value.apply(self, arg_value)
    }
}
