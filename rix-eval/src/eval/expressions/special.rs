//! Special form expression evaluation

use crate::error::{Error, Result};
use crate::eval::Evaluator;
use crate::eval::context::VariableScope;
use crate::thunk;
use crate::value::NixValue;
use rix_parser::ast::{HasEntry, Inherit, Paren};
use rowan::ast::AstNode;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

impl Evaluator {
    pub(crate) fn evaluate_let_in(
        &self,
        let_in: &rix_parser::ast::LetIn,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        // Collect all bindings and their expressions
        let mut binding_exprs = Vec::new();
        let mut inherit_bindings = Vec::new();
        let file_id = self.current_file_id();

        // Support for mutual recursion: all bindings share a mutable map
        let shared_rec_map = std::sync::Arc::new(std::sync::Mutex::new(HashMap::new()));
        let mut rec_scope = scope.clone();
        rec_scope.set_recursive(shared_rec_map.clone());

        // Process inherits first
        for inherit_node in let_in.inherits() {
            let inherit_from = inherit_node.from();
            let inherit_scope = if let Some(from_node) = &inherit_from {
                if let Some(from_expr) = from_node.expr() {
                    // Evaluate 'from' expression in the recursive scope
                    let from_value = self.evaluate_expr_with_scope(&from_expr, &rec_scope)?;
                    match from_value.force(self)? {
                        NixValue::AttributeSet(attrs) => attrs,
                        _ => return Err(Error::UnsupportedExpression {
                            reason: "inherit from expression must be an attribute set".to_string(),
                        }),
                    }
                } else {
                    rec_scope.vars().clone()
                }
            } else {
                rec_scope.vars().clone()
            };

            for attr in inherit_node.attrs() {
                let key = if let Some(ident) = rix_parser::ast::Ident::cast(attr.syntax().clone()) {
                    ident.to_string()
                } else {
                    attr.to_string().trim_matches('"').to_string()
                };

                if let Some(value) = inherit_scope.get(&key) {
                    inherit_bindings.push((key, value.clone()));
                } else if inherit_from.is_none() {
                    // Check parent recursive scope too for inherit if no 'from'
                    if let Some(value) = rec_scope.get(&key) {
                        inherit_bindings.push((key, value));
                    } else {
                        return Err(Error::UnsupportedExpression {
                            reason: format!("inherit: attribute '{}' not found in scope", key),
                        });
                    }
                } else {
                    return Err(Error::UnsupportedExpression {
                        reason: format!("inherit: attribute '{}' not found in scope", key),
                    });
                }
            }
        }

        // Process attribute values
        for binding in let_in.attrpath_values() {
            let attrpath = binding.attrpath().ok_or_else(|| Error::UnsupportedExpression {
                reason: "let binding missing attrpath".to_string(),
            })?;
            
            let value_expr = binding.value().ok_or_else(|| Error::UnsupportedExpression {
                reason: "let binding missing value".to_string(),
            })?;

            // Resolve the attribute path (names can be dynamic)
            let mut path_components = Vec::new();
            for attr in attrpath.attrs() {
                if let Some(ident) = rix_parser::ast::Ident::cast(attr.syntax().clone()) {
                    path_components.push(ident.to_string());
                } else if let Some(str_node) = rix_parser::ast::Str::cast(attr.syntax().clone()) {
                    // For let bindings, dynamic names are evaluated in the recursive scope
                    let str_value = self.evaluate_string(&str_node, &rec_scope)?;
                    if let NixValue::String(s) = str_value {
                        path_components.push(s);
                    } else {
                        path_components.push(attr.to_string().trim_matches('"').to_string());
                    }
                } else {
                    path_components.push(attr.to_string().trim_matches('"').to_string());
                }
            }

            if path_components.is_empty() { continue; }
            binding_exprs.push((path_components, value_expr));
        }

        // New scope for the body (lexical)
        let mut new_scope = scope.clone();

        // Add inheritance to shared map and lexical scope
        for (key, value) in inherit_bindings {
            shared_rec_map.lock().unwrap().insert(key.clone(), value.clone());
            new_scope.insert(key, value);
        }

        // Group regular and nested bindings
        let mut grouped_nested: HashMap<String, Vec<(Vec<String>, rix_parser::ast::Expr)>> = HashMap::new();
        
        for (path, expr) in binding_exprs {
            if path.len() == 1 {
                let var_name = path[0].clone();
                let thunk = thunk::Thunk::new(&expr, rec_scope.clone(), file_id);
                let thunk_value = NixValue::Thunk(Arc::new(thunk));
                shared_rec_map.lock().unwrap().insert(var_name.clone(), thunk_value.clone());
                new_scope.insert(var_name, thunk_value);
            } else {
                let first = path[0].clone();
                grouped_nested.entry(first).or_default().push((path[1..].to_vec(), expr));
            }
        }

        // Process nested bindings (e.g., let a.b = 1; in ...)
        for (var_name, nested) in grouped_nested {
            let mut nested_map = HashMap::new();
            for (subpath, expr) in nested {
                // If subpath has more levels, we should ideally handle it recursively.
                // For now, handle 1 level of nesting: let a.b = 1; -> a = { b = 1; }
                let thunk = thunk::Thunk::new(&expr, rec_scope.clone(), file_id);
                nested_map.insert(subpath[0].clone(), NixValue::Thunk(Arc::new(thunk)));
            }
            
            let set_value = NixValue::AttributeSet(nested_map);
            shared_rec_map.lock().unwrap().insert(var_name.clone(), set_value.clone());
            new_scope.insert(var_name, set_value);
        }

        // Final result: evaluate body or handle legacy let
        if let Some(body_expr) = let_in.body() {
            self.evaluate_expr_with_scope(&body_expr, &new_scope)
        } else {
            // Legacy let handling (where result is from 'body' attribute)
            if let Some(body_val) = new_scope.get("body") {
                body_val.force(self)
            } else {
                Err(Error::UnsupportedExpression {
                    reason: "legacy let missing 'body' attribute".to_string(),
                })
            }
        }
    }

