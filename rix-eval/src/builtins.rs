//! Builtin functions for the Nix evaluator
//!
//! This module provides implementations of Nix builtin functions that can be
//! registered with the evaluator.

use crate::VariableScope;
use crate::builtin::Builtin;
use crate::error::{Error, Result};
use crate::eval::Evaluator;
use crate::value::NixValue;
use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Import builtin function
///
/// Imports and evaluates a Nix file. The argument must be a path value.
pub struct ImportBuiltin;

impl Builtin for ImportBuiltin {
    fn name(&self) -> &str {
        "import"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("import takes 1 argument, got {}", args.len()),
            });
        }

        let forced = args[0].clone().force(evaluator)?;
        let path = match forced {
            NixValue::Path(p) => p,
            NixValue::StorePath(p) => std::path::PathBuf::from(p),
            NixValue::String(s) => std::path::PathBuf::from(s),
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("import expects a path, got {}", forced),
                });
            }
        };

        evaluator.evaluate_from_file(&path)
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "import requires evaluator context".to_string(),
        })
    }
}

/// Type checking builtins
pub struct IsNullBuiltin;
impl Builtin for IsNullBuiltin {
    fn name(&self) -> &str {
        "isNull"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("isNull takes 1 argument, got {}", args.len()),
            });
        }
        let forced = args[0].clone().force(evaluator)?;
        Ok(NixValue::Boolean(matches!(forced, NixValue::Null)))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "isNull requires evaluator context".to_string(),
        })
    }
}

pub struct IsBoolBuiltin;
impl Builtin for IsBoolBuiltin {
    fn name(&self) -> &str {
        "isBool"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("isBool takes 1 argument, got {}", args.len()),
            });
        }
        let forced = args[0].clone().force(evaluator)?;
        Ok(NixValue::Boolean(matches!(forced, NixValue::Boolean(_))))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "isBool requires evaluator context".to_string(),
        })
    }
}

pub struct IsIntBuiltin;
impl Builtin for IsIntBuiltin {
    fn name(&self) -> &str {
        "isInt"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("isInt takes 1 argument, got {}", args.len()),
            });
        }
        let forced = args[0].clone().force(evaluator)?;
        Ok(NixValue::Boolean(matches!(forced, NixValue::Integer(_))))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "isInt requires evaluator context".to_string(),
        })
    }
}

pub struct IsFloatBuiltin;
impl Builtin for IsFloatBuiltin {
    fn name(&self) -> &str {
        "isFloat"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("isFloat takes 1 argument, got {}", args.len()),
            });
        }
        let forced = args[0].clone().force(evaluator)?;
        Ok(NixValue::Boolean(matches!(forced, NixValue::Float(_))))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "isFloat requires evaluator context".to_string(),
        })
    }
}

pub struct IsStringBuiltin;
impl Builtin for IsStringBuiltin {
    fn name(&self) -> &str {
        "isString"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("isString takes 1 argument, got {}", args.len()),
            });
        }
        let forced = args[0].clone().force(evaluator)?;
        Ok(NixValue::Boolean(matches!(forced, NixValue::String(_))))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "isString requires evaluator context".to_string(),
        })
    }
}

pub struct IsPathBuiltin;
impl Builtin for IsPathBuiltin {
    fn name(&self) -> &str {
        "isPath"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("isPath takes 1 argument, got {}", args.len()),
            });
        }
        let forced = args[0].clone().force(evaluator)?;
        Ok(NixValue::Boolean(matches!(
            forced,
            NixValue::Path(_) | NixValue::StorePath(_)
        )))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "isPath requires evaluator context".to_string(),
        })
    }
}

pub struct IsListBuiltin;
impl Builtin for IsListBuiltin {
    fn name(&self) -> &str {
        "isList"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("isList takes 1 argument, got {}", args.len()),
            });
        }
        let forced = args[0].clone().force(evaluator)?;
        Ok(NixValue::Boolean(matches!(forced, NixValue::List(_))))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "isList requires evaluator context".to_string(),
        })
    }
}

pub struct IsAttrsBuiltin;
impl Builtin for IsAttrsBuiltin {
    fn name(&self) -> &str {
        "isAttrs"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("isAttrs takes 1 argument, got {}", args.len()),
            });
        }
        let forced = args[0].clone().force(evaluator)?;
        Ok(NixValue::Boolean(matches!(
            forced,
            NixValue::AttributeSet(_)
        )))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "isAttrs requires evaluator context".to_string(),
        })
    }
}

/// IsFunction builtin - checks if a value is a function
pub struct IsFunctionBuiltin;
impl Builtin for IsFunctionBuiltin {
    fn name(&self) -> &str {
        "isFunction"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("isFunction takes 1 argument, got {}", args.len()),
            });
        }
        let forced = args[0].clone().force(evaluator)?;
        Ok(NixValue::Boolean(matches!(forced, NixValue::Function(_))))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "isFunction requires evaluator context".to_string(),
        })
    }
}

/// StringLength builtin - returns the length of a string (alias for length)
pub struct StringLengthBuiltin;
impl Builtin for StringLengthBuiltin {
    fn name(&self) -> &str {
        "stringLength"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("stringLength takes 1 argument, got {}", args.len()),
            });
        }
        let forced = args[0].clone().force(evaluator)?;
        let len = match forced {
            NixValue::String(s) => s.len(),
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("stringLength expects a string, got {}", forced),
                });
            }
        };
        Ok(NixValue::Integer(len as i64))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "stringLength requires evaluator context".to_string(),
        })
    }
}

/// Seq builtin - forces evaluation of first argument, returns second argument
///
/// `builtins.seq a b` evaluates `a` (forcing any thunks) and then returns `b`.
/// This is used for strict evaluation in otherwise lazy contexts.
pub struct SeqBuiltin;

impl Builtin for SeqBuiltin {
    fn name(&self) -> &str {
        "seq"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("seq takes 2 arguments, got {}", args.len()),
            });
        }
        // Force the first argument (evaluate any thunks)
        let _ = args[0].clone().force(evaluator)?;
        // Return the second argument
        Ok(args[1].clone())
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "seq requires evaluator context".to_string(),
        })
    }
}

/// Elem builtin - checks if an element is in a list
pub struct ElemBuiltin;
impl Builtin for ElemBuiltin {
    fn name(&self) -> &str {
        "elem"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("elem takes 2 arguments, got {}", args.len()),
            });
        }

        let x = args[0].clone().force(evaluator)?;
        let xs_val = args[1].clone().force(evaluator)?;
        let xs = match xs_val {
            NixValue::List(l) => l,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("elem: second argument must be a list, got {}", xs_val),
                });
            }
        };

        for item in xs {
            let item_forced = item.force(evaluator)?;
            if evaluator.evaluate_equal(&x, &item_forced)? == NixValue::Boolean(true) {
                return Ok(NixValue::Boolean(true));
            }
        }

        Ok(NixValue::Boolean(false))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "elem requires evaluator context".to_string(),
        })
    }
}

/// IntersectAttrs builtin - returns the intersection of two attribute sets
///
/// `builtins.intersectAttrs e1 e2` returns an attribute set containing only the attributes
/// that are present in both `e1` and `e2`, with values from `e2`.
/// IntersectAttrs builtin - returns the intersection of two attribute sets
pub struct IntersectAttrsBuiltin;
impl Builtin for IntersectAttrsBuiltin {
    fn name(&self) -> &str {
        "intersectAttrs"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("intersectAttrs takes 2 arguments, got {}", args.len()),
            });
        }

        let e1_val = args[0].clone().force(evaluator)?;
        let e1 = match e1_val {
            NixValue::AttributeSet(a) => a,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!(
                        "intersectAttrs: first argument must be an attribute set, got {}",
                        e1_val
                    ),
                });
            }
        };

        let e2_val = args[1].clone().force(evaluator)?;
        let e2 = match e2_val {
            NixValue::AttributeSet(a) => a,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!(
                        "intersectAttrs: second argument must be an attribute set, got {}",
                        e2_val
                    ),
                });
            }
        };

        let mut result = HashMap::new();
        for (key, value) in e2 {
            if e1.contains_key(&key) {
                result.insert(key, value);
            }
        }

        Ok(NixValue::AttributeSet(result))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "intersectAttrs requires evaluator context".to_string(),
        })
    }
}

/// ElemAt builtin - gets an element from a list by index
///
/// `builtins.elemAt xs n` returns the element at index `n` (0-based) in list `xs`.
/// Note: The argument order in Nix is `elemAt list index`, not `elemAt index list`.
pub struct ElemAtBuiltin;
impl Builtin for ElemAtBuiltin {
    fn name(&self) -> &str {
        "elemAt"
    }
    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("elemAt takes 2 arguments, got {}", args.len()),
            });
        }
        // In Nix, elemAt is called as: elemAt list index
        // So args[0] is the list, args[1] is the index
        match &args[0] {
            NixValue::List(list) => {
                let index = match &args[1] {
                    NixValue::Integer(i) => {
                        if *i < 0 {
                            return Err(Error::UnsupportedExpression {
                                reason: format!("elemAt: index must be non-negative, got {}", i),
                            });
                        }
                        *i as usize
                    }
                    _ => {
                        return Err(Error::UnsupportedExpression {
                            reason: format!(
                                "elemAt: second argument must be an integer, got {}",
                                args[1]
                            ),
                        });
                    }
                };
                if index >= list.len() {
                    return Err(Error::UnsupportedExpression {
                        reason: format!(
                            "elemAt: index {} out of bounds for list of length {}",
                            index,
                            list.len()
                        ),
                    });
                }
                Ok(list[index].clone())
            }
            _ => Err(Error::UnsupportedExpression {
                reason: format!("elemAt: first argument must be a list, got {}", args[0]),
            }),
        }
    }
}

/// TypeOf builtin - returns the type of a value as a string
pub struct TypeOfBuiltin;
impl Builtin for TypeOfBuiltin {
    fn name(&self) -> &str {
        "typeOf"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("typeOf takes 1 argument, got {}", args.len()),
            });
        }
        let forced = args[0].clone().force(evaluator)?;
        let type_name = match forced {
            NixValue::Integer(_) => "int",
            NixValue::Float(_) => "float",
            NixValue::Boolean(_) => "bool",
            NixValue::Builtin(_) => "lambda",
            NixValue::String(_) => "string",
            NixValue::Null => "null",
            NixValue::List(_) => "list",
            NixValue::AttributeSet(_) => "set",
            NixValue::Path(_) => "path",
            NixValue::StorePath(_) => "path",
            NixValue::Function(_) => "lambda",
            NixValue::Derivation(_) => "set",
            _ => "thunk", // Should be unreachable after force
        };
        Ok(NixValue::String(type_name.to_string()))
    }
    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "typeOf requires evaluator context".to_string(),
        })
    }
}

/// ToString builtin - converts a value to a string
pub struct ToStringBuiltin;
impl Builtin for ToStringBuiltin {
    fn name(&self) -> &str {
        "toString"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("toString takes 1 argument, got {}", args.len()),
            });
        }
        let forced = args[0].clone().force(evaluator)?;
        let str_value = self.to_string_inner(&forced, evaluator)?;
        Ok(NixValue::String(str_value))
    }
    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "toString requires evaluator context".to_string(),
        })
    }
}

