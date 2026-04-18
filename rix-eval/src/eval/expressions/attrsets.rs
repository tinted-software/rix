//! Attribute set expression evaluation

use crate::error::{Error, Result};
use crate::eval::Evaluator;
use crate::eval::context::VariableScope;
use crate::thunk;
use crate::value::NixValue;
use rix_parser::ast::Expr;
use rix_parser::ast::{AttrpathValue, HasEntry, Inherit};
use rowan::ast::AstNode;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

impl Evaluator {
    pub(crate) fn evaluate_attr_set(
        &self,
        set: &rix_parser::ast::AttrSet,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        // In Nix, multiple entries for the same key (e.g. set = { a = 1; }; set = { b = 2; };)
        // are merged. Crucially, if any is 'rec', they all share the recursive scope.
        // We'll perform a basic grouping here or rely on the underlying merge logic
        // but ensure the scope is updated correctly.

        // Check if this is a recursive attribute set
        let is_recursive = set.rec_token().is_some();

        if is_recursive {
            self.evaluate_recursive_attr_set(set, scope)
        } else {
            self.evaluate_normal_attr_set(set, scope)
        }
    }

    fn evaluate_normal_attr_set(
        &self,
        set: &rix_parser::ast::AttrSet,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let mut bindings = HashMap::new();
        let file_id = self.current_file_id();

        for entry in set.entries() {
            let syntax = entry.syntax();
            if let Some(inherit) = Inherit::cast(syntax.clone()) {
                let inherit_from = inherit.from();
                let inherit_scope = if let Some(ref from) = inherit_from {
                    if let Some(expr) = from.expr() {
                        let val = self.evaluate_expr_with_scope(&expr, scope)?.force(self)?;
                        match val {
                            NixValue::AttributeSet(m) => {
                                let mut s = VariableScope::new();
                                for (k, v) in m {
                                    s.insert(k, v);
                                }
                                s
                            }
                            _ => {
                                return Err(Error::UnsupportedExpression {
                                    reason: "inherit from non-attrset".to_string(),
                                });
                            }
                        }
                    } else {
                        scope.clone()
                    }
                } else {
                    scope.clone()
                };

                for attr in inherit.attrs() {
                    let key = attr
                        .syntax()
                        .text()
                        .to_string()
                        .trim_matches('"')
                        .to_string();
                    let val = if inherit_from.is_some() {
                        inherit_scope
                            .get(&key)
                            .ok_or_else(|| Error::UnsupportedExpression {
                                reason: format!("inherit from: attr {} not found", key),
                            })?
                    } else {
                        scope
                            .get(&key)
                            .or_else(|| {
                                if !scope.withs().is_empty() {
                                    self.create_lookup_thunk(&key, scope).ok()
                                } else {
                                    None
                                }
                            })
                            .ok_or_else(|| Error::UnsupportedExpression {
                                reason: format!("unknown identifier: {}", key),
                            })?
                    };
                    bindings.insert(key, val);
                }
            } else if let Some(apv) = AttrpathValue::cast(syntax.clone()) {
                let attrpath = apv.attrpath().unwrap();
                let value_expr = apv.value().ok_or_else(|| Error::UnsupportedExpression {
                    reason: "missing value".to_string(),
                })?;

                let mut path = Vec::new();
                for attr in attrpath.attrs() {
                    if let Some(ident) = rix_parser::ast::Ident::cast(attr.syntax().clone()) {
                        path.push(ident.to_string());
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

                let thunk = NixValue::Thunk(Arc::new(thunk::Thunk::new(
                    &value_expr,
                    scope.clone(),
                    file_id,
                )));

                let mut current = &mut bindings;
                for (i, key) in path.iter().enumerate() {
                    if i == path.len() - 1 {
                        if let Some(existing) = current.remove(key) {
                            let merged = self.merge_attribute_sets(existing, thunk.clone())?;
                            current.insert(key.clone(), merged);
                        } else {
                            current.insert(key.clone(), thunk.clone());
                        }
                    } else {
                        let entry = current
                            .entry(key.clone())
                            .or_insert_with(|| NixValue::AttributeSet(HashMap::new()));
                        if let NixValue::AttributeSet(ref mut m) = *entry {
                            current = m;
                        } else {
                            // If it's a thunk that forced to a set, we'd need to merge.
                            // For simplicity, handle only literal nested sets for now.
                            return Err(Error::UnsupportedExpression {
                                reason: "duplicate key (not a set)".to_string(),
                            });
                        }
                    }
                }
            }
        }

        Ok(NixValue::AttributeSet(bindings))
    }

    pub(crate) fn evaluate_recursive_attr_set(
        &self,
        set: &rix_parser::ast::AttrSet,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let file_id = self.current_file_id();
        let mut top_level_bindings: HashMap<String, NixValue> = HashMap::new();
        let shared_rec_map = Arc::new(Mutex::new(HashMap::new()));
        let mut rec_scope = scope.clone();
        rec_scope.push_recursive(shared_rec_map.clone());

        // Recursive sets in Nix are special: they share a scope for ALL their attributes.
        // We'll follow the same logic as normal sets but use rec_scope for all thunks.
        for entry in set.entries() {
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
                        let from_expr = from.expr().unwrap();
                        let from_val = self.evaluate_expr_with_scope(&from_expr, &rec_scope)?;
                        NixValue::DeferredInherit(Box::new(from_val), key.clone())
                    } else {
                        rec_scope
                            .get(&key)
                            .or_else(|| {
                                if !rec_scope.withs().is_empty() {
                                    self.create_lookup_thunk(&key, &rec_scope).ok()
                                } else {
                                    None
                                }
                            })
                            .ok_or_else(|| Error::UnsupportedExpression {
                                reason: format!("unknown identifier: {}", key),
                            })?
                    };

                    top_level_bindings.insert(key.clone(), val.clone());
                    shared_rec_map
                        .lock()
                        .unwrap()
                        .insert(key.clone(), val.clone());
                }
            } else if let Some(apv) = AttrpathValue::cast(syntax.clone()) {
                let attrpath = apv.attrpath().unwrap();
                let value_expr = apv.value().unwrap();

                let mut path = Vec::new();
                for attr in attrpath.attrs() {
                    path.push(
                        attr.syntax()
                            .text()
                            .to_string()
                            .trim_matches('"')
                            .to_string(),
                    );
                }

                let thunk = NixValue::Thunk(Arc::new(thunk::Thunk::new(
                    &value_expr,
                    rec_scope.clone(),
                    file_id,
                )));

                // We'll use a recursive merge for top-level recursive sets
                let mut current_map = &mut top_level_bindings;
                for (i, key) in path.iter().enumerate() {
                    if i == path.len() - 1 {
                        let final_val = if let Some(existing) = current_map.remove(key) {
                            self.merge_attribute_sets(existing, thunk.clone())?
                        } else {
                            thunk.clone()
                        };
                        current_map.insert(key.clone(), final_val.clone());
                        if i == 0 {
                            shared_rec_map
                                .lock()
                                .unwrap()
                                .insert(key.clone(), final_val);
                        }
                    } else {
                        let entry = current_map
                            .entry(key.clone())
                            .or_insert_with(|| NixValue::AttributeSet(HashMap::new()));
                        if let NixValue::AttributeSet(ref mut m) = *entry {
                            current_map = m;
                        } else {
                            return Err(Error::UnsupportedExpression {
                                reason: "duplicate key (not a set)".to_string(),
                            });
                        }
                    }
                }
            }
        }

