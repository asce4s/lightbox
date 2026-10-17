//! The non-destructive edit model (SPEC §6.6.1): operations, stacks, validation and hashing.

mod hash;
mod op;
mod stack;
#[cfg(any(test, feature = "test-strategies"))]
pub mod strategies;
mod validate;

pub use op::{EditOp, NormRect, RetouchSpot};
pub use stack::EditStack;
pub use validate::EditError;

#[cfg(test)]
mod tests;