impl ToStringBuiltin {
    fn to_string_inner(&self, value: &NixValue, evaluator: &Evaluator) -> Result<String> {
        match value {
            NixValue::String(s) => Ok(s.clone()),
            NixValue::Integer(i) => Ok(i.to_string()),
            NixValue::Float(f) => {
                // Nix toString uses 6 decimal places for floats (like std::to_string)
                Ok(format!("{:.6}", f))
            }
            NixValue::Boolean(b) => {
                if *b {
                    Ok("1".to_string())
                } else {
                    Ok("".to_string())
                }
            }
            NixValue::Null => Ok("".to_string()),
            NixValue::Path(p) => Ok(p.display().to_string()),
            NixValue::StorePath(p) => Ok(p.clone()),
            NixValue::Derivation(drv) => Ok(format!("<derivation {}>", drv.name)),
            NixValue::Function(_) | NixValue::Builtin(_) => {
                Ok(format!("{}", value))
            }
            NixValue::List(items) => {
                let mut parts = Vec::new();
                for item in items {
                    let forced = item.clone().force(evaluator)?;
                    parts.push(self.to_string_inner(&forced, evaluator)?);
                }
                Ok(parts.join(" "))
            }
            NixValue::AttributeSet(attrs) => {
                // Check for __toString attribute first
                if let Some(ts) = attrs.get("__toString") {
                    let ts_forced = ts.clone().force(evaluator)?;
                    // Call the self: body pattern
                    let result = ts_forced.apply(evaluator, NixValue::AttributeSet(attrs.clone()))?;
                    let result_forced = result.force(evaluator)?;
                    return self.to_string_inner(&result_forced, evaluator);
                }
                // Check for outPath attribute
                if let Some(outpath) = attrs.get("outPath") {
                    let outpath_forced = outpath.clone().force(evaluator)?;
                    return self.to_string_inner(&outpath_forced, evaluator);
                }
                // Default: format as attribute set
                Ok(format!("{}", value))
            }
            NixValue::Thunk(_) => {
                let forced = value.clone().force(evaluator)?;
                self.to_string_inner(&forced, evaluator)
            }
            _ => {
                Ok(format!("{}", value))
            }
        }
    }
}

/// Length builtin - returns the length of a list or string
pub struct LengthBuiltin;
impl Builtin for LengthBuiltin {
    fn name(&self) -> &str {
        "length"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("length takes 1 argument, got {}", args.len()),
            });
        }
        let forced = args[0].clone().force(evaluator)?;
        match forced {
            NixValue::List(l) => Ok(NixValue::Integer(l.len() as i64)),
            _ => Err(Error::UnsupportedExpression {
                reason: format!("length expects a list, got {}", forced),
            }),
        }
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "length requires evaluator context".to_string(),
        })
    }
}

/// Head builtin - returns the first element of a list
pub struct HeadBuiltin;
impl Builtin for HeadBuiltin {
    fn name(&self) -> &str {
        "head"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("head takes 1 argument, got {}", args.len()),
            });
        }
        let forced = args[0].clone().force(evaluator)?;
        match forced {
            NixValue::List(l) => l
                .get(0)
                .cloned()
                .ok_or_else(|| Error::UnsupportedExpression {
                    reason: "head: empty list".to_string(),
                }),
            _ => Err(Error::UnsupportedExpression {
                reason: format!("head expects a list, got {}", forced),
            }),
        }
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "head requires evaluator context".to_string(),
        })
    }
}

/// Tail builtin - returns all but the first element of a list
pub struct TailBuiltin;
impl Builtin for TailBuiltin {
    fn name(&self) -> &str {
        "tail"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("tail takes 1 argument, got {}", args.len()),
            });
        }
        let forced = args[0].clone().force(evaluator)?;
        match forced {
            NixValue::List(l) => {
                if l.is_empty() {
                    return Err(Error::UnsupportedExpression {
                        reason: "tail: empty list".to_string(),
                    });
                }
                Ok(NixValue::List(l[1..].to_vec()))
            }
            _ => Err(Error::UnsupportedExpression {
                reason: format!("tail expects a list, got {}", forced),
            }),
        }
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "tail requires evaluator context".to_string(),
        })
    }
}

/// AttrNames builtin - returns the attribute names of an attribute set as a list
pub struct AttrNamesBuiltin;
impl Builtin for AttrNamesBuiltin {
    fn name(&self) -> &str {
        "attrNames"
    }
    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("attrNames takes 1 argument, got {}", args.len()),
            });
        }
        match &args[0] {
            NixValue::AttributeSet(attrs) => {
                let mut names: Vec<String> = attrs.keys().cloned().collect();
                names.sort(); // Nix returns attribute names in sorted order
                let names_values: Vec<NixValue> =
                    names.into_iter().map(|k| NixValue::String(k)).collect();
                Ok(NixValue::List(names_values))
            }
            _ => Err(Error::UnsupportedExpression {
                reason: format!("attrNames expects an attribute set, got {}", args[0]),
            }),
        }
    }
}

/// AttrValues builtin - returns the attribute values of an attribute set as a list
pub struct AttrValuesBuiltin;
impl Builtin for AttrValuesBuiltin {
    fn name(&self) -> &str {
        "attrValues"
    }
    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("attrValues takes 1 argument, got {}", args.len()),
            });
        }
        match &args[0] {
            NixValue::AttributeSet(attrs) => {
                // Get keys, sort them, then collect values in sorted key order
                let mut keys: Vec<String> = attrs.keys().cloned().collect();
                keys.sort(); // Nix returns attribute values in sorted key order
                // Note: attrValues requires evaluator context to force thunks
                // This is handled specially in evaluate_apply
                let values: Vec<NixValue> =
                    keys.iter().map(|k| attrs.get(k).unwrap().clone()).collect();
                Ok(NixValue::List(values))
            }
            _ => Err(Error::UnsupportedExpression {
                reason: format!("attrValues expects an attribute set, got {}", args[0]),
            }),
        }
    }
}

/// CatAttrs builtin - collects an attribute from a list of attribute sets
pub struct CatAttrsBuiltin;
impl Builtin for CatAttrsBuiltin {
    fn name(&self) -> &str {
        "catAttrs"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("catAttrs takes 2 arguments, got {}", args.len()),
            });
        }

        let attr_name_val = args[0].clone().force(evaluator)?;
        let attr_name = match attr_name_val {
            NixValue::String(s) => s,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("catAttrs: first argument must be a string, got {}", attr_name_val),
                });
            }
        };

        let list_val = args[1].clone().force(evaluator)?;
        let list = match list_val {
            NixValue::List(l) => l,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("catAttrs: second argument must be a list, got {}", list_val),
                });
            }
        };

        // Collect the attribute from each attribute set in the list
        let mut result = Vec::new();
        for item in list {
            let item_forced = item.force(evaluator)?;
            match item_forced {
                NixValue::AttributeSet(attrs) => {
                    if let Some(val) = attrs.get(&attr_name) {
                        result.push(val.clone());
                    }
                }
                _ => {
                    return Err(Error::UnsupportedExpression {
                        reason: format!(
                            "catAttrs: each element must be an attribute set, got {}",
                            item_forced
                        ),
                    });
                }
            }
        }

        Ok(NixValue::List(result))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "catAttrs requires evaluator context".to_string(),
        })
    }
}

/// HasAttr builtin - checks if an attribute set has a specific attribute
pub struct HasAttrBuiltin;
impl Builtin for HasAttrBuiltin {
    fn name(&self) -> &str {
        "hasAttr"
    }
    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("hasAttr takes 2 arguments, got {}", args.len()),
            });
        }
        let attr_name = match &args[0] {
            NixValue::String(s) => s,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("hasAttr: first argument must be a string, got {}", args[0]),
                });
            }
        };
        match &args[1] {
            NixValue::AttributeSet(attrs) => Ok(NixValue::Boolean(attrs.contains_key(attr_name))),
            _ => Err(Error::UnsupportedExpression {
                reason: format!(
                    "hasAttr: second argument must be an attribute set, got {}",
                    args[1]
                ),
            }),
        }
    }
}

/// GetAttr builtin - gets an attribute from an attribute set, with optional default
pub struct GetAttrBuiltin;
impl Builtin for GetAttrBuiltin {
    fn name(&self) -> &str {
        "getAttr"
    }
    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() < 2 || args.len() > 3 {
            return Err(Error::UnsupportedExpression {
                reason: format!("getAttr takes 2 or 3 arguments, got {}", args.len()),
            });
        }
        let attr_name = match &args[0] {
            NixValue::String(s) => s,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("getAttr: first argument must be a string, got {}", args[0]),
                });
            }
        };
        match &args[1] {
            NixValue::AttributeSet(attrs) => {
                if let Some(value) = attrs.get(attr_name) {
                    Ok(value.clone())
                } else if args.len() == 3 {
                    // Return default value
                    Ok(args[2].clone())
                } else {
                    Err(Error::UnsupportedExpression {
                        reason: format!("getAttr: attribute '{}' not found", attr_name),
                    })
                }
            }
            _ => Err(Error::UnsupportedExpression {
                reason: format!(
                    "getAttr: second argument must be an attribute set, got {}",
                    args[1]
                ),
            }),
        }
    }
}

/// ConcatLists builtin - concatenates a list of lists
pub struct ConcatListsBuiltin;
impl Builtin for ConcatListsBuiltin {
    fn name(&self) -> &str {
        "concatLists"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("concatLists takes 1 argument, got {}", args.len()),
            });
        }

        let list_val = args[0].clone().force(evaluator)?;
        let list = match list_val {
            NixValue::List(l) => l,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("concatLists expects a list, got {}", list_val),
                });
            }
        };

        let mut result = Vec::new();
        for item in list {
            let inner_list_val = item.force(evaluator)?;
            match inner_list_val {
                NixValue::List(l) => {
                    result.extend(l);
                }
                _ => {
                    return Err(Error::UnsupportedExpression {
                        reason: format!(
                            "concatLists: all elements must be lists, got {}",
                            inner_list_val
                        ),
                    });
                }
            }
        }
        Ok(NixValue::List(result))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "concatLists requires evaluator context".to_string(),
        })
    }
}

/// ConcatStringsSep builtin - concatenates strings with a separator
pub struct ConcatStringsSepBuiltin;
impl Builtin for ConcatStringsSepBuiltin {
    fn name(&self) -> &str {
        "concatStringsSep"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("concatStringsSep takes 2 arguments, got {}", args.len()),
            });
        }
        let sep_val = args[0].clone().force(evaluator)?;
        let separator = match sep_val {
            NixValue::String(ref s) => s.clone(),
            NixValue::Path(ref p) => p.to_string_lossy().to_string(),
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!(
                        "concatStringsSep: first argument must be a string, got {}",
                        sep_val
                    ),
                });
            }
        };
        let list_val = args[1].clone().force(evaluator)?;
        match list_val {
            NixValue::List(strings) => {
                let mut str_values: Vec<String> = Vec::new();
                for v in strings {
                    let forced = v.force(evaluator)?;
                    match forced {
                        NixValue::String(s) => str_values.push(s),
                        other => str_values.push(other.to_string()),
                    }
                }
                let joined = str_values.join(&separator);
                Ok(NixValue::String(joined))
            }
            _ => Err(Error::UnsupportedExpression {
                reason: format!(
                    "concatStringsSep: second argument must be a list, got {}",
                    list_val
                ),
            }),
        }
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "concatStringsSep requires evaluator context".to_string(),
        })
    }
}

/// Abort builtin - aborts evaluation with an error message
pub struct AbortBuiltin;
impl Builtin for AbortBuiltin {
    fn name(&self) -> &str {
        "abort"
    }
    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("abort takes 1 argument, got {}", args.len()),
            });
        }
        let message = match &args[0] {
            NixValue::String(s) => s.clone(),
            _ => format!("{}", args[0]),
        };
        Err(Error::UnsupportedExpression {
            reason: format!("abort: {}", message),
        })
    }
}

