//! # nix-eval
//!
//! A pure Rust library for evaluating Nix expressions.

mod builtin;
mod builtins;
mod error;
mod eval;
mod function;
pub mod prelude;
mod thunk;
mod value;
mod xml;

// Re-export public API
pub use builtin::Builtin;
pub use error::{Error, Result};
pub use eval::{EvaluationContext, Evaluator, VariableScope};
pub use function::Function;
pub use thunk::Thunk;
pub use value::{Derivation, NixValue};
