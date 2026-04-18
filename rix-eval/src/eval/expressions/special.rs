//! Special form expression evaluation

use crate::error::{Error, Result};
use crate::eval::Evaluator;
use crate::eval::context::VariableScope;
use crate::thunk;
use crate::value::NixValue;
use rix_parser::ast::HasEntry;
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
        let file_id = self.current_file_id();
        let mut new_scope = scope.clone();
        let shared_rec_map = Arc::new(std::sync::Mutex::new(HashMap::new()));
        
        let mut rec_scope = scope.clone();
        rec_scope.set_recursive(shared_rec_map.clone());

        // Phase 1: Collect inherits that are recursive (inherit (x) a;)
        let mut recursive_inherits = Vec::new();
        for inherit_node in let_in.inherits() {
            let from_expr = inherit_node.from().and_then(|f| f.expr());
            if let Some(from) = from_expr {
                for attr in inherit_node.attrs() {
                    let key = attr.syntax().text().to_string().trim_matches('"').to_string();
                    recursive_inherits.push((key, from.clone()));
                }
            } else {
                // simple inherit: inherit a; (non-recursive, looks up in outer scope)
                for attr in inherit_node.attrs() {
                    let key = attr.syntax().text().to_string().trim_matches('"').to_string();
                    let val = self.lookup_identifier(&key, scope)?;
        let mut inherit_bindings = Vec::new();

        // Phase 2: Process direct bindings and prepare thunks for inherits
        for entry in let_in.entries() {
            let syntax = entry.syntax();
            if let Some(attrpath_value) = AttrpathValue::cast(syntax.clone()) {
                let attrpath = attrpath_value.attrpath().ok_or_else(|| Error::UnsupportedExpression {
                    reason: "let: missing attrpath".to_string(),
                })?;
                let value_expr = attrpath_value.value().ok_or_else(|| Error::UnsupportedExpression {
                    reason: "let: missing value".to_string(),
                })?;

                let first_attr = attrpath.attrs().next().ok_or_else(|| Error::UnsupportedExpression {
                    reason: "let: empty attrpath".to_string(),
                })?;

                let var_name = if let Some(ident) = rix_parser::ast::Ident::cast(first_attr.syntax().clone()) {
                    ident.to_string()
                } else if let Some(dynamic) = rix_parser::ast::Dynamic::cast(first_attr.syntax().clone()) {
                    let expr = dynamic.expr().ok_or_else(|| Error::UnsupportedExpression {
                        reason: "let: dynamic attribute missing expression".to_string(),
                    })?;
                    let name_val = self.evaluate_expr_with_scope(&expr, &rec_scope)?;
                    name_val.force(self)?.as_string()?
                } else if let Some(str_node) = rix_parser::ast::Str::cast(first_attr.syntax().clone()) {
                    let name_val = self.evaluate_string(&str_node, &rec_scope)?;
                    name_val.as_string()?
                } else {
                }
            } else {
                return Err(Error::UnsupportedExpression { reason: "inherit from non-attrset".to_string() });
            }
        }

        // Phase 4: Evaluate body
        let body_expr = let_in.body().ok_or_else(|| Error::UnsupportedExpression {
            reason: "let: missing body".to_string(),
        })?;
        self.evaluate_expr_with_scope(&body_expr, &new_scope)
    }

    pub(crate) fn evaluate_legacy_let(
        &self,
        legacy_let: &rix_parser::ast::LegacyLet,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let file_id = self.current_file_id();
        let mut new_scope = scope.clone();
        let shared_rec_map = Arc::new(std::sync::Mutex::new(HashMap::new()));
        
        let mut rec_scope = scope.clone();
        rec_scope.set_recursive(shared_rec_map.clone());

        for binding in legacy_let.attrpath_values() {
            let attrpath = binding.attrpath().ok_or_else(|| Error::UnsupportedExpression {
                reason: "let: missing attrpath".to_string(),
            })?;
            let value_expr = binding.value().ok_or_else(|| Error::UnsupportedExpression {
                reason: "let: missing value".to_string(),
            })?;

            let first_attr = attrpath.attrs().next().unwrap();
            let var_name = first_attr.syntax().text().to_string().trim_matches('"').to_string();

            let thunk = thunk::Thunk::new(&value_expr, rec_scope.clone(), file_id);
            let thunk_value = NixValue::Thunk(Arc::new(thunk));
            shared_rec_map.lock().unwrap().insert(var_name.clone(), thunk_value.clone());
            new_scope.insert(var_name, thunk_value);
        }

        if let Some(body_val) = new_scope.get("body") {
            body_val.force(self)
        } else {
            Err(Error::UnsupportedExpression { reason: "legacy let: missing body".to_string() })
        }
    }

    pub(crate) fn evaluate_with(
        &self,
        with: &rix_parser::ast::With,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let attrset_expr = with.namespace().ok_or_else(|| Error::UnsupportedExpression {
            reason: "with: missing namespace".to_string(),
        })?;

        let file_id = self.current_file_id();
        let thunk = thunk::Thunk::new(&attrset_expr, scope.clone(), file_id);
        let thunk_value = NixValue::Thunk(Arc::new(thunk));

        let mut new_scope = scope.clone();
        new_scope.push_with(thunk_value);

        let body_expr = with.body().ok_or_else(|| Error::UnsupportedExpression {
            reason: "with: missing body".to_string(),
        })?;

        self.evaluate_expr_with_scope(&body_expr, &new_scope)
    }

    pub(crate) fn evaluate_assert(
        &self,
        assert: &rix_parser::ast::Assert,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let condition_expr = assert.condition().ok_or_else(|| Error::UnsupportedExpression {
            reason: "assert: missing condition".to_string(),
        })?;

        let condition_forced = self.evaluate_expr_with_scope(&condition_expr, scope)?.force(self)?;
        if !condition_forced.as_bool().unwrap_or(true) {
            return Err(Error::UnsupportedExpression { reason: "assertion failed".to_string() });
        }

        let body_expr = assert.body().ok_or_else(|| Error::UnsupportedExpression {
            reason: "assert: missing body".to_string(),
        })?;
        self.evaluate_expr_with_scope(&body_expr, scope)
    }

    pub(crate) fn evaluate_if_else(
        &self,
        if_else: &rix_parser::ast::IfElse,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let condition_expr = if_else.condition().ok_or_else(|| Error::UnsupportedExpression {
            reason: "if: missing condition".to_string(),
        })?;

        let condition_forced = self.evaluate_expr_with_scope(&condition_expr, scope)?.force(self)?;
        let is_truthy = match condition_forced {
            NixValue::Boolean(false) | NixValue::Null => false,
            _ => true,
        };

        if is_truthy {
            let then_expr = if_else.body().unwrap();
            self.evaluate_expr_with_scope(&then_expr, scope)
        } else {
            let else_expr = if_else.else_body().unwrap();
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
            let name = &path_str[1..path_str.len()-1];
            if let Some(p) = self.search_paths.get(name) {
                return Ok(NixValue::Path(p.clone()));
            }
            return Err(Error::UnsupportedExpression { reason: format!("unknown search path: {}", name) });
        }
        let file_path = if path_str.starts_with('/') {
            PathBuf::from(path_str)
        } else {
            self.current_file_path().map(|p| p.parent().unwrap().join(&path_str)).unwrap_or_else(|| PathBuf::from(path_str))
        };
        Ok(NixValue::Path(file_path))
    }

    pub(crate) fn evaluate_select(
        &self,
        select: &rix_parser::ast::Select,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let expr = select.expr().unwrap();
        let mut current_value = self.evaluate_expr_with_scope(&expr, scope)?;
        let attrpath = select.attrpath().unwrap();

        for attr_node in attrpath.attrs() {
            let attr_name = if let Some(ident) = rix_parser::ast::Ident::cast(attr_node.syntax().clone()) {
                ident.to_string()
            } else if let Some(expr) = rix_parser::ast::Expr::cast(attr_node.syntax().clone()) {
                self.evaluate_expr_with_scope(&expr, scope)?.force(self)?.as_string()?
            } else {
                attr_node.syntax().text().to_string().trim_matches('"').to_string()
            };

            match current_value.clone().force(self)? {
                NixValue::AttributeSet(mut attrs) => {
                    if let Some(value) = attrs.remove(&attr_name) {
                        current_value = value;
                    } else {
                        if let Some(default_expr) = select.default_expr() {
                            return self.evaluate_expr_with_scope(&default_expr, scope);
                        }
                        return Err(Error::UnsupportedExpression { reason: format!("attr {} not found", attr_name) });
                    }
                }
                _ => return Err(Error::UnsupportedExpression { reason: "select from non-attrset".to_string() }),
            }
        }
        current_value.force(self)
    }

    pub(crate) fn evaluate_has_attr(
        &self,
        has_attr: &rix_parser::ast::HasAttr,
        scope: &VariableScope,
    ) -> Result<NixValue> {
        let expr = has_attr.expr().unwrap();
        let mut current_value = self.evaluate_expr_with_scope(&expr, scope)?;
        let attrpath = has_attr.attrpath().unwrap();

        for attr_node in attrpath.attrs() {
            let attr_name = if let Some(ident) = rix_parser::ast::Ident::cast(attr_node.syntax().clone()) {
                ident.to_string()
            } else if let Some(expr) = rix_parser::ast::Expr::cast(attr_node.syntax().clone()) {
                self.evaluate_expr_with_scope(&expr, scope)?.force(self)?.as_string()?
            } else {
                attr_node.syntax().text().to_string().trim_matches('"').to_string()
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

    pub(crate) fn evaluate_paren(&self, paren: &rix_parser::ast::Paren, scope: &VariableScope) -> Result<NixValue> {
        self.evaluate_expr_with_scope(&paren.expr().unwrap(), scope)
    }
}