/// Trace builtin - prints a message and returns the second argument
pub struct TraceBuiltin;
impl Builtin for TraceBuiltin {
    fn name(&self) -> &str {
        "trace"
    }
    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("trace takes 2 arguments, got {}", args.len()),
            });
        }
        let message = match &args[0] {
            NixValue::String(s) => s.clone(),
            _ => format!("{}", args[0]),
        };
        // In a real implementation, this would print to stderr
        // For now, we'll just return the value
        eprintln!("trace: {}", message);
        Ok(args[1].clone())
    }
}

/// Derivation builtin - creates a derivation (build plan)
///
/// Note: This requires evaluator context to properly handle paths and store paths.
/// For now, this is a placeholder that will need evaluator integration.
pub struct DerivationBuiltin;
impl Builtin for DerivationBuiltin {
    fn name(&self) -> &str {
        "derivation"
    }
    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason:
                "derivation requires evaluator context and must be called via call_with_evaluator"
                    .to_string(),
        })
    }

    fn call_with_evaluator(
        &self,
        args: &[NixValue],
        evaluator: &crate::eval::Evaluator,
    ) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("derivation takes 1 argument, got {}", args.len()),
            });
        }

        // The argument should be an attribute set with derivation attributes
        match &args[0].clone().force(evaluator)? {
            NixValue::AttributeSet(attrs) => {
                // Extract required attributes
                let name_val =
                    attrs
                        .get("name")
                        .cloned()
                        .ok_or_else(|| Error::UnsupportedExpression {
                            reason: "derivation: missing or invalid 'name' attribute".to_string(),
                        })?;
                let name = name_val.force(evaluator)?.as_string()?;

                let system = match attrs.get("system").cloned() {
                    Some(v) => v.force(evaluator)?.as_string()?,
                    None => "unknown".to_string(),
                };

                let builder_val =
                    attrs
                        .get("builder")
                        .cloned()
                        .ok_or_else(|| Error::UnsupportedExpression {
                            reason: "derivation: missing or invalid 'builder' attribute"
                                .to_string(),
                        })?;
                let builder = builder_val.force(evaluator)?.to_string(); // Handles Path/String/etc.

                // Extract optional attributes
                let args_val = attrs.get("args").cloned();
                let args = if let Some(av) = args_val {
                    match av.force(evaluator)? {
                        NixValue::List(l) => {
                            let mut str_args = Vec::new();
                            for item in l {
                                str_args.push(item.force(evaluator)?.as_string()?);
                            }
                            str_args
                        }
                        _ => Vec::new(),
                    }
                } else {
                    Vec::new()
                };

                // Extract environment variables
                let mut env = HashMap::new();
                let mut outputs = HashMap::new();

                // Check for explicit outputs attribute
                if let Some(NixValue::List(output_list)) = attrs.get("outputs") {
                    // Parse outputs list
                    for output in output_list {
                        if let NixValue::String(output_name) = output {
                            // Output paths will be computed after derivation creation
                            outputs.insert(output_name.clone(), String::new());
                        }
                    }
                }

                // If no outputs specified, default to "out"
                if outputs.is_empty() {
                    outputs.insert("out".to_string(), String::new());
                }

                let mut result_attrs_original = HashMap::new();
                for (key, value) in attrs.iter() {
                    let key = key.to_string();
                    if key != "name"
                        && key != "system"
                        && key != "builder"
                        && key != "args"
                        && key != "outputs"
                    {
                        // All other attributes become environment variables
                        let forced_v = value.clone().force(evaluator)?;
                        result_attrs_original.insert(key.clone(), forced_v.clone());
                        let env_value = match forced_v {
                            NixValue::String(s) => s,
                            _ => forced_v.to_string(),
                        };
                        env.insert(key.clone(), env_value);
                    }
                }

                // Create derivation structure
                let mut derivation = crate::Derivation {
                    name: name.clone(),
                    system: system.clone(),
                    builder: builder.clone(),
                    args: args.clone(),
                    env: env.clone(),
                    input_derivations: HashMap::new(),
                    input_sources: Vec::new(),
                    outputs: HashMap::new(), // Will be populated after computing store path
                };

                // Compute store path and write .drv file
                // Note: In a full implementation, we'd also need to:
                // - Compute output paths based on the derivation hash
                // - Handle input derivations and sources properly
                // - Set up the $out environment variable
                let _store_path = derivation.write_to_store()?;

                // Compute output paths (simplified - in reality these depend on the derivation hash)
                // For now, we'll use placeholder paths that would be computed properly
                // in a full implementation
                for output_name in outputs.keys() {
                    // In a real implementation, output paths would be computed as:
                    // /nix/store/<hash>-<name>-<output-name>
                    // For now, we'll leave them empty as they require proper store path computation
                    derivation
                        .outputs
                        .insert(output_name.clone(), String::new());
                }

                // In a real implementation, we'd compute the actual store path here.
                // For now, we'll return an attribute set that looks like a derivation.
                let mut result_attrs = HashMap::new();

                // Copy all original values into the result set
                for (k, v) in result_attrs_original {
                    result_attrs.insert(k, v);
                }

                // Add required derivation attributes
                result_attrs.insert("name".to_string(), NixValue::String(name));
                result_attrs.insert("system".to_string(), NixValue::String(system));
                result_attrs.insert("builder".to_string(), NixValue::String(builder));
                result_attrs.insert(
                    "args".to_string(),
                    NixValue::List(args.into_iter().map(NixValue::String).collect()),
                );
                result_attrs.insert(
                    "type".to_string(),
                    NixValue::String("derivation".to_string()),
                );

                // Add dummy drvPath and outPath (in a real system these would be computed)
                result_attrs.insert(
                    "drvPath".to_string(),
                    NixValue::String("/nix/store/placeholder.drv".to_string()),
                );
                result_attrs.insert(
                    "outPath".to_string(),
                    NixValue::String("/nix/store/placeholder".to_string()),
                );

                Ok(NixValue::AttributeSet(result_attrs))
            }
            _ => Err(Error::UnsupportedExpression {
                reason: format!("derivation expects an attribute set, got {}", args[0]),
            }),
        }
    }
}

/// StorePath builtin - converts a string to a store path value
///
/// Validates that the string is a valid Nix store path and returns it as a StorePath value.
pub struct StorePathBuiltin;

impl Builtin for StorePathBuiltin {
    fn name(&self) -> &str {
        "storePath"
    }
    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("storePath takes 1 argument, got {}", args.len()),
            });
        }

        match &args[0] {
            NixValue::String(path_str) => {
                // Validate that it's a valid store path format
                // Store paths have format: /nix/store/<hash>-<name>
                if !path_str.starts_with("/nix/store/") {
                    return Err(Error::UnsupportedExpression {
                        reason: format!(
                            "storePath: path must start with /nix/store/, got {}",
                            path_str
                        ),
                    });
                }

                // Extract the part after /nix/store/
                let store_part = &path_str[11..]; // Length of "/nix/store/"

                // Find the first `-` which separates hash from name
                if let Some(dash_pos) = store_part.find('-') {
                    let hash = &store_part[..dash_pos];
                    let _name = &store_part[dash_pos + 1..];

                    // Validate hash: should be base32 alphanumeric
                    if hash.is_empty() || !hash.chars().all(|c| c.is_ascii_alphanumeric()) {
                        return Err(Error::UnsupportedExpression {
                            reason: format!("storePath: invalid hash in path {}", path_str),
                        });
                    }

                    Ok(NixValue::StorePath(path_str.clone()))
                } else {
                    Err(Error::UnsupportedExpression {
                        reason: format!(
                            "storePath: invalid store path format, missing hash-name separator: {}",
                            path_str
                        ),
                    })
                }
            }
            _ => Err(Error::UnsupportedExpression {
                reason: format!("storePath expects a string, got {}", args[0]),
            }),
        }
    }
}

/// Path builtin - creates a path value from a string
///
/// In Nix, `builtins.path` can:
/// - Convert a string to a path value
/// - Optionally copy files to the store (with name, filter, etc.)
/// For now, we implement basic string-to-path conversion.
pub struct PathBuiltin;

impl Builtin for PathBuiltin {
    fn name(&self) -> &str {
        "path"
    }
    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() < 1 || args.len() > 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("path takes 1 or 2 arguments, got {}", args.len()),
            });
        }

        match &args[0] {
            NixValue::String(path_str) => {
                // If it's already a store path, return as StorePath
                if path_str.starts_with("/nix/store/") {
                    // Validate store path format
                    let store_part = &path_str[11..];
                    if let Some(dash_pos) = store_part.find('-') {
                        let hash = &store_part[..dash_pos];
                        if !hash.is_empty() && hash.chars().all(|c| c.is_ascii_alphanumeric()) {
                            return Ok(NixValue::StorePath(path_str.clone()));
                        }
                    }
                }

                // Otherwise, convert to a Path value
                use std::path::PathBuf;
                let path = PathBuf::from(path_str);
                Ok(NixValue::Path(path))
            }
            NixValue::Path(_) => {
                // Already a path, return as-is
                Ok(args[0].clone())
            }
            NixValue::StorePath(_) => {
                // Already a store path, return as-is
                Ok(args[0].clone())
            }
            _ => Err(Error::UnsupportedExpression {
                reason: format!("path expects a string or path, got {}", args[0]),
            }),
        }
    }
}

/// BaseNameOf builtin - extracts the base name from a path or string
///
/// `builtins.baseNameOf path` returns the last component of the path.
/// For example: `baseNameOf "/foo/bar"` returns `"bar"`.
pub struct BaseNameOfBuiltin;

impl Builtin for BaseNameOfBuiltin {
    fn name(&self) -> &str {
        "baseNameOf"
    }

    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("baseNameOf takes 1 argument, got {}", args.len()),
            });
        }

        // Convert to string representation, handling Path values specially
        let path_str = match &args[0] {
            NixValue::String(s) => s.clone(),
            NixValue::Path(p) => {
                // For Path values, check if it's the root directory or ends with a component that is "."
                let path_display = p.display().to_string();
                // Check if path is root directory "/" or "."
                if p == std::path::Path::new("/") || p == std::path::Path::new(".") {
                    return Ok(NixValue::String("".to_string()));
                }
                // Check if the last component is "." (like "./.")
                if let Some(last_component) = p.components().last() {
                    if let std::path::Component::CurDir = last_component {
                        // If the path ends with ".", baseNameOf returns ""
                        return Ok(NixValue::String("".to_string()));
                    }
                }
                path_display
            }
            NixValue::StorePath(p) => p.clone(),
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("baseNameOf expects a string or path, got {}", args[0]),
                });
            }
        };

        // Handle empty string
        if path_str.is_empty() {
            return Ok(NixValue::String("".to_string()));
        }

        // If the path ends with '/', it represents a directory and baseNameOf returns ""
        // Check this BEFORE removing trailing slashes
        if path_str.ends_with('/') {
            return Ok(NixValue::String("".to_string()));
        }

        // Remove trailing slashes for normalization
        let cleaned = path_str.trim_end_matches('/').to_string();

        // Handle root path or paths that become empty after removing trailing slashes
        if cleaned == "/" || cleaned.is_empty() {
            return Ok(NixValue::String("".to_string()));
        }

        // Handle current directory - baseNameOf of "." is ""
        if cleaned == "." {
            return Ok(NixValue::String("".to_string()));
        }

        // Extract the last component
        // Split by '/' and take the last non-empty part
        let parts: Vec<&str> = cleaned.split('/').filter(|s| !s.is_empty()).collect();

        if parts.is_empty() {
            // This happens for paths like "///" which become empty after splitting
            Ok(NixValue::String("".to_string()))
        } else {
            Ok(NixValue::String(parts.last().unwrap().to_string()))
        }
    }
}

/// Throw builtin - throws an error with a message
///
/// `builtins.throw msg` throws an error with the given message string.
pub struct ThrowBuiltin;

