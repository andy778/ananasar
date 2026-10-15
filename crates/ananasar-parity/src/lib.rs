//! Wire-level parity tooling for comparing Modbus/TCP servers (for example
//! Ananasar and the original Ananas) byte for byte.
//!
//! - [`scenarios`] and [`runner`]: a fixed list of requests, run against any
//!   server address and recorded.
//! - [`proxy`]: a logging TCP proxy that records any client/server session.
//! - [`diff`]: compares two recordings, ignoring transaction ids and timing.
//! - [`golden`]: committed recordings of the original, with deviation notes.
//! - [`ours`]: Ananasar's server in-process, set up like a scenario profile.
//! - [`recording`]: the plain-text format they all share.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "developer tool, not yet audited for panics (see #59)"
)]

pub mod diff;
pub mod golden;
pub mod ours;
pub mod proxy;
pub mod recording;
pub mod runner;
pub mod scenarios;
