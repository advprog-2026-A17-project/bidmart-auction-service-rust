//! Public facade module for the auction service.
//!
//! The implementation lives in `auction_application` so this module can remain
//! a stable compatibility boundary for HTTP handlers, tests, and other modules
//! that import `service::auction_service::*`.

pub use super::auction_application::*;
