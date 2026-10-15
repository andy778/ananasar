//! DDE server for the shared register file. See the crate README for why this
//! crate, unlike the rest of the workspace, contains `unsafe` FFI.

pub mod item;
#[cfg(windows)]
#[allow(unsafe_code)]
mod server;

#[cfg(windows)]
pub use server::{Server, StartError};

/// The DDE service name clients connect to.
pub const SERVICE: &str = "Ananasar";
