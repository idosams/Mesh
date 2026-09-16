//! A small, dependency-free JSON value with deterministic serialisation.
//!
//! The result schema is validated as *data*, not as a Rust type: a row that
//! reaches the sink with a field missing has to be rejected by name, which is
//! only possible if the row can exist in a partial form first. [`Json`] is that
//! partial form. Field order is insertion order and never sorted, so a row
//! written twice from the same values is byte-identical — the property the
//! repeatability check leans on.

mod parse;
mod value;
mod write;

pub use parse::{parse, ParseError};
pub use value::{Json, JsonObject};
pub use write::{to_string_compact, to_string_pretty};
