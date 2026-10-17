//! Search query language parser producing SQL plans.
#![warn(missing_docs)]

mod lex;
mod plan;

pub use plan::{Diag, QueryPlan, parse};
