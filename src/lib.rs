// Module declarations for the library crate.

pub mod cli;
pub mod config;
pub mod event;
pub mod filter;
pub mod logger;

// Re-export statistics types for convenience, e.g., for tests or potential external users.
pub use filter::stats;
