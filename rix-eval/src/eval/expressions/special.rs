//! Special form expression evaluation

use crate::error::{Error, Result};
use crate::eval::Evaluator;
use crate::eval::context::VariableScope;
use crate::thunk;
use crate::value::NixValue;
use rix_parser::ast::AttrpathValue;
use rix_parser::ast::HasEntry;
use rix_parser::ast::Inherit;
use rowan::ast::AstNode;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

impl Evaluator {
    pub(crate) fn evaluate_let_in(
        &self,
        let_expr: &rix_parser::ast::LetIn,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let file_id = self.current_file_id();
        let mut top_level_bindings: HashMap<String, NixValue> = HashMap::new();
        let shared_rec_map = Arc::new(Mutex::new(HashMap::new()));
        let mut rec_scope = scope.clone();
        rec_scope.push_recursive(shared_rec_map.clone());

        for entry in let_expr.entries() {
            let syntax = entry.syntax();
            if let Some(inherit) = Inherit::cast(syntax.clone()) {
                let inherit_from = inherit.from();
                for attr in inherit.attrs() {
                    let key = attr
                        .syntax()
                        .text()
                        .to_string()
                        .trim_matches('"')
                        .to_string();
                    let val = if let Some(ref from) = inherit_from {
                        let from_expr =
                            from.expr().ok_or_else(|| Error::UnsupportedExpression {
                                reason: "inherit(from) missing expr".to_string(),
                            })?;
                        let from_val = NixValue::Thunk(Arc::new(thunk::Thunk::new(
                            &from_expr,
                            rec_scope.clone(),
                            file_id,
                        )));
                        NixValue::DeferredInherit(Box::new(from_val), key.clone())
                    } else {
                        if let Some(v) = scope.get(&key) {
                            v
                        } else if !scope.withs().is_empty() {
                            self.create_lookup_thunk(&key, scope)?
                        } else {
                            return Err(Error::UnsupportedExpression {
                                reason: format!("unknown identifier: {}", key),
                            });
                        }
                    };

                    let final_val = if let Some(existing) = top_level_bindings.remove(&key) {
                        self.merge_attribute_sets(existing, val)?
                    } else {
                        val
                    };
                    top_level_bindings.insert(key.clone(), final_val.clone());

                    // Update scope immediately so later entries can see it
                    shared_rec_map
                        .lock()
                        .unwrap()
                        .insert(key.clone(), final_val.clone());
                    rec_scope.insert(key, final_val);
                }
            } else if let Some(apv) = AttrpathValue::cast(syntax.clone()) {
                let attrpath = apv.attrpath().ok_or_else(|| Error::UnsupportedExpression {
                    reason: "missing attrpath".to_string(),
                })?;
                let value_expr = apv.value().ok_or_else(|| Error::UnsupportedExpression {
                    reason: "missing value".to_string(),
                })?;

                let mut path = Vec::new();
                for attr in attrpath.attrs() {
                    if let Some(ident) = rix_parser::ast::Ident::cast(attr.syntax().clone()) {
                        path.push(ident.to_string());
                    } else if let Some(dynamic) =
                        rix_parser::ast::Dynamic::cast(attr.syntax().clone())
                    {
                        let expr = dynamic.expr().ok_or_else(|| Error::UnsupportedExpression {
                            reason: "dynamic attr missing expr".to_string(),
                        })?;
                        let name_val = self
                            .evaluate_expr_with_scope(&expr, &rec_scope)?
                            .force(self)?;
                        path.push(name_val.as_string()?);
                    } else {
                        path.push(
                            attr.syntax()
                                .text()
                                .to_string()
                                .trim_matches('"')
                                .to_string(),
                        );
                    }
                }

                let mut nested_val = NixValue::Thunk(Arc::new(thunk::Thunk::new(
                    &value_expr,
                    rec_scope.clone(),
                    file_id,
                )));
                for key in path.iter().skip(1).rev() {
                    let mut inner_map = HashMap::new();
                    inner_map.insert(key.clone(), nested_val);
                    nested_val = NixValue::AttributeSet(inner_map);
                }

                let first_key = path[0].clone();
                let final_val = if let Some(existing) = top_level_bindings.remove(&first_key) {
                    self.merge_attribute_sets(existing, nested_val)?
                } else {
                    nested_val
                };
                top_level_bindings.insert(first_key.clone(), final_val.clone());

                // Update scope immediately
                shared_rec_map
                    .lock()
                    .unwrap()
                    .insert(first_key.clone(), final_val.clone());
                rec_scope.insert(first_key, final_val);
            }
        }

        let body = let_expr
            .body()
            .ok_or_else(|| Error::UnsupportedExpression {
                reason: "let missing body".to_string(),
            })?;
        self.evaluate_expr_with_scope(&body, &rec_scope)
    }

