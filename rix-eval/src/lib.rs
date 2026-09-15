//! # nix-eval
//!
//! A pure Rust library for evaluating Nix expressions.

pub mod builder;
mod builtin;
mod builtins;
mod error;
mod eval;
mod function;
pub mod prelude;
pub mod store;
mod thunk;
mod value;
mod xml;

// Re-export public API
pub use builder::{BuildOptions, BuildResult, Builder};
pub use builtin::Builtin;
pub use error::{Error, Result};
pub use eval::{EvaluationContext, Evaluator, VariableScope};
pub use function::Function;
pub use store::Store;
pub use thunk::Thunk;
pub use value::{Derivation, NixValue};
