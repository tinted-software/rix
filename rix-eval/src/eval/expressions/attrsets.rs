//! Attribute set expression evaluation

use crate::error::{Error, Result};
use crate::eval::Evaluator;
use crate::eval::context::VariableScope;
use crate::thunk;
use crate::value::NixValue;
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
        let mut attrs = HashMap::new();
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
                                for (k, v) in m { s.insert(k, v); }
                                s
                            }
                            _ => return Err(Error::UnsupportedExpression { reason: "inherit from non-attrset".to_string() }),
                        }
                    } else { scope.clone() }
                } else { scope.clone() };

                for attr in inherit.attrs() {
                    let key = attr.syntax().text().to_string().trim_matches('"').to_string();
                    if inherit_from.is_some() {
                        if let Some(val) = inherit_scope.get(&key) {
                            attrs.insert(key, val);
                        } else {
                            return Err(Error::UnsupportedExpression { reason: format!("inherit from: attr {} not found", key) });
                        }
                    } else {
                        let val = if scope.get(&key).is_some() {
                             // If it's definitely lexical, we can look it up now
                             // But wait! It might be shadowed by a 'with' later?
                             // No, lexical takes precedence.
                             scope.get(&key).unwrap()
                        } else if !scope.withs().is_empty() {
                             // If it might be in 'with', use deferred lookup
                             self.create_lookup_thunk(&key, scope)?
                        } else {
                             // Definitely missing
                             return Err(Error::UnsupportedExpression { reason: format!("unknown identifier: {}", key) });
                        };
                        attrs.insert(key, val);
                    }
                }
            } else if let Some(apv) = AttrpathValue::cast(syntax.clone()) {
                let attrpath = apv.attrpath().ok_or_else(|| Error::UnsupportedExpression { reason: "missing attrpath".to_string() })?;
                let value_expr = apv.value().ok_or_else(|| Error::UnsupportedExpression { reason: "missing value".to_string() })?;
                
                let mut path = Vec::new();
                for attr in attrpath.attrs() {
                    if let Some(ident) = rix_parser::ast::Ident::cast(attr.syntax().clone()) {
                        path.push(ident.to_string());
                    } else if let Some(dynamic) = rix_parser::ast::Dynamic::cast(attr.syntax().clone()) {
                        let expr = dynamic.expr().ok_or_else(|| Error::UnsupportedExpression { reason: "dynamic attr missing expr".to_string() })?;
                        let name_val = self.evaluate_expr_with_scope(&expr, scope)?.force(self)?;
                        path.push(name_val.as_string()?);
                    } else if let Some(str_node) = rix_parser::ast::Str::cast(attr.syntax().clone()) {
                        let name_val = self.evaluate_string(&str_node, scope)?;
                        path.push(name_val.as_string()?);
                    } else {
                        path.push(attr.syntax().text().to_string().trim_matches('"').to_string());
                    }
                }

                let thunk = NixValue::Thunk(Arc::new(thunk::Thunk::new(&value_expr, scope.clone(), file_id)));
                
                let mut nested_val = thunk;
                for key in path.iter().skip(1).rev() {
                    let mut inner_map = HashMap::new();
                    inner_map.insert(key.clone(), nested_val);
                    nested_val = NixValue::AttributeSet(inner_map);
                }

                let first_key = path[0].clone();
                if let Some(existing) = attrs.remove(&first_key) {
                    let merged = self.merge_attribute_sets(existing, nested_val)?;
                    attrs.insert(first_key, merged);
                } else {
                    attrs.insert(first_key, nested_val);
                }
            }
        }

        Ok(NixValue::AttributeSet(attrs))
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

        for entry in set.entries() {
            let syntax = entry.syntax();
            if let Some(inherit) = Inherit::cast(syntax.clone()) {
                let inherit_from = inherit.from();
                for attr in inherit.attrs() {
                    let key = attr.syntax().text().to_string().trim_matches('"').to_string();
                    let val = if let Some(ref from) = inherit_from {
                        let from_expr = from.expr().ok_or_else(|| Error::UnsupportedExpression { reason: "inherit(from) missing expr".to_string() })?;
                        let from_val = self.evaluate_expr_with_scope(&from_expr, &rec_scope)?;
                        NixValue::DeferredInherit(Box::new(from_val), key.clone())
                    } else {
                        if let Some(v) = scope.get(&key) {
                            v
                        } else if !scope.withs().is_empty() {
                            self.create_lookup_thunk(&key, scope)?
                        } else {
                            return Err(Error::UnsupportedExpression { reason: format!("unknown identifier: {}", key) });
                        }
                    };

                    let final_val = if let Some(existing) = top_level_bindings.remove(&key) {
                        self.merge_attribute_sets(existing, val)?
                    } else {
                        val
                    };
                    top_level_bindings.insert(key.clone(), final_val.clone());
                    
                    // Update scope immediately
                    shared_rec_map.lock().unwrap().insert(key.clone(), final_val.clone());
                    rec_scope.insert(key, final_val);
                }
            } else if let Some(apv) = AttrpathValue::cast(syntax.clone()) {
                let attrpath = apv.attrpath().ok_or_else(|| Error::UnsupportedExpression { reason: "missing attrpath".to_string() })?;
                let value_expr = apv.value().ok_or_else(|| Error::UnsupportedExpression { reason: "missing value".to_string() })?;
                
                let mut path = Vec::new();
                for attr in attrpath.attrs() {
                    if let Some(ident) = rix_parser::ast::Ident::cast(attr.syntax().clone()) {
                        path.push(ident.to_string());
                    } else if let Some(dynamic) = rix_parser::ast::Dynamic::cast(attr.syntax().clone()) {
                        let expr = dynamic.expr().ok_or_else(|| Error::UnsupportedExpression { reason: "dynamic attr missing expr".to_string() })?;
                        let name_val = self.evaluate_expr_with_scope(&expr, &rec_scope)?.force(self)?;
                        path.push(name_val.as_string()?);
                    } else {
                        path.push(attr.syntax().text().to_string().trim_matches('"').to_string());
                    }
                }

                let mut nested_val = NixValue::Thunk(Arc::new(thunk::Thunk::new(&value_expr, rec_scope.clone(), file_id)));
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
                shared_rec_map.lock().unwrap().insert(first_key.clone(), final_val.clone());
                rec_scope.insert(first_key, final_val);
            }
        }

        Ok(NixValue::AttributeSet(top_level_bindings))
    }

    pub(crate) fn evaluate_select(
        &self,
        select: &rix_parser::ast::Select,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let set_expr = select.expr().ok_or_else(|| Error::UnsupportedExpression { reason: "select missing expression".to_string() })?;
        let mut current = self.evaluate_expr_with_scope(&set_expr, scope)?.force(self)?;

        if let Some(attrpath) = select.attrpath() {
            for attr in attrpath.attrs() {
                let key = if let Some(ident) = rix_parser::ast::Ident::cast(attr.syntax().clone()) {
                    ident.to_string()
                } else if let Some(dynamic) = rix_parser::ast::Dynamic::cast(attr.syntax().clone()) {
                    let expr = dynamic.expr().ok_or_else(|| Error::UnsupportedExpression { reason: "dynamic attr missing expr".to_string() })?;
                    self.evaluate_expr_with_scope(&expr, scope)?.force(self)?.as_string()?
                } else {
                    attr.syntax().text().to_string().trim_matches('"').to_string()
                };

                match current {
                    NixValue::AttributeSet(m) => {
                        if let Some(val) = m.get(&key) {
                            current = val.clone().force(self)?;
                        } else {
                            if let Some(default) = select.default_expr() {
                                return self.evaluate_expr_with_scope(&default, scope);
                            } else {
                                return Err(Error::UnsupportedExpression { reason: format!("attribute '{}' not found", key) });
                            }
                        }
                    }
                    _ => {
                        if let Some(default) = select.default_expr() {
                            return self.evaluate_expr_with_scope(&default, scope);
                        } else {
                            return Err(Error::UnsupportedExpression { reason: format!("cannot select from non-attrset: {}", current) });
                        }
                    }
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
        let set_expr = has_attr.expr().ok_or_else(|| Error::UnsupportedExpression { reason: "has_attr missing expression".to_string() })?;
        let mut current = self.evaluate_expr_with_scope(&set_expr, scope)?.force(self)?;

        if let Some(attrpath) = has_attr.attrpath() {
            for attr in attrpath.attrs() {
                let key = if let Some(ident) = rix_parser::ast::Ident::cast(attr.syntax().clone()) {
                    ident.to_string()
                } else if let Some(dynamic) = rix_parser::ast::Dynamic::cast(attr.syntax().clone()) {
                    let expr = dynamic.expr().ok_or_else(|| Error::UnsupportedExpression { reason: "dynamic attr missing expr".to_string() })?;
                    self.evaluate_expr_with_scope(&expr, scope)?.force(self)?.as_string()?
                } else {
                    attr.syntax().text().to_string().trim_matches('"').to_string()
                };

                match current {
                    NixValue::AttributeSet(m) => {
                        if let Some(val) = m.get(&key) {
                            current = val.clone().force(self)?;
                        } else {
                            return Ok(NixValue::Boolean(false));
                        }
                    }
                    _ => return Ok(NixValue::Boolean(false)),
                }
            }
        }
        Ok(NixValue::Boolean(true))
    }
}