    pub(crate) fn evaluate_legacy_let(
        &self,
        legacy_let: &rix_parser::ast::LegacyLet,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let file_id = self.current_file_id();
        let mut top_level_bindings: HashMap<String, NixValue> = HashMap::new();
        let shared_rec_map = Arc::new(Mutex::new(HashMap::new()));
        let mut rec_scope = scope.clone();
        rec_scope.push_recursive(shared_rec_map.clone());

        for entry in legacy_let.entries() {
            let syntax = entry.syntax();
            if let Some(inherit) = Inherit::cast(syntax.clone()) {
                let inherit_from = inherit.from();
                for attr in inherit.attrs() {
                    let key = attr
                        .syntax()
                        .text()
                        .to_string()
                        .trim_matches('"')
                        .to_string();
                    let val = if let Some(ref from) = inherit_from {
                        let from_expr =
                            from.expr().ok_or_else(|| Error::UnsupportedExpression {
                                reason: "inherit(from) missing expr".to_string(),
                            })?;
                        let from_val = NixValue::Thunk(Arc::new(thunk::Thunk::new(
                            &from_expr,
                            rec_scope.clone(),
                            file_id,
                        )));
                        NixValue::DeferredInherit(Box::new(from_val), key.clone())
                    } else {
                        if let Some(v) = scope.get(&key) {
                            v
                        } else if !scope.withs().is_empty() {
                            self.create_lookup_thunk(&key, scope)?
                        } else {
                            return Err(Error::UnsupportedExpression {
                                reason: format!("unknown identifier: {}", key),
                            });
                        }
                    };

                    top_level_bindings.insert(key.clone(), val.clone());
                    shared_rec_map.lock().unwrap().insert(key, val);
                }
            } else if let Some(apv) = AttrpathValue::cast(syntax.clone()) {
                let attrpath = apv.attrpath().ok_or_else(|| Error::UnsupportedExpression {
                    reason: "missing attrpath".to_string(),
                })?;
                let value_expr = apv.value().ok_or_else(|| Error::UnsupportedExpression {
                    reason: "missing value".to_string(),
                })?;

                let mut path = Vec::new();
                for attr in attrpath.attrs() {
                    if let Some(ident) = rix_parser::ast::Ident::cast(attr.syntax().clone()) {
                        path.push(ident.to_string());
                    } else if let Some(dynamic) =
                        rix_parser::ast::Dynamic::cast(attr.syntax().clone())
                    {
                        let expr = dynamic.expr().ok_or_else(|| Error::UnsupportedExpression {
                            reason: "dynamic attr missing expr".to_string(),
                        })?;
                        let name_val = self.evaluate_expr_with_scope(&expr, scope)?.force(self)?;
                        path.push(name_val.as_string()?);
                    } else {
                        path.push(
                            attr.syntax()
                                .text()
                                .to_string()
                                .trim_matches('"')
                                .to_string(),
                        );
                    }
                }

                let mut nested_val = NixValue::Thunk(Arc::new(thunk::Thunk::new(
                    &value_expr,
                    rec_scope.clone(),
                    file_id,
                )));
                for key in path.iter().skip(1).rev() {
                    let mut inner_map = HashMap::new();
                    inner_map.insert(key.clone(), nested_val);
                    nested_val = NixValue::AttributeSet(inner_map);
                }

                let first_key = path[0].clone();
                top_level_bindings.insert(first_key.clone(), nested_val.clone());
                shared_rec_map.lock().unwrap().insert(first_key, nested_val);
            }
        }

        top_level_bindings
            .get("body")
            .cloned()
            .ok_or_else(|| Error::UnsupportedExpression {
                reason: "legacy let missing body".to_string(),
            })
    }