        Ok(NixValue::AttributeSet(top_level_bindings))
    }

    pub(crate) fn evaluate_select(
        &self,
        select: &rix_parser::ast::Select,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let set_val = self
            .evaluate_expr_with_scope(&select.expr().unwrap(), scope)?
            .force(self)?;
        let attrpath = select.attrpath().unwrap();

        let mut current = set_val;
        for attr in attrpath.attrs() {
            let key = if let Some(ident) = rix_parser::ast::Ident::cast(attr.syntax().clone()) {
                ident.to_string()
            } else if let Some(dynamic) = rix_parser::ast::Dynamic::cast(attr.syntax().clone()) {
                let name_val = self
                    .evaluate_expr_with_scope(&dynamic.expr().unwrap(), scope)?
                    .force(self)?;
                name_val.as_string()?
            } else {
                attr.syntax()
                    .text()
                    .to_string()
                    .trim_matches('"')
                    .to_string()
            };

            match current {
                NixValue::AttributeSet(attrs) => {
                    if let Some(val) = attrs.get(&key) {
                        current = val.clone();
                    } else if let Some(default) = select.default_expr() {
                        return self.evaluate_expr_with_scope(&default, scope);
                    } else {
                        return Err(Error::UnsupportedExpression {
                            reason: format!("attribute '{}' not found", key),
                        });
                    }
                }
                _ => {
                    return Err(Error::UnsupportedExpression {
                        reason: format!("cannot select from non-attrset: {}", current),
                    });
                }
            }
        }
        Ok(current)
    }

    pub(crate) fn evaluate_has_attr(
        &self,
        has_attr: &rix_parser::ast::HasAttr,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let set_val = self
            .evaluate_expr_with_scope(&has_attr.expr().unwrap(), scope)?
            .force(self)?;
        let attrpath = has_attr.attrpath().unwrap();

        let mut current = set_val;
        for attr in attrpath.attrs() {
            let key = if let Some(ident) = rix_parser::ast::Ident::cast(attr.syntax().clone()) {
                ident.to_string()
            } else if let Some(dynamic) = rix_parser::ast::Dynamic::cast(attr.syntax().clone()) {
                let name_val = self
                    .evaluate_expr_with_scope(&dynamic.expr().unwrap(), scope)?
                    .force(self)?;
                name_val.as_string()?
            } else {
                attr.syntax()
                    .text()
                    .to_string()
                    .trim_matches('"')
                    .to_string()
            };

            match current {
                NixValue::AttributeSet(attrs) => {
                    if let Some(val) = attrs.get(&key) {
                        current = val.clone();
                    } else {
                        return Ok(NixValue::Boolean(false));
                    }
                }
                _ => return Ok(NixValue::Boolean(false)),
            }
        }
        Ok(NixValue::Boolean(true))
    }
}
