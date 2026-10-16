//! Ananasar's own server, started in-process on a loopback port, set up the
//! way a scenario profile asks. Used by the golden test and the cross-run to
//! produce "our" side of a comparison.

use crate::scenarios::PROFILES;
use ananasar_core::protocol::Exception;
use ananasar_core::registers::RegisterFile;
use ananasar_core::server::ServerConfig;
use ananasar_net::Server;
use std::io;
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// A running server that stops when dropped.
pub struct Running {
    /// Where it listens.
    pub addr: SocketAddr,
    stop: Arc<AtomicBool>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// The exception a `exception-XX` profile forces, from its two hex digits.
fn exception_from_code(code: u8) -> Option<Exception> {
    Some(match code {
        0x01 => Exception::IllegalFunction,
        0x02 => Exception::IllegalDataAddress,
        0x03 => Exception::IllegalDataValue,
        0x04 => Exception::SlaveDeviceFailure,
        0x05 => Exception::Acknowledge,
        0x06 => Exception::SlaveDeviceBusy,
        0x07 => Exception::NegativeAcknowledge,
        0x08 => Exception::MemoryParityError,
        0x0A => Exception::GatewayPathUnavailable,
        0x0B => Exception::GatewayTargetFailed,
        _ => return None,
    })
}

/// The server configuration a profile stands for, or `None` when Ananasar has
/// no counterpart for the profile's option.
pub fn server_config(profile: &str) -> Option<ServerConfig> {
    if !PROFILES.iter().any(|p| p.name == profile) {
        return None;
    }
    let mut cfg = ServerConfig::default();
    if profile == "default" {
        return Some(cfg);
    }
    if let Some(fc) = profile.strip_prefix("disable-fc") {
        let fc: u8 = fc.parse().ok()?;
        cfg.supported.remove(&fc);
        return Some(cfg);
    }
    if let Some(code) = profile.strip_prefix("exception-") {
        cfg.forced_exception = Some(exception_from_code(u8::from_str_radix(code, 16).ok()?)?);
        return Some(cfg);
    }
    match profile {
        "fault-ti" => cfg.wrong_transaction_id = true,
        "fault-protocol-id" => cfg.wrong_protocol_id = true,
        "fault-length-high" => cfg.wrong_length_high = true,
        "fault-out-of-synch" => cfg.out_of_synch = true,
        "address-offset" => cfg.address_offset = true,
        _ => return None,
    }
    Some(cfg)
}

/// Start a server with all registers zero and `cfg`, on a free loopback port.
pub fn start(cfg: ServerConfig) -> io::Result<Running> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let stop = Arc::new(AtomicBool::new(false));
    let server = Server {
        regs: Arc::new(Mutex::new(RegisterFile::new())),
        cfg: Arc::new(Mutex::new(cfg)),
        clients: Arc::default(),
        stop: stop.clone(),
        notify: Arc::new(|_| {}),
        log: None,
        stats: Arc::default(),
    };
    std::thread::spawn(move || server.run(&listener));
    Ok(Running { addr, stop })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_map_to_configs() {
        assert!(server_config("default").is_some());
        let cfg = server_config("disable-fc22").unwrap();
        assert!(!cfg.supported.contains(&0x16) && cfg.supported.contains(&0x03));
        let cfg = server_config("exception-0b").unwrap();
        assert_eq!(cfg.forced_exception, Some(Exception::GatewayTargetFailed));
        assert!(server_config("fault-ti").is_some_and(|c| c.wrong_transaction_id));
        assert!(server_config("address-offset").is_some_and(|c| c.address_offset));
        assert!(server_config("nonsense").is_none());
    }

    #[test]
    fn every_profile_has_a_config() {
        for p in PROFILES {
            assert!(server_config(p.name).is_some(), "{}", p.name);
        }
    }
}