    pub(crate) fn evaluate_if_else(
        &self,
        if_expr: &rix_parser::ast::IfElse,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let condition_expr = if_expr
            .condition()
            .ok_or_else(|| Error::UnsupportedExpression {
                reason: "if missing condition".to_string(),
            })?;
        let condition = self
            .evaluate_expr_with_scope(&condition_expr, scope)?
            .force(self)?;

        let is_true = match condition {
            NixValue::Boolean(b) => b,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: "if condition must be boolean".to_string(),
                });
            }
        };

        if is_true {
            let then_expr = if_expr.body().ok_or_else(|| Error::UnsupportedExpression {
                reason: "if missing then body".to_string(),
            })?;
            self.evaluate_expr_with_scope(&then_expr, scope)
        } else {
            let else_expr = if_expr
                .else_body()
                .ok_or_else(|| Error::UnsupportedExpression {
                    reason: "if missing else body".to_string(),
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

        // Handle search paths like <nixpkgs>
        if path_str.starts_with('<') && path_str.ends_with('>') {
            let content = &path_str[1..path_str.len() - 1];
            let (name, subpath) = match content.find('/') {
                Some(idx) => (&content[0..idx], Some(&content[idx + 1..])),
                None => (content, None),
            };

            if let Some(base_path) = self.search_paths.get(name) {
                let mut resolved = base_path.clone();
                if let Some(sub) = subpath {
                    resolved.push(sub);
                }
                return Ok(NixValue::Path(resolved));
            } else {
                return Err(Error::UnsupportedExpression {
                    reason: format!("search path '{}' not found", name),
                });
            }
        }

        let path = PathBuf::from(&path_str);

        if path.is_absolute() {
            return Ok(NixValue::Path(path));
        }

        // If it's a relative path (starts with . or .. or is just a name),
        // resolve it relative to the directory of the current file.
        if let Some(mut current_file) = self.current_file_path() {
            current_file.pop(); // Remove filename to get directory
            return Ok(NixValue::Path(current_file.join(path)));
        }

        Ok(NixValue::Path(path))
    }

    pub(crate) fn evaluate_paren(
        &self,
        paren: &rix_parser::ast::Paren,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let expr = paren.expr().ok_or_else(|| Error::UnsupportedExpression {
            reason: "paren missing expr".to_string(),
        })?;
        self.evaluate_expr_with_scope(&expr, scope)
    }

    pub(crate) fn evaluate_with(
        &self,
        with_expr: &rix_parser::ast::With,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let namespace_expr = with_expr
            .namespace()
            .ok_or_else(|| Error::UnsupportedExpression {
                reason: "with missing namespace".to_string(),
            })?;
        let body_expr = with_expr
            .body()
            .ok_or_else(|| Error::UnsupportedExpression {
                reason: "with missing body".to_string(),
            })?;

        let file_id = self.current_file_id();
        let namespace_thunk =
            std::sync::Arc::new(crate::Thunk::new(&namespace_expr, scope.clone(), file_id));
        let namespace_val = NixValue::Thunk(namespace_thunk);

        let mut new_scope = scope.clone();
        new_scope.push_with(namespace_val);

        self.evaluate_expr_with_scope(&body_expr, &new_scope)
    }

    pub(crate) fn evaluate_assert(
        &self,
        assert_expr: &rix_parser::ast::Assert,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let condition_expr =
            assert_expr
                .condition()
                .ok_or_else(|| Error::UnsupportedExpression {
                    reason: "assert missing condition".to_string(),
                })?;
        let body_expr = assert_expr
            .body()
            .ok_or_else(|| Error::UnsupportedExpression {
                reason: "assert missing body".to_string(),
            })?;

        let condition = self
            .evaluate_expr_with_scope(&condition_expr, scope)?
            .force(self)?;
        match condition {
            NixValue::Boolean(true) => self.evaluate_expr_with_scope(&body_expr, scope),
            NixValue::Boolean(false) => Err(Error::UnsupportedExpression {
                reason: "assertion failed".to_string(),
            }),
            _ => Err(Error::UnsupportedExpression {
                reason: "assert condition must be boolean".to_string(),
            }),
        }
    }
}