    pub(crate) fn evaluate_legacy_let(
        &self,
        legacy_let: &rix_parser::ast::LegacyLet,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let mut new_scope = scope.clone();
        let file_id = self.current_file_id();
        let shared_rec_map = std::sync::Arc::new(std::sync::Mutex::new(HashMap::new()));
        
        let mut rec_scope = scope.clone();
        rec_scope.set_recursive(shared_rec_map.clone());

        for binding in legacy_let.attrpath_values() {
            let attrpath = binding.attrpath().ok_or_else(|| Error::UnsupportedExpression {
                reason: "legacy let binding missing attrpath".to_string(),
            })?;
            
            let value_expr = binding.value().ok_or_else(|| Error::UnsupportedExpression {
                reason: "legacy let binding missing value".to_string(),
            })?;

            let var_name = attrpath.attrs().next().map(|attr| {
                if let Some(ident) = rix_parser::ast::Ident::cast(attr.syntax().clone()) {
                    ident.to_string()
                } else {
                    attr.to_string().trim_matches('"').to_string()
                }
            }).ok_or_else(|| Error::UnsupportedExpression {
                reason: "legacy let binding missing variable name".to_string(),
            })?;

            let thunk = thunk::Thunk::new(&value_expr, rec_scope.clone(), file_id);
            let thunk_value = NixValue::Thunk(Arc::new(thunk));

            shared_rec_map.lock().unwrap().insert(var_name.clone(), thunk_value.clone());
            new_scope.insert(var_name, thunk_value);
        }

        if let Some(body_val) = new_scope.get("body") {
            body_val.force(self)
        } else {
            Err(Error::UnsupportedExpression {
                reason: "legacy let missing 'body' attribute".to_string(),
            })
        }
    }