impl Builtin for ThrowBuiltin {
    fn name(&self) -> &str {
        "throw"
    }

    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("throw takes 1 argument, got {}", args.len()),
            });
        }

        let message = match &args[0] {
            NixValue::String(s) => s.clone(),
            _ => format!("{}", args[0]),
        };

        Err(Error::UnsupportedExpression { reason: message })
    }
}

/// TryEval builtin - evaluates an expression and catches errors
///
/// `builtins.tryEval expr` evaluates `expr` and returns an attribute set with:
/// - `success`: boolean indicating if evaluation succeeded
/// - `value`: the evaluated value (if success) or undefined (if failure)
///
/// This requires evaluator context to evaluate the expression, so it's handled specially in evaluate_apply.
pub struct TryEvalBuiltin;

impl Builtin for TryEvalBuiltin {
    fn name(&self) -> &str {
        "tryEval"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("tryEval takes 1 argument, got {}", args.len()),
            });
        }

        match args[0].clone().force(evaluator) {
            Ok(v) => {
                let mut res = std::collections::HashMap::new();
                res.insert("success".to_string(), NixValue::Boolean(true));
                res.insert("value".to_string(), v);
                Ok(NixValue::AttributeSet(res))
            }
            Err(_) => {
                let mut res = std::collections::HashMap::new();
                res.insert("success".to_string(), NixValue::Boolean(false));
                res.insert("value".to_string(), NixValue::Boolean(false));
                Ok(NixValue::AttributeSet(res))
            }
        }
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "tryEval requires evaluator context".to_string(),
        })
    }
}

/// Map builtin - applies a function to each element of a list
///
/// `builtins.map f list` applies function `f` to each element of `list` and returns a new list.
/// This requires evaluator context to call Nix functions, so it's handled specially in evaluate_apply.
pub struct MapBuiltin;

impl Builtin for MapBuiltin {
    fn name(&self) -> &str {
        "map"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("takes 2 arguments, got {}", args.len()),
            });
        }

        let func = &args[0];
        let list_val = args[1].clone().force(evaluator)?;

        match list_val {
            NixValue::List(list) => {
                let mut results = Vec::new();
                for item in list {
                    let mut thunk_scope = VariableScope::new();
                    thunk_scope.insert("__f".to_string(), func.clone());
                    thunk_scope.insert("__v".to_string(), item);

                    let thunk = NixValue::Thunk(Arc::new(crate::thunk::Thunk::new_from_text(
                        "__f __v".to_string(),
                        thunk_scope,
                        evaluator.current_file_id(),
                    )));
                    results.push(thunk);
                }
                Ok(NixValue::List(results))
            }
            _ => Err(Error::UnsupportedExpression {
                reason: format!("map: second argument must be a list, got {}", list_val),
            }),
        }
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "map requires evaluator context".to_string(),
        })
    }
}

/// ConcatMap builtin - maps a function over a list and concatenates the results
///
/// `builtins.concatMap f list` applies function `f` to each element of `list` and concatenates
/// all the resulting lists into a single list.
/// This requires evaluator context to call Nix functions, so it's handled specially in evaluate_apply.
pub struct ConcatMapBuiltin;

impl Builtin for ConcatMapBuiltin {
    fn name(&self) -> &str {
        "concatMap"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("concatMap takes 2 arguments, got {}", args.len()),
            });
        }
        let func = &args[0];
        let list_val = args[1].clone().force(evaluator)?;
        match list_val {
            NixValue::List(list) => {
                let mut results = Vec::new();
                for item in list {
                    let res = func.clone().apply(evaluator, item.clone())?;
                    let inner_list = res.force(evaluator)?;
                    match inner_list {
                        NixValue::List(items) => results.extend(items),
                        _ => {
                            return Err(Error::UnsupportedExpression {
                                reason: format!(
                                    "concatMap: function must return a list, got {}",
                                    inner_list
                                ),
                            });
                        }
                    }
                }
                Ok(NixValue::List(results))
            }
            _ => Err(Error::UnsupportedExpression {
                reason: format!(
                    "concatMap: second argument must be a list, got {}",
                    list_val
                ),
            }),
        }
    }
    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "concatMap requires evaluator context".to_string(),
        })
    }
}

/// Filter builtin - filters a list using a predicate
pub struct FilterBuiltin;
impl Builtin for FilterBuiltin {
    fn name(&self) -> &str {
        "filter"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("filter takes 2 arguments, got {}", args.len()),
            });
        }
        let func = &args[0];
        let list_val = args[1].clone().force(evaluator)?;
        match list_val {
            NixValue::List(list) => {
                let mut results = Vec::new();
                for item in list {
                    let res = func.clone().apply(evaluator, item.clone())?;
                    match res.force(evaluator)? {
                        NixValue::Boolean(true) => results.push(item),
                        NixValue::Boolean(false) => {}
                        val => {
                            return Err(Error::UnsupportedExpression {
                                reason: format!(
                                    "filter: predicate must return a boolean, got {}",
                                    val
                                ),
                            });
                        }
                    }
                }
                Ok(NixValue::List(results))
            }
            _ => Err(Error::UnsupportedExpression {
                reason: format!("filter: second argument must be a list, got {}", list_val),
            }),
        }
    }
    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "filter requires evaluator context".to_string(),
        })
    }
}

/// All builtin - checks if all elements satisfy a predicate
pub struct AllBuiltin;
impl Builtin for AllBuiltin {
    fn name(&self) -> &str {
        "all"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("all takes 2 arguments, got {}", args.len()),
            });
        }
        let func = &args[0];
        let list_val = args[1].clone().force(evaluator)?;
        match list_val {
            NixValue::List(list) => {
                for item in list {
                    let res = func.clone().apply(evaluator, item.clone())?;
                    match res.force(evaluator)? {
                        NixValue::Boolean(true) => {}
                        NixValue::Boolean(false) => return Ok(NixValue::Boolean(false)),
                        val => {
                            return Err(Error::UnsupportedExpression {
                                reason: format!(
                                    "all: predicate must return a boolean, got {}",
                                    val
                                ),
                            });
                        }
                    }
                }
                Ok(NixValue::Boolean(true))
            }
            _ => Err(Error::UnsupportedExpression {
                reason: format!("all: second argument must be a list, got {}", list_val),
            }),
        }
    }
    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "all requires evaluator context".to_string(),
        })
    }
}

/// Any builtin - checks if any element satisfies a predicate
pub struct AnyBuiltin;
impl Builtin for AnyBuiltin {
    fn name(&self) -> &str {
        "any"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("any takes 2 arguments, got {}", args.len()),
            });
        }
        let func = &args[0];
        let list_val = args[1].clone().force(evaluator)?;
        match list_val {
            NixValue::List(list) => {
                for item in list {
                    let res = func.clone().apply(evaluator, item.clone())?;
                    match res.force(evaluator)? {
                        NixValue::Boolean(true) => return Ok(NixValue::Boolean(true)),
                        NixValue::Boolean(false) => {}
                        val => {
                            return Err(Error::UnsupportedExpression {
                                reason: format!(
                                    "any: predicate must return a boolean, got {}",
                                    val
                                ),
                            });
                        }
                    }
                }
                Ok(NixValue::Boolean(false))
            }
            _ => Err(Error::UnsupportedExpression {
                reason: format!("any: second argument must be a list, got {}", list_val),
            }),
        }
    }
    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "any requires evaluator context".to_string(),
        })
    }
}

/// Sort builtin - sorts a list using a comparison function
pub struct SortBuiltin;
impl Builtin for SortBuiltin {
    fn name(&self) -> &str {
        "sort"
    }
    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("sort takes 2 arguments, got {}", args.len()),
            });
        }
        let func = args[0].clone();
        let list_val = args[1].clone().force(evaluator)?;
        match list_val {
            NixValue::List(list) => {
                let mut results = list.clone();
                let mut err = None;
                results.sort_by(|a, b| {
                    if err.is_some() {
                        return std::cmp::Ordering::Equal;
                    }
                    let res = (|| -> Result<std::cmp::Ordering> {
                        let partial = func.clone().apply(evaluator, a.clone())?;
                        let res = partial.apply(evaluator, b.clone())?;
                        match res.force(evaluator)? {
                            NixValue::Boolean(true) => Ok(std::cmp::Ordering::Less),
                            NixValue::Boolean(false) => Ok(std::cmp::Ordering::Greater),
                            val => Err(Error::UnsupportedExpression {
                                reason: format!(
                                    "sort: predicate must return a boolean, got {}",
                                    val
                                ),
                            }),
                        }
                    })();
                    match res {
                        Ok(ord) => ord,
                        Err(e) => {
                            err = Some(e);
                            std::cmp::Ordering::Equal
                        }
                    }
                });
                if let Some(e) = err {
                    return Err(e);
                }
                Ok(NixValue::List(results))
            }
            _ => Err(Error::UnsupportedExpression {
                reason: format!("sort: second argument must be a list, got {}", list_val),
            }),
        }
    }
    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "sort requires evaluator context".to_string(),
        })
    }
}

/// BitOr builtin - bitwise OR operation on integers
///
/// `builtins.bitOr a b` performs bitwise OR on two integers.
pub struct BitOrBuiltin;

impl Builtin for BitOrBuiltin {
    fn name(&self) -> &str {
        "bitOr"
    }

    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("bitOr takes 2 arguments, got {}", args.len()),
            });
        }

        match (&args[0], &args[1]) {
            (NixValue::Integer(a), NixValue::Integer(b)) => Ok(NixValue::Integer(a | b)),
            _ => Err(Error::UnsupportedExpression {
                reason: format!(
                    "bitOr expects two integers, got {} and {}",
                    args[0], args[1]
                ),
            }),
        }
    }
}

/// BitAnd builtin - bitwise AND operation on integers
///
/// `builtins.bitAnd a b` performs bitwise AND on two integers.
pub struct BitAndBuiltin;

impl Builtin for BitAndBuiltin {
    fn name(&self) -> &str {
        "bitAnd"
    }

    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("bitAnd takes 2 arguments, got {}", args.len()),
            });
        }

        match (&args[0], &args[1]) {
            (NixValue::Integer(a), NixValue::Integer(b)) => Ok(NixValue::Integer(a & b)),
            _ => Err(Error::UnsupportedExpression {
                reason: format!(
                    "bitAnd expects two integers, got {} and {}",
                    args[0], args[1]
                ),
            }),
        }
    }
}

/// BitXor builtin - bitwise XOR operation on integers
///
/// `builtins.bitXor a b` performs bitwise XOR on two integers.
pub struct BitXorBuiltin;

impl Builtin for BitXorBuiltin {
    fn name(&self) -> &str {
        "bitXor"
    }

    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("bitXor takes 2 arguments, got {}", args.len()),
            });
        }

        match (&args[0], &args[1]) {
            (NixValue::Integer(a), NixValue::Integer(b)) => Ok(NixValue::Integer(a ^ b)),
            _ => Err(Error::UnsupportedExpression {
                reason: format!(
                    "bitXor expects two integers, got {} and {}",
                    args[0], args[1]
                ),
            }),
        }
    }
}

/// Foldl' builtin - strict left fold over a list
///
/// `builtins.foldl' op nul list` applies the binary operator `op` to each element of `list`
/// from left to right, starting with `nul` as the initial accumulator.
/// This requires evaluator context to call Nix functions, so it's handled specially in evaluate_apply.
pub struct FoldlStrictBuiltin;

