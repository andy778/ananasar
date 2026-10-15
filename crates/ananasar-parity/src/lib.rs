//! Wire-level parity tooling for comparing Modbus/TCP servers (for example
//! Ananasar and the original Ananas) byte for byte.
//!
//! - [`scenarios`] and [`runner`]: a fixed list of requests, run against any
//!   server address and recorded.
//! - [`proxy`]: a logging TCP proxy that records any client/server session.
//! - [`diff`]: compares two recordings, ignoring transaction ids and timing.
//! - [`recording`]: the plain-text format they all share.

pub mod diff;
pub mod proxy;
pub mod recording;
pub mod runner;
pub mod scenarios;