    pub(crate) fn evaluate_with(
        &self,
        with: &rix_parser::ast::With,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let attrset_expr = with.namespace().ok_or_else(|| Error::UnsupportedExpression {
            reason: "with expression missing namespace".to_string(),
        })?;

        // Lazy 'with': store a thunk and only evaluate if lookup falls through
        let file_id = self.current_file_id();
        let thunk = thunk::Thunk::new(&attrset_expr, scope.clone(), file_id);
        let thunk_value = NixValue::Thunk(Arc::new(thunk));

        let mut new_scope = scope.clone();
        new_scope.push_with(thunk_value);

        let body_expr = with.body().ok_or_else(|| Error::UnsupportedExpression {
            reason: "with expression missing body".to_string(),
        })?;

        self.evaluate_expr_with_scope(&body_expr, &new_scope)
    }

    pub(crate) fn evaluate_assert(
        &self,
        assert: &rix_parser::ast::Assert,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let condition_expr = assert.condition().ok_or_else(|| Error::UnsupportedExpression {
            reason: "assert expression missing condition".to_string(),
        })?;

        let condition_value = self.evaluate_expr_with_scope(&condition_expr, scope)?;
        let condition_forced = condition_value.force(self)?;

        let is_truthy = match condition_forced {
            NixValue::Boolean(false) => false,
            NixValue::Null => false,
            _ => true,
        };

        if !is_truthy {
            return Err(Error::UnsupportedExpression {
                reason: "assertion failed".to_string(),
            });
        }

        let body_expr = assert.body().ok_or_else(|| Error::UnsupportedExpression {
            reason: "assert expression missing body".to_string(),
        })?;

        self.evaluate_expr_with_scope(&body_expr, scope)
    }

    pub(crate) fn evaluate_if_else(
        &self,
        if_else: &rix_parser::ast::IfElse,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let condition_expr = if_else.condition().ok_or_else(|| Error::UnsupportedExpression {
            reason: "if expression missing condition".to_string(),
        })?;

        let condition_value = self.evaluate_expr_with_scope(&condition_expr, scope)?;
        let condition_forced = condition_value.force(self)?;

        let is_truthy = match condition_forced {
            NixValue::Boolean(false) => false,
            NixValue::Null => false,
            _ => true,
        };

        if is_truthy {
            let then_expr = if_else.body().ok_or_else(|| Error::UnsupportedExpression {
                reason: "if expression missing then branch".to_string(),
            })?;
            self.evaluate_expr_with_scope(&then_expr, scope)
        } else {
            let else_expr = if_else.else_body().ok_or_else(|| Error::UnsupportedExpression {
                reason: "if expression missing else branch".to_string(),
            })?;
            self.evaluate_expr_with_scope(&else_expr, scope)
        }
    }

    pub(crate) fn evaluate_path(
        &self,
        path_expr: &rix_parser::ast::Path,
        _scope: &VariableScope,
    ) -> Result<NixValue> {
        let path_str = path_expr.to_string();

        if path_str.starts_with('<') && path_str.ends_with('>') {
            let search_name = &path_str[1..path_str.len() - 1];
            if let Some(search_path) = self.search_paths.get(search_name) {
                return Ok(NixValue::Path(search_path.clone()));
            }
            return Err(Error::UnsupportedExpression {
                reason: format!("unknown search path: {}", search_name),
            });
        }

        let file_path = if path_str.starts_with('/') {
            PathBuf::from(path_str)
        } else {
            if let Some(current_file_path) = self.current_file_path() {
                current_file_path
                    .parent()
                    .unwrap_or(&PathBuf::from("."))
                    .join(&path_str)
            } else {
                PathBuf::from(path_str)
            }
        };

        let path_string = file_path.to_string_lossy();
        if path_string.starts_with("/nix/store/") && self.is_valid_store_path(&path_string) {
            return Ok(NixValue::StorePath(path_string.to_string()));
        }

        Ok(NixValue::Path(file_path))
    }

