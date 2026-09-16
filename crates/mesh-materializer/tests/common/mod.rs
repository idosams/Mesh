//! Shared test machinery: a seeded generator, a generated corpus, a comparison surface and the
//! reference oracle.
//!
//! Not a Cargo target. Each integration test declares `mod common;` and pulls in what it needs, so
//! the modules below carry `#![allow(dead_code)]` — a helper used by one test binary is dead code
//! in the others, and the alternative is one enormous test file.

#![allow(dead_code)]

pub mod corpus;
pub mod digest;
pub mod oracle;
pub mod rng;
pub mod snapshot;
