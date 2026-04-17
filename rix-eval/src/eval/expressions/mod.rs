//! Expression evaluation modules
//!
//! These modules contain the implementation of expression evaluation methods
//! for the Evaluator. They are organized by expression type for better
//! maintainability and clarity.

mod attrsets;
mod functions;
mod import;
mod lists;
mod literals;
mod operators;
mod special;

// Re-export all expression evaluation methods
