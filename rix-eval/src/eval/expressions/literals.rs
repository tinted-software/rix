//! Literal expression evaluation

use crate::error::{Error, Result};
use crate::eval::Evaluator;
use crate::eval::context::VariableScope;
use crate::value::NixValue;
use rix_parser::ast::{InterpolPart, Literal, Str};
use rowan::ast::AstNode;

impl Evaluator {
    pub(crate) fn evaluate_literal(&self, literal: &Literal) -> Result<NixValue> {
        let text = literal.to_string();

        // Remove quotes from string literals
        if text.starts_with('"') && text.ends_with('"') {
            // Basic string unescaping
            // Handle backslash-newline line continuation first (backslash followed by actual newline)
            // Then handle other escape sequences
            let mut unescaped = text[1..text.len() - 1].to_string();
            // Replace backslash followed by newline with just newline (line continuation)
            unescaped = unescaped.replace("\\\n", "\n");
            // Handle other escape sequences
            unescaped = unescaped
                .replace("\\n", "\n")
                .replace("\\t", "\t")
                .replace("\\\"", "\"")
                .replace("\\\\", "\\");
            return Ok(NixValue::String(unescaped));
        }

        // Check for boolean literals
        if text == "true" {
            return Ok(NixValue::Boolean(true));
        }
        if text == "false" {
            return Ok(NixValue::Boolean(false));
        }
        if text == "null" {
            return Ok(NixValue::Null);
        }

        // Try to parse as integer
        if let Ok(int_val) = text.parse::<i64>() {
            return Ok(NixValue::Integer(int_val));
        }

        // Try to parse as float
        if let Ok(float_val) = text.parse::<f64>() {
            return Ok(NixValue::Float(float_val));
        }

        Err(Error::UnsupportedLiteral { literal: text })
    }

    pub(crate) fn evaluate_string(
        &self,
        str_expr: &Str,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        // Check if this is an indented string (multiline string using '')
        // In rnix, indented strings are represented differently - check the syntax
        let mut result = String::new();

        for part in str_expr.normalized_parts() {
            match part {
                InterpolPart::Literal(unescaped) => {
                    result.push_str(&unescaped);
                }
                InterpolPart::Interpolation(interp) => {
                    // This is an interpolated expression - get the expression
                    if let Some(expr) = interp.expr() {
                        // Evaluate the interpolated expression and force thunks
                        let value = self.evaluate_expr_with_scope(&expr, scope)?;
                        let value_forced = value.force(self)?;

                        // Convert the value to a string
                        let value_str = match value_forced {
                            NixValue::String(s) => s,
                            NixValue::Integer(i) => i.to_string(),
                            NixValue::Float(f) => f.to_string(),
                            NixValue::Boolean(b) => b.to_string(),
                            NixValue::Null => "".to_string(),
                            NixValue::Path(p) => p.display().to_string(),
                            NixValue::StorePath(p) => p.clone(),
                            NixValue::Derivation(drv) => format!("<derivation {}>", drv.name),
                            NixValue::List(_)
                            | NixValue::AttributeSet(_)
                            | NixValue::Thunk(_)
                            | NixValue::DeferredLookup(_, _)
                            | NixValue::DeferredInherit(_, _)
                            | NixValue::Function(_)
                            | NixValue::Builtin(_) => {
                                // For complex types, use their Display implementation
                                format!("{}", value_forced)
                            }
                        };

                        result.push_str(&value_str);
                    } else {
                        return Err(Error::UnsupportedExpression {
                            reason: "interpolation missing expression".to_string(),
                        });
                    }
                }
            }
        }

        Ok(NixValue::String(result))
    }
}
