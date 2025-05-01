// Module declarations for the library crate.

pub mod config; // Defines the Config struct.
pub mod event; // Handles reading/writing input_event structs.
pub mod filter; // Defines BounceFilter state and core logic.
pub mod logger; // Implements the logger thread for stats and stderr output.

// Re-export statistics types for convenience, e.g., for tests or potential external users.
pub use filter::stats;