impl Builtin for FoldlStrictBuiltin {
    fn name(&self) -> &str {
        "foldl'"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 3 {
            return Err(Error::UnsupportedExpression {
                reason: format!("foldl' takes 3 arguments, got {}", args.len()),
            });
        }
        let op = args[0].clone();
        let mut acc = args[1].clone();
        let list_val = args[2].clone().force(evaluator)?;

        match list_val {
            NixValue::List(list) => {
                for item in list {
                    // foldl' is strict, so we force the accumulator before applying
                    acc = acc.force(evaluator)?;
                    let res = op.clone().apply(evaluator, acc)?;
                    acc = res.apply(evaluator, item)?;
                }
                Ok(acc)
            }
            _ => Err(Error::UnsupportedExpression {
                reason: format!("foldl': third argument must be a list, got {}", list_val),
            }),
        }
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "foldl' requires evaluator context".to_string(),
        })
    }
}

/// Add builtin - adds two numbers
pub struct AddBuiltin;

impl Builtin for AddBuiltin {
    fn name(&self) -> &str {
        "add"
    }

    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("add takes 2 arguments, got {}", args.len()),
            });
        }

        match (&args[0], &args[1]) {
            (NixValue::Integer(a), NixValue::Integer(b)) => Ok(NixValue::Integer(a + b)),
            (NixValue::Float(a), NixValue::Float(b)) => Ok(NixValue::Float(a + b)),
            (NixValue::Integer(a), NixValue::Float(b)) => Ok(NixValue::Float(*a as f64 + b)),
            (NixValue::Float(a), NixValue::Integer(b)) => Ok(NixValue::Float(a + *b as f64)),
            _ => Err(Error::UnsupportedExpression {
                reason: format!("add expects two numbers, got {} and {}", args[0], args[1]),
            }),
        }
    }
}

/// Ceil builtin - rounds a number up to the nearest integer
pub struct CeilBuiltin;

impl Builtin for CeilBuiltin {
    fn name(&self) -> &str {
        "ceil"
    }

    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("ceil takes 1 argument, got {}", args.len()),
            });
        }

        match &args[0] {
            NixValue::Integer(i) => Ok(NixValue::Integer(*i)),
            NixValue::Float(f) => Ok(NixValue::Integer(f.ceil() as i64)),
            _ => Err(Error::UnsupportedExpression {
                reason: format!("ceil expects a number, got {}", args[0]),
            }),
        }
    }
}

/// Floor builtin - rounds a number down to the nearest integer
pub struct FloorBuiltin;

impl Builtin for FloorBuiltin {
    fn name(&self) -> &str {
        "floor"
    }

    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("floor takes 1 argument, got {}", args.len()),
            });
        }

        match &args[0] {
            NixValue::Integer(i) => Ok(NixValue::Integer(*i)),
            NixValue::Float(f) => Ok(NixValue::Integer(f.floor() as i64)),
            _ => Err(Error::UnsupportedExpression {
                reason: format!("floor expects a number, got {}", args[0]),
            }),
        }
    }
}

/// ParseDrvName builtin - parses a derivation name string into name and version
///
/// `builtins.parseDrvName "name-version"` returns `{name = "name"; version = "version";}`
/// The first dash followed by a non-alphabetic character separates name from version.
pub struct ParseDrvNameBuiltin;

impl Builtin for ParseDrvNameBuiltin {
    fn name(&self) -> &str {
        "parseDrvName"
    }

    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("parseDrvName takes 1 argument, got {}", args.len()),
            });
        }

        let s = match &args[0] {
            NixValue::String(s) => s,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("parseDrvName expects a string, got {}", args[0]),
                });
            }
        };

        // Find the first dash followed by a non-alphabetic character
        // This separates the name from the version
        let mut name_end = None;
        for (i, ch) in s.char_indices() {
            if ch == '-' {
                // Check if the next character (if any) is non-alphabetic
                let next_ch = s[i + ch.len_utf8()..].chars().next();
                if let Some(next) = next_ch {
                    if !next.is_alphabetic() {
                        name_end = Some(i);
                        break;
                    }
                } else {
                    // Dash at the end - this separates name from version
                    name_end = Some(i);
                    break;
                }
            }
        }

        let (name, version) = if let Some(end) = name_end {
            let name_part = &s[..end];
            let version_part = &s[end + 1..];
            (name_part.to_string(), version_part.to_string())
        } else {
            // No separator found - entire string is the name, version is empty
            (s.clone(), "".to_string())
        };

        let mut result = HashMap::new();
        result.insert("name".to_string(), NixValue::String(name));
        result.insert("version".to_string(), NixValue::String(version));
        Ok(NixValue::AttributeSet(result))
    }
}

/// Mul builtin - multiplies two numbers
pub struct MulBuiltin;

impl Builtin for MulBuiltin {
    fn name(&self) -> &str {
        "mul"
    }

    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("mul takes 2 arguments, got {}", args.len()),
            });
        }

        match (&args[0], &args[1]) {
            (NixValue::Integer(a), NixValue::Integer(b)) => Ok(NixValue::Integer(a * b)),
            (NixValue::Float(a), NixValue::Float(b)) => Ok(NixValue::Float(a * b)),
            (NixValue::Integer(a), NixValue::Float(b)) => Ok(NixValue::Float(*a as f64 * b)),
            (NixValue::Float(a), NixValue::Integer(b)) => Ok(NixValue::Float(a * *b as f64)),
            _ => Err(Error::UnsupportedExpression {
                reason: format!("mul expects two numbers, got {} and {}", args[0], args[1]),
            }),
        }
    }
}

/// Div builtin - integer division (truncates towards zero)
///
/// `builtins.div a b` performs integer division, truncating towards zero.
pub struct DivBuiltin;

impl Builtin for DivBuiltin {
    fn name(&self) -> &str {
        "div"
    }

    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("div takes 2 arguments, got {}", args.len()),
            });
        }

        match (&args[0], &args[1]) {
            (NixValue::Integer(a), NixValue::Integer(b)) => {
                if *b == 0 {
                    return Err(Error::UnsupportedExpression {
                        reason: "division by zero".to_string(),
                    });
                }
                // Integer division truncates towards zero
                Ok(NixValue::Integer(a / b))
            }
            (NixValue::Float(a), NixValue::Float(b)) => {
                if *b == 0.0 {
                    return Err(Error::UnsupportedExpression {
                        reason: "division by zero".to_string(),
                    });
                }
                // Float division returns float
                Ok(NixValue::Float(a / b))
            }
            (NixValue::Integer(a), NixValue::Float(b)) => {
                if *b == 0.0 {
                    return Err(Error::UnsupportedExpression {
                        reason: "division by zero".to_string(),
                    });
                }
                Ok(NixValue::Float(*a as f64 / b))
            }
            (NixValue::Float(a), NixValue::Integer(b)) => {
                if *b == 0 {
                    return Err(Error::UnsupportedExpression {
                        reason: "division by zero".to_string(),
                    });
                }
                Ok(NixValue::Float(a / *b as f64))
            }
            _ => Err(Error::UnsupportedExpression {
                reason: format!("div expects two numbers, got {} and {}", args[0], args[1]),
            }),
        }
    }
}

/// Sub builtin - subtracts two numbers
pub struct SubBuiltin;

impl Builtin for SubBuiltin {
    fn name(&self) -> &str {
        "sub"
    }

    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("sub takes 2 arguments, got {}", args.len()),
            });
        }

        match (&args[0], &args[1]) {
            (NixValue::Integer(a), NixValue::Integer(b)) => Ok(NixValue::Integer(a - b)),
            (NixValue::Float(a), NixValue::Float(b)) => Ok(NixValue::Float(a - b)),
            (NixValue::Integer(a), NixValue::Float(b)) => Ok(NixValue::Float(*a as f64 - b)),
            (NixValue::Float(a), NixValue::Integer(b)) => Ok(NixValue::Float(a - *b as f64)),
            _ => Err(Error::UnsupportedExpression {
                reason: format!("sub expects two numbers, got {} and {}", args[0], args[1]),
            }),
        }
    }
}

/// ToJSON builtin - converts a Nix value to JSON string
pub struct ToJSONBuiltin;

fn nix_value_to_json_value(value: &NixValue, evaluator: &Evaluator) -> Result<serde_json::Value> {
    let forced = value.clone().force(evaluator)?;
    match forced {
        NixValue::Null => Ok(serde_json::Value::Null),
        NixValue::Boolean(b) => Ok(serde_json::Value::Bool(b)),
        NixValue::Integer(i) => Ok(serde_json::Value::Number(i.into())),
        NixValue::Float(f) => {
            let num =
                serde_json::Number::from_f64(f).ok_or_else(|| Error::UnsupportedExpression {
                    reason: "toJSON: float is not a valid JSON number".to_string(),
                })?;
            Ok(serde_json::Value::Number(num))
        }
        NixValue::String(s) => Ok(serde_json::Value::String(s)),
        NixValue::Path(p) => Ok(serde_json::Value::String(p.to_string_lossy().into_owned())),
        NixValue::StorePath(p) => Ok(serde_json::Value::String(p)),
        NixValue::List(l) => {
            let mut parts = Vec::new();
            for item in l {
                parts.push(nix_value_to_json_value(&item, evaluator)?);
            }
            Ok(serde_json::Value::Array(parts))
        }
        NixValue::AttributeSet(attrs) => {
            // Check for __toString
            if let Some(to_string) = attrs.get("__toString") {
                let to_string_forced = to_string.clone().force(evaluator)?;
                if let NixValue::Function(func) = to_string_forced {
                    let mut attrs_copy = attrs.clone();
                    attrs_copy.remove("__toString");
                    let result = func.apply(evaluator, NixValue::AttributeSet(attrs_copy))?;
                    let result_forced = result.force(evaluator)?;
                    if let NixValue::String(s) = result_forced {
                        return Ok(serde_json::Value::String(s));
                    }
                }
            }

            let mut map = serde_json::Map::new();
            let mut keys: Vec<_> = attrs.keys().collect();
            keys.sort();
            for key in keys {
                let val = attrs.get(key).unwrap();
                let json_val = nix_value_to_json_value(val, evaluator)?;
                map.insert(key.clone(), json_val);
            }
            Ok(serde_json::Value::Object(map))
        }
        NixValue::Function(_) | NixValue::Derivation(_) | NixValue::Builtin(_) => {
            Err(Error::UnsupportedExpression {
                reason: format!("toJSON: cannot convert {} to JSON", forced),
            })
        }
        _ => unreachable!("force() should have resolved this"),
    }
}

impl Builtin for ToJSONBuiltin {
    fn name(&self) -> &str {
        "toJSON"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("toJSON takes 1 argument, got {}", args.len()),
            });
        }

        let json_value = nix_value_to_json_value(&args[0], evaluator)?;
        let json_str = serde_json::to_string(&json_value).unwrap();
        Ok(NixValue::String(json_str))
    }
    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "toJSON requires evaluator context".to_string(),
        })
    }
}

/// ToXML builtin - converts a value to an XML string
pub struct ToXMLBuiltin;

impl Builtin for ToXMLBuiltin {
    fn name(&self) -> &str {
        "toXML"
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "toXML requires evaluator context".to_string(),
        })
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("toXML takes 1 argument, got {}", args.len()),
            });
        }

        let xml_str = crate::xml::to_xml(&args[0], evaluator)?;
        Ok(NixValue::String(xml_str))
    }
}

/// FromJSON builtin - parses a JSON string to a Nix value
pub struct FromJSONBuiltin;

