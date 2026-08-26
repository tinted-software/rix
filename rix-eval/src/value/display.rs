//! Display and equality implementations for NixValue

use crate::value::NixValue;
use std::cell::Cell;
use std::fmt;
use std::sync::Arc;

thread_local! {
    /// Recursion depth guard for Display to prevent stack overflow on recursive structures
    static DISPLAY_DEPTH: Cell<usize> = Cell::new(0);
}

const MAX_DISPLAY_DEPTH: usize = 20;

/// Format a Nix value as a string
impl fmt::Display for NixValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Guard against infinite recursion on cyclic structures
        let depth = DISPLAY_DEPTH.with(|d| {
            let current = d.get();
            if current > MAX_DISPLAY_DEPTH {
                return Err(fmt::Error);
            }
            d.set(current + 1);
            Ok(current)
        });

        let depth = match depth {
            Ok(d) => d,
            Err(_) => return write!(f, "<cycle>"),
        };

        let result = self.fmt_inner(f);
        DISPLAY_DEPTH.with(|d| d.set(depth));
        result
    }
}

fn format_nix_float(f: f64) -> String {
    if f == 0.0 {
        return "0".to_string();
    }
    let abs = f.abs();
    // Nix/C sprintf("%.6g") format:
    // Uses scientific notation if exponent is < -4 or >= precision (6).
    // Otherwise uses standard decimal notation with up to 6 significant digits.
    if abs >= 1e-4 && abs < 1e6 {
        // Find how many decimal digits needed for up to 6 significant digits
        let magnitude = f.abs().log10().floor() as i32;
        let decimals = (5 - magnitude).max(0) as usize;
        let s = format!("{:.decimals$}", f, decimals = decimals);
        if s.contains('.') {
            let trimmed = s.trim_end_matches('0').trim_end_matches('.');
            trimmed.to_string()
        } else {
            s
        }
    } else {
        // Scientific notation: e+XX or e-XX, 1 digit before decimal point, up to 5 after
        // e.g. 5e+22, 6.626e-34, 9.22462e+06
        let s = format!("{:.5e}", f);
        if let Some((mantissa, exp)) = s.split_once('e') {
            let trimmed_mantissa = if mantissa.contains('.') {
                mantissa.trim_end_matches('0').trim_end_matches('.')
            } else {
                mantissa
            };
            let exp_num: i32 = exp.parse().unwrap_or(0);
            format!("{}e{:+03}", trimmed_mantissa, exp_num)
        } else {
            s
        }
    }
}

fn escape_nix_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '$' if chars.peek() == Some(&'{') => {
                out.push_str("\\$");
            }
            _ => out.push(c),
        }
    }
    out
}

impl NixValue {
    fn fmt_inner(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NixValue::String(s) => {
                write!(f, "\"{}\"", escape_nix_string(s))
            }
            NixValue::Integer(i) => write!(f, "{}", i),
            NixValue::Float(fl) => {
                write!(f, "{}", format_nix_float(*fl))
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
                            format!("\"{}\"", escape_nix_string(k))
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
