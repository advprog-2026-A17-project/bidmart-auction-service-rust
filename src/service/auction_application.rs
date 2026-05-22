//! Auction application boundary.
//!
//! Public callers keep importing `service::auction_application` and
//! `service::auction_service`, while the implementation is isolated in
//! `auction_core` and supporting use-case modules. The per-auction `Mutex`
//! remains intentional for the current single-instance production topology;
//! database transactions and row locks remain the correctness layer.

pub use super::auction_core::*;
pub use super::auction_types::*;