impl Builtin for FromJSONBuiltin {
    fn name(&self) -> &str {
        "fromJSON"
    }

    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("fromJSON takes 1 argument, got {}", args.len()),
            });
        }

        let json_str = match &args[0] {
            NixValue::String(s) => s,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("fromJSON expects a string, got {}", args[0]),
                });
            }
        };

        // Simple JSON parsing (basic implementation)
        // For a full implementation, we'd want a proper JSON parser
        let trimmed = json_str.trim();

        if trimmed == "null" {
            return Ok(NixValue::Null);
        }
        if trimmed == "true" {
            return Ok(NixValue::Boolean(true));
        }
        if trimmed == "false" {
            return Ok(NixValue::Boolean(false));
        }

        // Try to parse as integer
        if let Ok(i) = trimmed.parse::<i64>() {
            return Ok(NixValue::Integer(i));
        }

        // Try to parse as float
        if let Ok(f) = trimmed.parse::<f64>() {
            return Ok(NixValue::Float(f));
        }

        // Try to parse as string (remove quotes)
        if trimmed.starts_with('"') && trimmed.ends_with('"') {
            let unquoted = &trimmed[1..trimmed.len() - 1];
            // Unescape JSON string
            let unescaped = unquoted
                .replace("\\\"", "\"")
                .replace("\\\\", "\\")
                .replace("\\n", "\n")
                .replace("\\r", "\r")
                .replace("\\t", "\t");
            return Ok(NixValue::String(unescaped));
        }

        // Try to parse as list
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            // Simple list parsing - split by comma and parse each element
            let content = &trimmed[1..trimmed.len() - 1].trim();
            if content.is_empty() {
                return Ok(NixValue::List(Vec::new()));
            }
            // This is a simplified parser - a full implementation would handle nested structures
            return Err(Error::UnsupportedExpression {
                reason: "fromJSON: complex JSON parsing not yet implemented".to_string(),
            });
        }

        // Try to parse as object
        if trimmed.starts_with('{') && trimmed.ends_with('}') {
            // Simple object parsing
            return Err(Error::UnsupportedExpression {
                reason: "fromJSON: object parsing not yet implemented".to_string(),
            });
        }

        Err(Error::UnsupportedExpression {
            reason: format!("fromJSON: cannot parse JSON: {}", json_str),
        })
    }
}

/// GenList builtin - generates a list by calling a function for each index
///
/// `builtins.genList f n` generates a list of length n by calling f for each index from 0 to n-1.
/// This is a placeholder implementation - full implementation requires evaluator context to call Nix functions.
pub struct GenListBuiltin;

impl Builtin for GenListBuiltin {
    fn name(&self) -> &str {
        "genList"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("genList takes 2 arguments, got {}", args.len()),
            });
        }

        // genList generator length
        let generator = args[0].clone().force(evaluator)?;
        let length = match args[1].clone().force(evaluator)? {
            NixValue::Integer(n) => {
                if n < 0 {
                    return Err(Error::UnsupportedExpression {
                        reason: format!("genList: length must be non-negative, got {}", n),
                    });
                }
                n as usize
            }
            v => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("genList: second argument must be an integer, got {}", v),
                });
            }
        };

        let mut result = Vec::with_capacity(length);
        for i in 0..length {
            let mut thunk_scope = VariableScope::new();
            thunk_scope.insert("__f".to_string(), generator.clone());
            thunk_scope.insert("__i".to_string(), NixValue::Integer(i as i64));

            let thunk = NixValue::Thunk(Arc::new(crate::thunk::Thunk::new_from_text(
                "__f __i".to_string(),
                thunk_scope,
                evaluator.current_file_id(),
            )));
            result.push(thunk);
        }

        Ok(NixValue::List(result))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "genList requires evaluator context".to_string(),
        })
    }
}

/// PathExists builtin - checks if a path exists
pub struct PathExistsBuiltin;

impl Builtin for PathExistsBuiltin {
    fn name(&self) -> &str {
        "pathExists"
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: format!("{} requires evaluator context", self.name()),
        })
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("pathExists takes 1 argument, got {}", args.len()),
            });
        }
        let path_val = args[0].clone().force(evaluator)?;
        let path_str = match path_val {
            NixValue::String(s) => s,
            NixValue::Path(p) => p.to_string_lossy().to_string(),
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: "pathExists: argument must be a string or path".to_string(),
                });
            }
        };

        let resolved_path = evaluator.resolve_path(&std::path::PathBuf::from(path_str));
        Ok(NixValue::Boolean(resolved_path.exists()))
    }
}

/// ReadFile builtin - reads a file and returns its contents as a string
pub struct ReadFileBuiltin;

impl Builtin for ReadFileBuiltin {
    fn name(&self) -> &str {
        "readFile"
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: format!("{} requires evaluator context", self.name()),
        })
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("readFile takes 1 argument, got {}", args.len()),
            });
        }
        let path_val = args[0].clone().force(evaluator)?;
        let path_str = match path_val {
            NixValue::String(s) => s,
            NixValue::Path(p) => p.to_string_lossy().to_string(),
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: "readFile: argument must be a string or path".to_string(),
                });
            }
        };

        let resolved_path = evaluator.resolve_path(&std::path::PathBuf::from(path_str));
        let content =
            std::fs::read_to_string(resolved_path).map_err(|e| Error::UnsupportedExpression {
                reason: format!("readFile: failed to read file: {}", e),
            })?;

        Ok(NixValue::String(content))
    }
}

/// RemoveAttrs builtin - removes attributes from an attribute set
pub struct RemoveAttrsBuiltin;

impl Builtin for RemoveAttrsBuiltin {
    fn name(&self) -> &str {
        "removeAttrs"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("removeAttrs takes 2 arguments, got {}", args.len()),
            });
        }

        let attrs_val = args[0].clone().force(evaluator)?;
        let attrs = match attrs_val {
            NixValue::AttributeSet(a) => a,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!(
                        "removeAttrs: first argument must be an attribute set, got {}",
                        attrs_val
                    ),
                });
            }
        };

        let keys_val = args[1].clone().force(evaluator)?;
        let keys_to_remove = match keys_val {
            NixValue::List(l) => l,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!(
                        "removeAttrs: second argument must be a list, got {}",
                        keys_val
                    ),
                });
            }
        };

        // Collect keys to remove as strings
        let mut keys_set = std::collections::HashSet::new();
        for key_value in keys_to_remove {
            let key_forced = key_value.force(evaluator)?;
            let key = match key_forced {
                NixValue::String(s) => s,
                _ => {
                    return Err(Error::UnsupportedExpression {
                        reason: format!(
                            "removeAttrs: list must contain strings, got {}",
                            key_forced
                        ),
                    });
                }
            };
            keys_set.insert(key);
        }

        // Create new attribute set without the removed keys
        let mut new_attrs = HashMap::new();
        for (key, value) in attrs {
            if !keys_set.contains(&key) {
                new_attrs.insert(key.clone(), value.clone());
            }
        }

        Ok(NixValue::AttributeSet(new_attrs))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "removeAttrs requires evaluator context".to_string(),
        })
    }
}

/// ToPath builtin - converts a string to a path value
pub struct ToPathBuiltin;

impl Builtin for ToPathBuiltin {
    fn name(&self) -> &str {
        "toPath"
    }

    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("toPath takes 1 argument, got {}", args.len()),
            });
        }

        match &args[0] {
            NixValue::String(path_str) => {
                // Convert string to a Path value
                let path = PathBuf::from(path_str);
                Ok(NixValue::Path(path))
            }
            NixValue::Path(_) => {
                // Already a path, return as-is
                Ok(args[0].clone())
            }
            NixValue::StorePath(_) => {
                // Already a store path, return as-is
                Ok(args[0].clone())
            }
            _ => Err(Error::UnsupportedExpression {
                reason: format!("toPath expects a string or path, got {}", args[0]),
            }),
        }
    }
}

/// MapAttrs builtin - maps a function over an attribute set
pub struct MapAttrsBuiltin;

impl Builtin for MapAttrsBuiltin {
    fn name(&self) -> &str {
        "mapAttrs"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("mapAttrs takes 2 arguments, got {}", args.len()),
            });
        }

        let f = args[0].clone();
        let attrs_val = args[1].clone().force(evaluator)?;
        let attrs = match attrs_val {
            NixValue::AttributeSet(a) => a,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!(
                        "mapAttrs: second argument must be an attribute set, got {}",
                        attrs_val
                    ),
                });
            }
        };

        let mut result = HashMap::new();
        for (name, value) in attrs {
            let mut thunk_scope = VariableScope::new();
            thunk_scope.insert("__f".to_string(), f.clone());
            thunk_scope.insert("__n".to_string(), NixValue::String(name.clone()));
            thunk_scope.insert("__v".to_string(), value.clone());

            let thunk = NixValue::Thunk(Arc::new(crate::thunk::Thunk::new_from_text(
                "__f __n __v".to_string(),
                thunk_scope,
                evaluator.current_file_id(),
            )));
            result.insert(name, thunk);
        }

        Ok(NixValue::AttributeSet(result))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "mapAttrs requires evaluator context".to_string(),
        })
    }
}

/// ReadDir builtin - reads directory contents
pub struct ReadDirBuiltin;

impl Builtin for ReadDirBuiltin {
    fn name(&self) -> &str {
        "readDir"
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: format!("{} requires evaluator context", self.name()),
        })
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("readDir takes 1 argument, got {}", args.len()),
            });
        }
        let path_val = args[0].clone().force(evaluator)?;
        let path_str = match path_val {
            NixValue::String(s) => s,
            NixValue::Path(p) => p.to_string_lossy().to_string(),
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: "readDir: argument must be a string or path".to_string(),
                });
            }
        };

        let resolved_path = evaluator.resolve_path(&std::path::PathBuf::from(path_str));
        let entries =
            std::fs::read_dir(resolved_path).map_err(|e| Error::UnsupportedExpression {
                reason: format!("readDir: failed to read directory: {}", e),
            })?;

        let mut result = HashMap::new();
        for entry in entries {
            let entry = entry.map_err(|e| Error::UnsupportedExpression {
                reason: format!("readDir: directory entry error: {}", e),
            })?;
            let name = entry.file_name().to_string_lossy().to_string();
            let ft = entry
                .file_type()
                .map_err(|e| Error::UnsupportedExpression {
                    reason: format!("readDir: file type error: {}", e),
                })?;

            let type_str = if ft.is_dir() {
                "directory"
            } else if ft.is_file() {
                "regular"
            } else if ft.is_symlink() {
                "symlink"
            } else {
                "unknown"
            };

            result.insert(name, NixValue::String(type_str.to_string()));
        }

        Ok(NixValue::AttributeSet(result))
    }
}

/// ReadFileType builtin - returns the type of a file
pub struct ReadFileTypeBuiltin;

impl Builtin for ReadFileTypeBuiltin {
    fn name(&self) -> &str {
        "readFileType"
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: format!("{} requires evaluator context", self.name()),
        })
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("readFileType takes 1 argument, got {}", args.len()),
            });
        }
        let path_val = args[0].clone().force(evaluator)?;
        let path_str = match path_val {
            NixValue::String(s) => s,
            NixValue::Path(p) => p.to_string_lossy().to_string(),
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: "readFileType: argument must be a string or path".to_string(),
                });
            }
        };

        let resolved_path = evaluator.resolve_path(&std::path::PathBuf::from(path_str));
        let metadata =
            std::fs::symlink_metadata(resolved_path).map_err(|e| Error::UnsupportedExpression {
                reason: format!("readFileType: failed to get metadata: {}", e),
            })?;

        let ft = metadata.file_type();
        let type_str = if ft.is_dir() {
            "directory"
        } else if ft.is_file() {
            "regular"
            // Symlink should be handled but metadata.file_type() might need more.
            // On Unix it works, on Windows it's different.
        } else if ft.is_symlink() {
            "symlink"
        } else {
            "unknown"
        };

        Ok(NixValue::String(type_str.to_string()))
    }
}

/// LessThan builtin - compares two values
pub struct LessThanBuiltin;

