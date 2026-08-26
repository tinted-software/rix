//! Attribute set expression evaluation

use crate::error::{Error, Result};
use crate::eval::Evaluator;
use crate::eval::context::VariableScope;
use crate::thunk;
use crate::value::NixValue;
use rix_parser::ast::{AttrpathValue, Expr, HasEntry, Inherit};
use rowan::ast::AstNode;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

impl Evaluator {
    pub(crate) fn evaluate_attr(
        &self,
        attr: &rix_parser::ast::Attr,
        scope: &VariableScope,
    ) -> Result<Option<String>> {
        match attr {
            rix_parser::ast::Attr::Ident(ident) => Ok(Some(ident.to_string())),
            rix_parser::ast::Attr::Dynamic(dynamic) => {
                let name_val = self
                    .evaluate_expr_with_scope(&dynamic.expr().unwrap(), scope)?
                    .force(self)?;
                if let NixValue::Null = name_val {
                    Ok(None)
                } else {
                    Ok(Some(name_val.as_string()?))
                }
            }
            rix_parser::ast::Attr::Str(s) => {
                let val = self.evaluate_string(s, scope)?;
                Ok(Some(val.as_string()?))
            }
        }
    }

    pub(crate) fn evaluate_attr_set(
        &self,
        set: &rix_parser::ast::AttrSet,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let is_recursive = set.rec_token().is_some();
        if is_recursive {
            self.evaluate_recursive_attr_set_entries(set.entries().collect(), scope)
        } else {
            self.evaluate_normal_attr_set_entries(set.entries().collect(), scope)
        }
    }

