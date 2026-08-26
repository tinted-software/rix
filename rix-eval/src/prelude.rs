//! Prelude module for convenient imports
//!
//! This module re-exports commonly used types and traits from the library.
//! Import everything with `use nix_eval::prelude::*;` or import specific items.
//!
//! # Example
//!
//! ```no_run
//! use nix_eval::prelude::*;
//!
//! let evaluator = Evaluator::new();
//! let result = evaluator.evaluate("42").unwrap();
//! assert_eq!(result, NixValue::Integer(42));
//! ```

pub use crate::builtin::Builtin;
pub use crate::error::{Error, Result};
pub use crate::eval::{EvaluationContext, Evaluator, VariableScope};
pub use crate::function::Function;
pub use crate::thunk::Thunk;
pub use crate::value::{Derivation, NixValue};