impl Builtin for LessThanBuiltin {
    fn name(&self) -> &str {
        "lessThan"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("lessThan takes 2 arguments, got {}", args.len()),
            });
        }

        let a = args[0].clone().force(evaluator)?;
        let b = args[1].clone().force(evaluator)?;

        // Compare based on type
        let result = match (&a, &b) {
            (NixValue::Integer(x), NixValue::Integer(y)) => x < y,
            (NixValue::Float(x), NixValue::Float(y)) => x < y,
            (NixValue::Integer(x), NixValue::Float(y)) => (*x as f64) < *y,
            (NixValue::Float(x), NixValue::Integer(y)) => *x < (*y as f64),
            (NixValue::String(x), NixValue::String(y)) => x < y,
            (NixValue::Path(x), NixValue::Path(y)) => x < y,
            (NixValue::List(x), NixValue::List(y)) => {
                let mut is_less = false;
                let mut found_difference = false;
                for (xi, yi) in x.iter().zip(y.iter()) {
                    match self.call_with_evaluator(&[xi.clone(), yi.clone()], evaluator)? {
                        NixValue::Boolean(true) => {
                            is_less = true;
                            found_difference = true;
                            break;
                        }
                        NixValue::Boolean(false) => {
                            // Check if yi < xi to detect if they are equal
                            match self.call_with_evaluator(&[yi.clone(), xi.clone()], evaluator)? {
                                NixValue::Boolean(true) => {
                                    // xi > yi
                                    is_less = false;
                                    found_difference = true;
                                    break;
                                }
                                NixValue::Boolean(false) => {
                                    // xi == yi, continue
                                }
                                _ => unreachable!(),
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                if found_difference {
                    is_less
                } else {
                    x.len() < y.len()
                }
            }
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("lessThan: cannot compare {} and {}", a, b),
                });
            }
        };

        Ok(NixValue::Boolean(result))
    }
    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "lessThan requires evaluator context".to_string(),
        })
    }
}

/// ListToAttrs builtin - converts a list of attribute sets to an attribute set
pub struct ListToAttrsBuiltin;
impl Builtin for ListToAttrsBuiltin {
    fn name(&self) -> &str {
        "listToAttrs"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("listToAttrs takes 1 argument, got {}", args.len()),
            });
        }

        let list_val = args[0].clone().force(evaluator)?;
        let list = match list_val {
            NixValue::List(l) => l,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("listToAttrs: argument must be a list, got {}", list_val),
                });
            }
        };

        let mut result = HashMap::new();
        for item in list {
            let item_forced = item.force(evaluator)?;
            let item_attrs = match item_forced {
                NixValue::AttributeSet(a) => a,
                _ => {
                    return Err(Error::UnsupportedExpression {
                        reason: format!(
                            "listToAttrs: each element must be an attribute set, got {}",
                            item_forced
                        ),
                    });
                }
            };

            let name_val = item_attrs
                .get("name")
                .ok_or_else(|| Error::UnsupportedExpression {
                    reason: "listToAttrs: element must have a 'name' attribute".to_string(),
                })?;
            let name_forced = name_val.clone().force(evaluator)?;
            let name = match name_forced {
                NixValue::String(s) => s,
                _ => {
                    return Err(Error::UnsupportedExpression {
                        reason: format!(
                            "listToAttrs: 'name' attribute must be a string, got {}",
                            name_forced
                        ),
                    });
                }
            };

            let value = item_attrs
                .get("value")
                .ok_or_else(|| Error::UnsupportedExpression {
                    reason: "listToAttrs: element must have a 'value' attribute".to_string(),
                })?
                .clone();

            // Insert into result (first occurrence wins if duplicate names)
            result.entry(name).or_insert(value);
        }

        Ok(NixValue::AttributeSet(result))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "listToAttrs requires evaluator context".to_string(),
        })
    }
}

/// Partition builtin - partitions a list based on a predicate
pub struct PartitionBuiltin;

impl Builtin for PartitionBuiltin {
    fn name(&self) -> &str {
        "partition"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("partition takes 2 arguments, got {}", args.len()),
            });
        }

        let pred = args[0].clone();
        let list_val = args[1].clone().force(evaluator)?;
        let list = match list_val {
            NixValue::List(l) => l,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!(
                        "partition: second argument must be a list, got {}",
                        list_val
                    ),
                });
            }
        };

        let mut right = Vec::new();
        let mut wrong = Vec::new();

        for item in list {
            let res = pred
                .clone()
                .apply(evaluator, item.clone())?
                .force(evaluator)?;
            match res {
                NixValue::Boolean(b) => {
                    if b {
                        right.push(item);
                    } else {
                        wrong.push(item);
                    }
                }
                _ => {
                    return Err(Error::UnsupportedExpression {
                        reason: format!("partition: predicate must return a boolean, got {}", res),
                    });
                }
            }
        }

        let mut result = HashMap::new();
        result.insert("right".to_string(), NixValue::List(right));
        result.insert("wrong".to_string(), NixValue::List(wrong));

        Ok(NixValue::AttributeSet(result))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "partition requires evaluator context".to_string(),
        })
    }
}

/// HashString builtin - hashes a string using the specified algorithm
pub struct HashStringBuiltin;

impl Builtin for HashStringBuiltin {
    fn name(&self) -> &str {
        "hashString"
    }

    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("hashString takes 2 arguments, got {}", args.len()),
            });
        }

        let algorithm = match &args[0] {
            NixValue::String(s) => s.as_str(),
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!(
                        "hashString: first argument must be a string, got {}",
                        args[0]
                    ),
                });
            }
        };

        let input = match &args[1] {
            NixValue::String(s) => s.as_str(),
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!(
                        "hashString: second argument must be a string, got {}",
                        args[1]
                    ),
                });
            }
        };

        // Compute hash based on algorithm
        let hash_hex = match algorithm {
            "md5" => {
                let digest = md5::compute(input.as_bytes());
                hex::encode(digest.as_slice())
            }
            "sha1" => {
                use sha1::{Digest, Sha1};
                let mut hasher = Sha1::new();
                hasher.update(input.as_bytes());
                let hash_bytes = hasher.finalize();
                hex::encode(hash_bytes)
            }
            "sha256" => {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(input.as_bytes());
                let hash_bytes = hasher.finalize();
                hex::encode(hash_bytes)
            }
            "sha512" => {
                use sha2::{Digest, Sha512};
                let mut hasher = Sha512::new();
                hasher.update(input.as_bytes());
                let hash_bytes = hasher.finalize();
                hex::encode(hash_bytes)
            }
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!(
                        "hashString: unsupported algorithm '{}', supported: md5, sha1, sha256, sha512",
                        algorithm
                    ),
                });
            }
        };

        Ok(NixValue::String(hash_hex))
    }
}

/// GroupBy builtin - groups elements of a list by a key function
pub struct GroupByBuiltin;

impl Builtin for GroupByBuiltin {
    fn name(&self) -> &str {
        "groupBy"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("groupBy takes 2 arguments, got {}", args.len()),
            });
        }

        let func = args[0].clone().force(evaluator)?;
        let list_val = args[1].clone().force(evaluator)?;
        let list = match list_val {
            NixValue::List(l) => l,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: "groupBy: second argument must be a list".to_string(),
                });
            }
        };

        let mut groups: HashMap<String, Vec<NixValue>> = HashMap::new();
        for item in list {
            let key_val = func
                .clone()
                .apply(evaluator, item.clone())?
                .force(evaluator)?;
            let key = key_val.as_string()?;
            groups.entry(key).or_default().push(item);
        }

        let mut result = HashMap::new();
        for (key, values) in groups {
            result.insert(key, NixValue::List(values));
        }

        Ok(NixValue::AttributeSet(result))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "groupBy requires evaluator context".to_string(),
        })
    }
}

/// HasContext builtin - checks if a value has a context (store path references)
pub struct HasContextBuiltin;

impl Builtin for HasContextBuiltin {
    fn name(&self) -> &str {
        "hasContext"
    }

    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("hasContext takes 1 argument, got {}", args.len()),
            });
        }

        // Check if the value has context (store path references)
        // For now, we'll check if it's a StorePath or contains StorePath references
        // In a full implementation, we'd need to track context through string interpolation
        let has_context = match &args[0] {
            NixValue::StorePath(_) => true,
            NixValue::String(s) => {
                // Check if string contains store path references (format: /nix/store/...)
                s.contains("/nix/store/")
            }
            _ => false,
        };

        Ok(NixValue::Boolean(has_context))
    }
}

/// Substring builtin - extracts a substring from a string
pub struct SubstringBuiltin;

impl Builtin for SubstringBuiltin {
    fn name(&self) -> &str {
        "substring"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 3 {
            return Err(Error::UnsupportedExpression {
                reason: format!("substring takes 3 arguments, got {}", args.len()),
            });
        }

        let start = match args[0].clone().force(evaluator)? {
            NixValue::Integer(i) => i,
            v => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("substring: first argument must be an integer, got {}", v),
                });
            }
        };

        let len = match args[1].clone().force(evaluator)? {
            NixValue::Integer(i) => i,
            v => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("substring: second argument must be an integer, got {}", v),
                });
            }
        };

        let s = match args[2].clone().force(evaluator)? {
            NixValue::String(s) => s,
            v => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("substring: third argument must be a string, got {}", v),
                });
            }
        };

        if start < 0 {
            return Ok(NixValue::String("".to_string()));
        }

        let start = start as usize;
        let s_chars: Vec<char> = s.chars().collect();

        if start >= s_chars.len() {
            return Ok(NixValue::String("".to_string()));
        }

        let end = if len < 0 {
            s_chars.len()
        } else {
            std::cmp::min(start + len as usize, s_chars.len())
        };

        Ok(NixValue::String(s_chars[start..end].iter().collect()))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "substring requires evaluator context".to_string(),
        })
    }
}

/// ReplaceStrings builtin - replaces occurrences of strings in a string
pub struct ReplaceStringsBuiltin;

impl Builtin for ReplaceStringsBuiltin {
    fn name(&self) -> &str {
        "replaceStrings"
    }

    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 3 {
            return Err(Error::UnsupportedExpression {
                reason: format!("replaceStrings takes 3 arguments, got {}", args.len()),
            });
        }

        let from_list = match &args[0] {
            NixValue::List(l) => l,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!(
                        "replaceStrings: first argument must be a list, got {}",
                        args[0]
                    ),
                });
            }
        };

        let to_list = match &args[1] {
            NixValue::List(l) => l,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!(
                        "replaceStrings: second argument must be a list, got {}",
                        args[1]
                    ),
                });
            }
        };

        if from_list.len() != to_list.len() {
            return Err(Error::UnsupportedExpression {
                reason: format!(
                    "replaceStrings: from and to lists must have the same length, got {} and {}",
                    from_list.len(),
                    to_list.len()
                ),
            });
        }

        let s = match &args[2] {
            NixValue::String(s) => s.clone(),
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!(
                        "replaceStrings: third argument must be a string, got {}",
                        args[2]
                    ),
                });
            }
        };

        // Apply replacements sequentially
        // Nix's replaceStrings processes replacements in order, applying each pattern globally
        // Empty strings are handled specially: they insert replacements at boundaries
        let mut result = s.clone();

        for (from, to) in from_list.iter().zip(to_list.iter()) {
            let from_str = match from {
                NixValue::String(s) => s,
                _ => {
                    return Err(Error::UnsupportedExpression {
                        reason: format!(
                            "replaceStrings: from list must contain strings, got {}",
                            from
                        ),
                    });
                }
            };

            let to_str = match to {
                NixValue::String(s) => s,
                _ => {
                    return Err(Error::UnsupportedExpression {
                        reason: format!("replaceStrings: to list must contain strings, got {}", to),
                    });
                }
            };

            if from_str.is_empty() {
                // Empty string: insert replacement at start and end
                result = format!("{}{}{}", to_str, result, to_str);
            } else {
                // Non-empty string: replace all occurrences
                result = result.replace(from_str, to_str);
            }
        }

        Ok(NixValue::String(result))
    }
}

