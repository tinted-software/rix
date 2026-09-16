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
pub mod sandbox;
pub mod store;
mod thunk;
mod value;
mod xml;

pub use builder::{BuildOptions, BuildResult, Builder};
pub use builtin::Builtin;
pub use error::{Error, Result};
pub use eval::{EvaluationContext, Evaluator, VariableScope};
pub use function::Function;
pub use sandbox::{BOOTSTRAP_TARBALL_ENV, ResolvedSandbox, SandboxConfig};
pub use store::Store;
pub use thunk::Thunk;
pub use value::{Derivation, NixValue};
