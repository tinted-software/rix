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
///   Represents a single layer in the variable scope stack
#[derive(Debug, Clone)]
pub enum ScopeLayer {
    /// Regular lexical variables (e.g. from function arguments)
    Lexical(HashMap<String, NixValue>),
    /// Shared recursive variables (e.g. from 'rec' or 'let')
    Recursive(Arc<Mutex<HashMap<String, NixValue>>>),
}

/// Represents a variable scope for name resolution
///
/// A scope consists of a stack of layers (lexical or recursive) and a stack
/// of 'with' attribute sets. Name resolution follows the stack from top to bottom.
#[derive(Debug, Clone)]
pub struct VariableScope {
    /// Stack of scope layers (lexical and recursive)
    layers: Vec<ScopeLayer>,
    /// Stack of 'with' attribute sets (lazy fallback)
    withs: Vec<NixValue>,
}

impl VariableScope {
    /// Create a new empty scope
    pub fn new() -> Self {
        Self {
            layers: vec![ScopeLayer::Lexical(HashMap::new())],
            withs: Vec::new(),
        }
    }

    /// Add a shared recursive map to this scope
    pub fn push_recursive(&mut self, rec: Arc<Mutex<HashMap<String, NixValue>>>) {
        self.layers.push(ScopeLayer::Recursive(rec));
    }

    /// Add a new lexical layer to this scope
    pub fn push_lexical(&mut self) {
        self.layers.push(ScopeLayer::Lexical(HashMap::new()));
    }

    /// Add a 'with' attribute set to the stack
    pub fn push_with(&mut self, with_set: NixValue) {
        self.withs.push(with_set);
    }

    /// Look up a variable in the scope according to Nix rules
    ///
    /// Checks layers from innermost to outermost.
    pub fn get(&self, name: &str) -> Option<NixValue> {
        for layer in self.layers.iter().rev() {
            match layer {
                ScopeLayer::Lexical(vars) => {
                    if let Some(v) = vars.get(name) {
                        return Some(v.clone());
                    }
                }
                ScopeLayer::Recursive(mutex) => {
                    if let Ok(map) = mutex.lock()
                        && let Some(v) = map.get(name)
                    {
                        return Some(v.clone());
                    }
                }
            }
        }
        None
    }

    /// Insert a lexical variable into the current (innermost) layer
    pub fn insert(&mut self, name: String, value: NixValue) {
        // Find the topmost lexical layer, or create one if none exists
        if let Some(ScopeLayer::Lexical(vars)) = self.layers.last_mut() {
            vars.insert(name, value);
        } else {
            let mut vars = HashMap::new();
            vars.insert(name, value);
            self.layers.push(ScopeLayer::Lexical(vars));
        }
    }

    /// Get all lexical variables in the current layer
    pub fn current_vars(&self) -> Option<&HashMap<String, NixValue>> {
        if let Some(ScopeLayer::Lexical(vars)) = self.layers.last() {
            Some(vars)
        } else {
            None
        }
    }

    /// Get with stack
    pub fn withs(&self) -> &[NixValue] {
        &self.withs
    }

    /// Length of the scope stack
    pub fn depth(&self) -> usize {
        self.layers.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
            || (self.layers.len() == 1
                && match &self.layers[0] {
                    ScopeLayer::Lexical(vars) => vars.is_empty(),
                    _ => false,
                })
    }
}

impl Default for VariableScope {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for VariableScope {
    fn drop(&mut self) {
        let mut to_drop = Vec::new();
        for layer in self.layers.drain(..) {
            match layer {
                ScopeLayer::Lexical(mut map) => {
                    for (_, v) in map.drain() {
                        to_drop.push(v);
                    }
                }
                ScopeLayer::Recursive(mutex) => {
                    if let Ok(mutex) = Arc::try_unwrap(mutex) {
                        if let Ok(mut map) = mutex.into_inner() {
                            for (_, v) in map.drain() {
                                to_drop.push(v);
                            }
                        }
                    }
                }
            }
        }
        while let Some(v) = to_drop.pop() {
            if let NixValue::Thunk(thunk) = v {
                if let Ok(mut thunk_inner) = Arc::try_unwrap(thunk) {
                    for layer in thunk_inner.closure.layers.drain(..) {
                        match layer {
                            ScopeLayer::Lexical(mut map) => {
                                for (_, v) in map.drain() {
                                    to_drop.push(v);
                                }
                            }
                            ScopeLayer::Recursive(mutex) => {
                                if let Ok(mutex) = Arc::try_unwrap(mutex) {
                                    if let Ok(mut map) = mutex.into_inner() {
                                        for (_, v) in map.drain() {
                                            to_drop.push(v);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

impl From<HashMap<String, NixValue>> for VariableScope {
    fn from(vars: HashMap<String, NixValue>) -> Self {
        Self {
            layers: vec![ScopeLayer::Lexical(vars)],
            withs: Vec::new(),
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
