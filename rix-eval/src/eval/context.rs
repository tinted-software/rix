//! Evaluation context and scope management

use crate::value::NixValue;
use codespan::FileId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Represents a variable scope for name resolution
///
/// A scope maps variable names to their values. Scopes can be nested,
/// with inner scopes shadowing outer scopes.
///
/// Support for Nix scoping rules:
/// - Lexical variables take precedence.
/// - 'with' expressions provide a lazy fallback stack.
/// - 'rec' and 'let' bindings provide a shared recursive scope.
#[derive(Debug, Clone)]
pub struct VariableScope {
    /// Lexical variables (e.g. from 'let', function arguments)
    vars: HashMap<String, NixValue>,
    /// Stack of 'with' attribute sets (lazy fallback)
    /// We use NixValue::Thunk to keep them lazy.
    withs: Vec<NixValue>,
    /// Shared recursive scope (used by 'rec' and 'let' for mutual recursion)
    recursive: Option<Arc<Mutex<HashMap<String, NixValue>>>>,
}

impl VariableScope {
    /// Create a new empty scope
    pub fn new() -> Self {
        Self {
            vars: HashMap::new(),
            withs: Vec::new(),
            recursive: None,
        }
    }

    /// Set the shared recursive map for this scope
    pub fn set_recursive(&mut self, rec: Arc<Mutex<HashMap<String, NixValue>>>) {
        self.recursive = Some(rec);
    }

    /// Add a 'with' attribute set to the stack
    pub fn push_with(&mut self, with_set: NixValue) {
        self.withs.push(with_set);
    }

    /// Look up a variable in the scope according to Nix rules
    ///
    /// NOTE: This only checks lexical and recursive variables.
    /// 'with' lookup requires an Evaluator and is handled in evaluator.rs.
    pub fn get(&self, name: &str) -> Option<NixValue> {
        // 1. Check lexical variables (take precedence)
        if let Some(v) = self.vars.get(name) {
            return Some(v.clone());
        }

        // 2. Check recursive shared scope (for mutual recursion)
        if let Some(ref rec) = self.recursive {
            if let Ok(map) = rec.lock() {
                if let Some(v) = map.get(name) {
                    return Some(v.clone());
                }
            }
        }

        None
    }

    /// Insert a lexical variable
    pub fn insert(&mut self, name: String, value: NixValue) {
        self.vars.insert(name, value);
    }

    /// Get reference to lexical variables
    pub fn vars(&self) -> &HashMap<String, NixValue> {
        &self.vars
    }

    /// Get mutable reference to lexical variables (for compatibility)
    pub fn vars_mut(&mut self) -> &mut HashMap<String, NixValue> {
        &mut self.vars
    }

    /// Get with stack
    pub fn withs(&self) -> &[NixValue] {
        &self.withs
    }

    /// Remove a variable
    pub fn remove(&mut self, name: &str) -> Option<NixValue> {
        self.vars.remove(name)
    }

    /// Length of lexical variables
    pub fn len(&self) -> usize {
        self.vars.len()
    }

    /// check if empty
    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }
}

impl Default for VariableScope {
    fn default() -> Self {
        Self::new()
    }
}

impl From<HashMap<String, NixValue>> for VariableScope {
    fn from(vars: HashMap<String, NixValue>) -> Self {
        Self {
            vars,
            withs: Vec::new(),
            recursive: None,
        }
    }
}

/// Evaluation context for tracking file and scope information
///
/// This struct stores the context needed for evaluation, including
/// the file ID (for source tracking) and the variable scope.
#[derive(Debug, Clone)]
pub struct EvaluationContext {
    /// File ID in the source map (None for expressions without a file)
    pub file_id: Option<FileId>,
    /// Variable scope for this context
    pub scope: VariableScope,
}