    pub(crate) fn evaluate_select(
        &self,
        select: &rix_parser::ast::Select,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let expr = select.expr().ok_or_else(|| Error::UnsupportedExpression {
            reason: "select expression missing base expression".to_string(),
        })?;

        let base_value_raw = self.evaluate_expr_with_scope(&expr, scope)?;
        let mut current_value = base_value_raw.force(self)?;

        let attrpath = select.attrpath().ok_or_else(|| Error::UnsupportedExpression {
            reason: "select expression missing attrpath".to_string(),
        })?;

        for attr_node in attrpath.attrs() {
            let attr_syntax = attr_node.syntax();
            let attr_name = if let Some(ident) = rix_parser::ast::Ident::cast(attr_syntax.clone()) {
                ident.to_string()
            } else if let Some(str_node) = rix_parser::ast::Str::cast(attr_syntax.clone()) {
                let str_value = self.evaluate_string(&str_node, scope)?;
                match str_value {
                    NixValue::String(s) => s,
                    _ => return Err(Error::UnsupportedExpression { reason: "attr name must be string".to_string() }),
                }
            } else if let Some(expr) = rix_parser::ast::Expr::cast(attr_syntax.clone()) {
                let expr_value = self.evaluate_expr_with_scope(&expr, scope)?;
                match expr_value.force(self)? {
                    NixValue::String(s) => s,
                    _ => return Err(Error::UnsupportedExpression { reason: "attr name must be string".to_string() }),
                }
            } else {
                attr_syntax.text().to_string().trim_matches('"').to_string()
            };

            match current_value.clone().force(self)? {
                NixValue::AttributeSet(mut attrs) => {
                    if let Some(value) = attrs.remove(&attr_name) {
                        current_value = value;
                    } else {
                        // Support default value (or-expression)
                        if let Some(default_expr) = select.default_expr() {
                            return self.evaluate_expr_with_scope(&default_expr, scope);
                        }
                        return Err(Error::UnsupportedExpression {
                            reason: format!("attribute '{}' not found", attr_name),
                        });
                    }
                }
                _ => return Err(Error::UnsupportedExpression {
                    reason: format!("cannot select from non-attrset: {:?}", current_value),
                }),
            }
        }

        current_value.force(self)
    }

    pub(crate) fn evaluate_has_attr(
        &self,
        has_attr: &rix_parser::ast::HasAttr,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let expr = has_attr.expr().ok_or_else(|| Error::UnsupportedExpression {
            reason: "hasAttr missing base expression".to_string(),
        })?;

        let base_value = self.evaluate_expr_with_scope(&expr, scope)?.force(self)?;
        let mut current_value = base_value;

        let attrpath = has_attr.attrpath().ok_or_else(|| Error::UnsupportedExpression {
            reason: "hasAttr missing attrpath".to_string(),
        })?;

        for attr_node in attrpath.attrs() {
            let attr_syntax = attr_node.syntax();
            let attr_name = if let Some(ident) = rix_parser::ast::Ident::cast(attr_syntax.clone()) {
                ident.to_string()
            } else {
                // Simplified for hasAttr
                attr_syntax.text().to_string().trim_matches('"').to_string()
            };

            match current_value.clone().force(self)? {
                NixValue::AttributeSet(attrs) => {
                    if let Some(value) = attrs.get(&attr_name) {
                        current_value = value.clone();
                    } else {
                        return Ok(NixValue::Boolean(false));
                    }
                }
                _ => return Ok(NixValue::Boolean(false)),
            }
        }

        Ok(NixValue::Boolean(true))
    }

    pub(crate) fn evaluate_paren(&self, paren: &Paren, scope: &VariableScope) -> Result<NixValue> {
        let inner_expr = paren.expr().ok_or_else(|| Error::UnsupportedExpression {
            reason: "paren missing inner expression".to_string(),
        })?;
        self.evaluate_expr_with_scope(&inner_expr, scope)
    }
}
