//! Minecraft bot abstraction layer.
//!
//! ## Architecture
//!
//! JS used mineflayer directly from within the agent process. In Rust:
//!
//!   Agent (core crate)
//!     └─→ BotHandle  (this crate, thin async facade)
//!           └─→ azalea::Client  (actual MC protocol)
//!
//! BotHandle exposes high-level async methods so core/agent.rs never
//! touches protocol details. When `feature = "stub"` is active, BotHandle
//! is a no-op for unit testing.

pub mod bot;
pub mod skills;
pub mod world;

pub use bot::{BotHandle, BotConfig, BotEvent};