/// Split builtin - splits a string using a regex
pub struct SplitBuiltin;

impl Builtin for SplitBuiltin {
    fn name(&self) -> &str {
        "split"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("split takes 2 arguments, got {}", args.len()),
            });
        }

        let regex_val = args[0].clone().force(evaluator)?;
        let regex_str = match regex_val {
            NixValue::String(s) => s,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("split: first argument must be a string, got {}", regex_val),
                });
            }
        };

        let s_val = args[1].clone().force(evaluator)?;
        let s = match s_val {
            NixValue::String(s) => s,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("split: second argument must be a string, got {}", s_val),
                });
            }
        };

        let re = match Regex::new(&regex_str) {
            Ok(r) => r,
            Err(e) => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("split: invalid regex '{}': {}", regex_str, e),
                });
            }
        };

        // Handle empty regex specially: split between every character
        if regex_str.is_empty() {
            let mut result = Vec::new();
            result.push(NixValue::String("".to_string())); // leading empty
            for ch in s.chars() {
                result.push(NixValue::List(Vec::new())); // empty capture groups
                result.push(NixValue::String(ch.to_string()));
            }
            result.push(NixValue::List(Vec::new())); // empty capture groups at end
            result.push(NixValue::String("".to_string())); // trailing empty
            return Ok(NixValue::List(result));
        }

        let can_match_empty = re.is_match("");
        let mut result = Vec::new();
        let mut last_end = 0;
        let mut last_match_was_empty = false;

        for caps in re.captures_iter(&s) {
            let full_match = caps.get(0).unwrap();

            // Text before the match (always include, even if empty)
            result.push(NixValue::String(
                s[last_end..full_match.start()].to_string(),
            ));

            // Capture groups (Nix excluding the full match)
            let mut groups = Vec::new();
            for i in 1..caps.len() {
                groups.push(match caps.get(i) {
                    Some(m) => NixValue::String(m.as_str().to_string()),
                    None => NixValue::Null,
                });
            }
            result.push(NixValue::List(groups));

            last_match_was_empty = full_match.start() == full_match.end();
            last_end = full_match.end();
        }

        // If the regex can match an empty string and the last non-empty match
        // ended at the end of the string, Rust regex may not report the trailing
        // zero-width match. Add it manually.
        if can_match_empty && last_end == s.len() && !result.is_empty() && !last_match_was_empty {
            // Compute capture groups for an empty match
            let mut groups = Vec::new();
            if let Some(empty_caps) = re.captures("") {
                for i in 1..empty_caps.len() {
                    groups.push(match empty_caps.get(i) {
                        Some(m) => NixValue::String(m.as_str().to_string()),
                        None => NixValue::Null,
                    });
                }
            }
            result.push(NixValue::String("".to_string()));
            result.push(NixValue::List(groups));
            result.push(NixValue::String("".to_string()));
        } else if last_end <= s.len() {
            // Final part after last match (always include, even if empty)
            result.push(NixValue::String(s[last_end..].to_string()));
        }

        Ok(NixValue::List(result))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "split requires evaluator context".to_string(),
        })
    }
}

/// Match builtin - matches a string against a regex
pub struct MatchBuiltin;

impl Builtin for MatchBuiltin {
    fn name(&self) -> &str {
        "match"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("match takes 2 arguments, got {}", args.len()),
            });
        }

        let regex_val = args[0].clone().force(evaluator)?;
        let regex_str = match regex_val {
            NixValue::String(s) => s,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("match: first argument must be a string, got {}", regex_val),
                });
            }
        };

        let s_val = args[1].clone().force(evaluator)?;
        let s = match s_val {
            NixValue::String(s) => s,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("match: second argument must be a string, got {}", s_val),
                });
            }
        };

        // Nix match must match the ENTIRE string
        let anchored_regex = format!("^({})$", regex_str);
        let re = match Regex::new(&anchored_regex) {
            Ok(r) => r,
            Err(e) => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("match: invalid regex '{}': {}", regex_str, e),
                });
            }
        };

        if let Some(caps) = re.captures(&s) {
            let mut groups = Vec::new();
            // Nix match returns capture groups of the ORIGINAL regex.
            // Since we wrapped in (...), our index 1 is the whole match.
            // We want the groups FROM THE USER'S REGEX.
            // Actually, because we added ^( ... )$, user's group 1 is now our group 2.
            // Wait! If user had groups, they start at index 2 of our re.
            // But if user didn't have groups, they should get an empty list if it matches?
            // Nix: "If the pattern matches, the result is a list of strings... one for each parenthesized subexpression."

            // Let's count groups in original regex first?
            // Or just skip our index 1.
            for i in 2..caps.len() {
                groups.push(match caps.get(i) {
                    Some(m) => NixValue::String(m.as_str().to_string()),
                    None => NixValue::Null,
                });
            }
            Ok(NixValue::List(groups))
        } else {
            Ok(NixValue::Null)
        }
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "match requires evaluator context".to_string(),
        })
    }
}

/// CompareVersions builtin - compares two version strings
///
/// `builtins.compareVersions a b` returns:
/// - -1 if a < b
/// - 0 if a == b
/// - 1 if a > b
pub struct CompareVersionsBuiltin;

impl Builtin for CompareVersionsBuiltin {
    fn name(&self) -> &str {
        "compareVersions"
    }

    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if args.len() != 2 {
            return Err(Error::UnsupportedExpression {
                reason: format!("compareVersions takes 2 arguments, got {}", args.len()),
            });
        }

        let a = match &args[0] {
            NixValue::String(s) => s,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!(
                        "compareVersions: first argument must be a string, got {}",
                        args[0]
                    ),
                });
            }
        };

        let b = match &args[1] {
            NixValue::String(s) => s,
            _ => {
                return Err(Error::UnsupportedExpression {
                    reason: format!(
                        "compareVersions: second argument must be a string, got {}",
                        args[1]
                    ),
                });
            }
        };

        // Nix version comparison algorithm:
        // 1. Split versions into components (numbers and strings)
        // 2. Compare components lexicographically
        // 3. Numbers are compared numerically, strings lexicographically
        // 4. Empty components are treated as 0
        // 5. "pre" suffix is special - versions with "pre" are less than versions without

        fn split_version(s: &str) -> Vec<String> {
            let mut components = Vec::new();
            let mut current = String::new();

            for ch in s.chars() {
                if ch.is_ascii_digit() {
                    if !current.is_empty() && !current.chars().next().unwrap().is_ascii_digit() {
                        components.push(current.clone());
                        current.clear();
                    }
                    current.push(ch);
                } else if ch.is_ascii_alphabetic() {
                    if !current.is_empty() && current.chars().next().unwrap().is_ascii_digit() {
                        components.push(current.clone());
                        current.clear();
                    }
                    current.push(ch);
                } else {
                    if !current.is_empty() {
                        components.push(current.clone());
                        current.clear();
                    }
                }
            }
            if !current.is_empty() {
                components.push(current);
            }
            components
        }

        let a_parts = split_version(a);
        let b_parts = split_version(b);

        // Compare components
        let max_len = a_parts.len().max(b_parts.len());
        for i in 0..max_len {
            let a_part = a_parts.get(i).map(|s| s.as_str()).unwrap_or("");
            let b_part = b_parts.get(i).map(|s| s.as_str()).unwrap_or("");

            if a_part == b_part {
                continue;
            }

            // Special case for "pre" - it's smaller than anything else (including empty)
            if a_part == "pre" {
                return Ok(NixValue::Integer(-1));
            }
            if b_part == "pre" {
                return Ok(NixValue::Integer(1));
            }

            // Handle empty parts (padding)
            if a_part.is_empty() {
                return Ok(NixValue::Integer(-1));
            }
            if b_part.is_empty() {
                return Ok(NixValue::Integer(1));
            }

            // Try to parse as numbers
            let a_num = a_part.parse::<i64>().ok();
            let b_num = b_part.parse::<i64>().ok();

            match (a_num, b_num) {
                (Some(a_n), Some(b_n)) => {
                    if a_n < b_n {
                        return Ok(NixValue::Integer(-1));
                    } else if a_n > b_n {
                        return Ok(NixValue::Integer(1));
                    }
                }
                (Some(_), None) => return Ok(NixValue::Integer(1)), // Number > String
                (None, Some(_)) => return Ok(NixValue::Integer(-1)), // String < Number
                (None, None) => {
                    if a_part < b_part {
                        return Ok(NixValue::Integer(-1));
                    } else if a_part > b_part {
                        return Ok(NixValue::Integer(1));
                    }
                }
            }
        }

        // All components equal
        Ok(NixValue::Integer(0))
    }
}

/// SplitVersion builtin - splits a version string into components
pub struct SplitVersionBuiltin;

impl Builtin for SplitVersionBuiltin {
    fn name(&self) -> &str {
        "splitVersion"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("splitVersion takes 1 argument, got {}", args.len()),
            });
        }

        let version = match args[0].clone().force(evaluator)? {
            NixValue::String(s) => s,
            v => {
                return Err(Error::UnsupportedExpression {
                    reason: format!("splitVersion expects a string, got {}", v),
                });
            }
        };

        let mut result = Vec::new();
        let mut current = String::new();

        for ch in version.chars() {
            if ch.is_alphanumeric() {
                current.push(ch);
            } else if !current.is_empty() {
                result.push(NixValue::String(current.clone()));
                current.clear();
            }
        }

        if !current.is_empty() {
            result.push(NixValue::String(current));
        }

        Ok(NixValue::List(result))
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "splitVersion requires evaluator context".to_string(),
        })
    }
}

/// NixVersion builtin - returns the Nix version string
pub struct NixVersionBuiltin;

impl Builtin for NixVersionBuiltin {
    fn name(&self) -> &str {
        "nixVersion"
    }

    fn call(&self, args: &[NixValue]) -> Result<NixValue> {
        if !args.is_empty() {
            return Err(Error::UnsupportedExpression {
                reason: format!("nixVersion takes 0 arguments, got {}", args.len()),
            });
        }
        // Return a version string compatible with nixpkgs checks
        // Using "2.18" as a safe default that works with most checks
        Ok(NixValue::String("2.18".to_string()))
    }
}

/// FunctionArgs builtin - returns an attribute set describing a functions parameters
pub struct FunctionArgsBuiltin;
impl Builtin for FunctionArgsBuiltin {
    fn name(&self) -> &str {
        "functionArgs"
    }

    fn call_with_evaluator(&self, args: &[NixValue], evaluator: &Evaluator) -> Result<NixValue> {
        if args.len() != 1 {
            return Err(Error::UnsupportedExpression {
                reason: format!("functionArgs takes 1 argument, got {}", args.len()),
            });
        }

        let func_val = args[0].clone().force(evaluator)?;
        match func_val {
            NixValue::Function(func) => {
                let mut result = HashMap::new();
                match func.parameter() {
                    crate::function::Parameter::Simple(_) => {
                        // Simple functions dont have named arguments for functionArgs
                    }
                    crate::function::Parameter::Pattern { entries, .. } => {
                        for (name, default) in entries {
                            result.insert(name.clone(), NixValue::Boolean(default.is_some()));
                        }
                    }
                }
                Ok(NixValue::AttributeSet(result))
            }
            _ => Err(Error::UnsupportedExpression {
                reason: "functionArgs: argument must be a function".to_string(),
            }),
        }
    }

    fn call(&self, _args: &[NixValue]) -> Result<NixValue> {
        Err(Error::UnsupportedExpression {
            reason: "functionArgs requires evaluator context".to_string(),
        })
    }
}
