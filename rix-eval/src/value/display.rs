//! Display and equality implementations for NixValue

use crate::value::NixValue;
use std::fmt;
use std::sync::Arc;

/// Format a Nix value as a string
impl fmt::Display for NixValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NixValue::String(s) => {
                // Escape special characters in strings for display
                let escaped: String = s
                    .chars()
                    .map(|c| match c {
                        '\n' => "\\n".to_string(),
                        '\t' => "\\t".to_string(),
                        '\r' => "\\r".to_string(),
                        '"' => "\\\"".to_string(),
                        '\\' => "\\\\".to_string(),
                        _ => c.to_string(),
                    })
                    .collect();
                write!(f, "\"{}\"", escaped)
            }
            NixValue::Integer(i) => write!(f, "{}", i),
            NixValue::Float(fl) => {
                // Nix displays floats with a maximum of 5 significant digits,
                // trimming trailing zeros and decimal point.
                let formatted = format!("{:.5}", fl);
                let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
                write!(f, "{}", trimmed)
            }
            NixValue::Boolean(b) => write!(f, "{}", b),
            NixValue::Null => write!(f, "null"),
            NixValue::AttributeSet(attrs) => {
                let mut sorted_keys: Vec<&String> = attrs.keys().collect();
                sorted_keys.sort();
                let entries: Vec<String> = sorted_keys
                    .iter()
                    .map(|k| {
                        let needs_quoting = k.is_empty()
                            || k.chars()
                                .next()
                                .map(|c| !c.is_ascii_alphabetic() && c != '_')
                                .unwrap_or(true)
                            || k.chars().any(|c| {
                                !c.is_ascii_alphanumeric() && c != '_' && c != '-' && c != '\''
                            });

                        let keywords = [
                            "if", "then", "else", "assert", "with", "let", "in", "rec", "inherit",
                        ];
                        let needs_quoting = needs_quoting || keywords.contains(&k.as_str());

                        let key_disp = if needs_quoting {
                            format!("\"{}\"", k)
                        } else {
                            k.to_string()
                        };
                        format!("{} = {};", key_disp, attrs[*k])
                    })
                    .collect();
                if entries.is_empty() {
                    write!(f, "{{ }}")
                } else {
                    write!(f, "{{ {} }}", entries.join(" "))
                }
            }
            NixValue::List(items) => {
                if items.is_empty() {
                    write!(f, "[ ]")
                } else {
                    let items_str: Vec<String> = items.iter().map(|v| format!("{}", v)).collect();
                    write!(f, "[ {} ]", items_str.join(" "))
                }
            }
            NixValue::Thunk(_) => {
                write!(f, "<thunk>")
            }
            NixValue::Function(func) => {
                // Curried builtins display as <PRIMOP-APP>
                if func.body_text().starts_with("__curried_builtin_call:")
                    || func.body_text() == "__curried_foldl_call"
                {
                    write!(f, "<PRIMOP-APP>")
                } else {
                    write!(f, "<LAMBDA>")
                }
            }
            NixValue::Path(path) => {
                let path_str = path.to_string_lossy().replace('\\', "/");
                write!(f, "{}", path_str)
            }
            NixValue::StorePath(path) => {
                write!(f, "{}", path)
            }
            NixValue::Derivation(drv) => {
                write!(f, "<derivation {}>", drv.name)
            }
            NixValue::DeferredLookup(name, _) => {
                write!(f, "<deferred lookup: {}>", name)
            }
            NixValue::DeferredInherit(_, name) => {
                write!(f, "<deferred inherit: {}>", name)
            }
            NixValue::Builtin(_name) => {
                write!(f, "<PRIMOP>")
            }
        }
    }
}

impl PartialEq for NixValue {
    fn eq(&self, other: &Self) -> bool {
        // For thunks and functions, we compare by pointer identity since forcing/applying requires an evaluator
        // In practice, thunks should be forced before comparison, and functions are compared by identity
        match (self, other) {
            (NixValue::Thunk(a), NixValue::Thunk(b)) => Arc::ptr_eq(a, b),
            (NixValue::Function(a), NixValue::Function(b)) => Arc::ptr_eq(a, b),
            (NixValue::String(a), NixValue::String(b)) => a == b,
            (NixValue::Integer(a), NixValue::Integer(b)) => a == b,
            (NixValue::Float(a), NixValue::Float(b)) => a == b,
            (NixValue::Boolean(a), NixValue::Boolean(b)) => a == b,
            (NixValue::Null, NixValue::Null) => true,
            (NixValue::List(a), NixValue::List(b)) => a == b,
            (NixValue::AttributeSet(a), NixValue::AttributeSet(b)) => a == b,
            (NixValue::Path(a), NixValue::Path(b)) => a == b,
            (NixValue::StorePath(a), NixValue::StorePath(b)) => a == b,
            (NixValue::Derivation(a), NixValue::Derivation(b)) => Arc::ptr_eq(a, b),
            (NixValue::DeferredLookup(a_name, _), NixValue::DeferredLookup(b_name, _)) => {
                a_name == b_name
            }
            (NixValue::DeferredInherit(_, a_name), NixValue::DeferredInherit(_, b_name)) => {
                a_name == b_name
            }
            (NixValue::Builtin(a), NixValue::Builtin(b)) => a == b,
            _ => false,
        }
    }
}