    pub(crate) fn evaluate_normal_attr_set_entries(
        &self,
        entries: Vec<rix_parser::ast::Entry>,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let mut bindings = HashMap::new();
        let file_id = self.current_file_id();

        // Separate inherits from attrpath values
        let mut inherits = Vec::new();
        let mut apv_entries = Vec::new();

        for entry in entries {
            let syntax = entry.syntax();
            if let Some(inherit) = Inherit::cast(syntax.clone()) {
                inherits.push(inherit);
            } else if let Some(apv) = AttrpathValue::cast(syntax.clone()) {
                apv_entries.push(apv);
            }
        }

        // Process inherits
        for inherit in inherits {
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
                let key = match self.evaluate_attr(&attr, scope)? {
                    Some(k) => k,
                    None => continue,
                };
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
        }

        // Group APVs by first key to detect merged literal AttrSets (e.g. set = rec { ... }; set = { ... };)
        let mut apvs_by_first_key: HashMap<String, Vec<(Vec<String>, AttrpathValue)>> =
            HashMap::new();
        for apv in apv_entries {
            let attrpath = apv.attrpath().unwrap();
            let mut path = Vec::new();
            let mut skip = false;
            for attr in attrpath.attrs() {
                if let Some(k) = self.evaluate_attr(&attr, scope)? {
                    path.push(k);
                } else {
                    skip = true;
                    break;
                }
            }
            if skip || path.is_empty() {
                continue;
            }
            let first_key = path[0].clone();
            apvs_by_first_key
                .entry(first_key)
                .or_default()
                .push((path, apv));
        }

        for (first_key, group) in apvs_by_first_key {
            // Check if all in group are single-segment path and their values are literal AttrSets
            let all_single_attrsets = group.len() > 1
                && group.iter().all(|(path, apv)| {
                    path.len() == 1 && matches!(apv.value(), Some(Expr::AttrSet(_)))
                });

            if all_single_attrsets {
                let mut is_rec = false;
                let mut combined_sub_entries = Vec::new();
                for (_, apv) in group {
                    if let Some(Expr::AttrSet(s)) = apv.value() {
                        if s.rec_token().is_some() {
                            is_rec = true;
                        }
                        combined_sub_entries.extend(s.entries());
                    }
                }
                let merged_set = if is_rec {
                    self.evaluate_recursive_attr_set_entries(combined_sub_entries, scope)?
                } else {
                    self.evaluate_normal_attr_set_entries(combined_sub_entries, scope)?
                };
                bindings.insert(first_key, merged_set);
            } else {
                for (path, apv) in group {
                    let value_expr = apv.value().ok_or_else(|| Error::UnsupportedExpression {
                        reason: "missing value".to_string(),
                    })?;

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
                                return Err(Error::UnsupportedExpression {
                                    reason: "duplicate key (not a set)".to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(NixValue::AttributeSet(bindings))
    }

    pub(crate) fn evaluate_recursive_attr_set_entries(
        &self,
        entries: Vec<rix_parser::ast::Entry>,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let file_id = self.current_file_id();
        let mut top_level_bindings: HashMap<String, NixValue> = HashMap::new();
        let shared_rec_map = Arc::new(Mutex::new(HashMap::new()));
        let mut rec_scope = scope.clone();
        rec_scope.push_recursive(shared_rec_map.clone());

        // Separate inherits from attrpath values
        let mut inherits = Vec::new();
        let mut apv_entries = Vec::new();

        for entry in entries {
            let syntax = entry.syntax();
            if let Some(inherit) = Inherit::cast(syntax.clone()) {
                inherits.push(inherit);
            } else if let Some(apv) = AttrpathValue::cast(syntax.clone()) {
                apv_entries.push(apv);
            }
        }

        // Pass 1: Inherits
        for inherit in inherits {
            let inherit_from = inherit.from();
            for attr in inherit.attrs() {
                let key = match self.evaluate_attr(&attr, scope)? {
                    Some(k) => k,
                    None => continue,
                };
                let val = if let Some(ref from) = inherit_from {
                    let from_expr = from.expr().ok_or_else(|| Error::UnsupportedExpression {
                        reason: "inherit(from) missing expr".to_string(),
                    })?;
                    // The 'from' expression must be evaluated in the rec_scope
                    let from_thunk = NixValue::Thunk(Arc::new(thunk::Thunk::new(
                        &from_expr,
                        rec_scope.clone(),
                        file_id,
                    )));
                    NixValue::DeferredInherit(Box::new(from_thunk), key.clone())
                } else {
                    // Lookup in the outer scope
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

                top_level_bindings.insert(key.clone(), val.clone());
                shared_rec_map
                    .lock()
                    .unwrap()
                    .insert(key.clone(), val.clone());
            }
        }

        // Group APVs by first key to detect merged literal AttrSets
        let mut apvs_by_first_key: HashMap<String, Vec<(Vec<String>, AttrpathValue)>> =
            HashMap::new();
        for apv in apv_entries {
            let attrpath = apv.attrpath().unwrap();
            let mut path = Vec::new();
            let mut skip = false;
            for attr in attrpath.attrs() {
                if let Some(k) = self.evaluate_attr(&attr, scope)? {
                    path.push(k);
                } else {
                    skip = true;
                    break;
                }
            }
            if skip || path.is_empty() {
                continue;
            }
            let first_key = path[0].clone();
            apvs_by_first_key
                .entry(first_key)
                .or_default()
                .push((path, apv));
        }

        for (first_key, group) in apvs_by_first_key {
            let all_single_attrsets = group.len() > 1
                && group.iter().all(|(path, apv)| {
                    path.len() == 1 && matches!(apv.value(), Some(Expr::AttrSet(_)))
                });

            if all_single_attrsets {
                let mut is_rec = false;
                let mut combined_sub_entries = Vec::new();
                for (_, apv) in group {
                    if let Some(Expr::AttrSet(s)) = apv.value() {
                        if s.rec_token().is_some() {
                            is_rec = true;
                        }
                        combined_sub_entries.extend(s.entries());
                    }
                }
                let merged_set = if is_rec {
                    self.evaluate_recursive_attr_set_entries(combined_sub_entries, &rec_scope)?
                } else {
                    self.evaluate_normal_attr_set_entries(combined_sub_entries, &rec_scope)?
                };
                top_level_bindings.insert(first_key.clone(), merged_set.clone());
                shared_rec_map.lock().unwrap().insert(first_key, merged_set);
            } else {
                for (path, apv) in group {
                    let value_expr = apv.value().ok_or_else(|| Error::UnsupportedExpression {
                        reason: "missing value".to_string(),
                    })?;

                    let value_thunk = NixValue::Thunk(Arc::new(thunk::Thunk::new(
                        &value_expr,
                        rec_scope.clone(),
                        file_id,
                    )));

                    // Build nested sets if needed
                    let mut current_val = value_thunk;
                    for key in path.iter().skip(1).rev() {
                        let mut m = HashMap::new();
                        m.insert(key.clone(), current_val);
                        current_val = NixValue::AttributeSet(m);
                    }

                    let first_k = path[0].clone();
                    let final_val = if let Some(existing) = top_level_bindings.remove(&first_k) {
                        self.merge_attribute_sets(existing, current_val)?
                    } else {
                        current_val
                    };

                    top_level_bindings.insert(first_k.clone(), final_val.clone());
                    shared_rec_map.lock().unwrap().insert(first_k, final_val);
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
            current = current.force(self)?;
            let key_opt = self.evaluate_attr(&attr, scope)?;
            let key = match key_opt {
                Some(k) => k,
                None => {
                    // Selection with null key: not found, use default or error
                    if let Some(default) = select.default_expr() {
                        return self.evaluate_expr_with_scope(&default, scope);
                    } else {
                        return Err(Error::UnsupportedExpression {
                            reason: "attribute 'null' not found".to_string(),
                        });
                    }
                }
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
                    // If the current value is not an attrset, check for or-default
                    if let Some(default) = select.default_expr() {
                        return self.evaluate_expr_with_scope(&default, scope);
                    }
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
            current = current.force(self)?;
            let key_opt = self.evaluate_attr(&attr, scope)?;
            let key = match key_opt {
                Some(k) => k,
                None => return Ok(NixValue::Boolean(false)),
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
