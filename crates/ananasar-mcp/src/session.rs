//! The Modbus/TCP client connection behind the `modbus_*` tools.

use ananasar_core::client::{ManualCommand, PollEngine};
use ananasar_core::protocol::{self, Function};
use ananasar_net::{connect, parse_endpoint, read_frame};
use std::io::{self, Write};
use std::net::TcpStream;
use std::time::Duration;

/// How many unrelated frames (stale replies) to skip while waiting for ours.
const MAX_STALE: usize = 16;

/// Why a request failed.
pub struct Fail {
    /// Text for the assistant.
    pub msg: String,
    /// The connection is unusable and has been closed by the caller.
    pub fatal: bool,
}

impl Fail {
    fn soft(msg: impl Into<String>) -> Self {
        Self { msg: msg.into(), fatal: false }
    }
    fn fatal(msg: impl Into<String>) -> Self {
        Self { msg: msg.into(), fatal: true }
    }
}

/// An open connection to one device.
pub struct Session {
    stream: TcpStream,
    engine: PollEngine,
    /// Host the session was opened to.
    pub host: String,
    /// Port the session was opened to.
    pub port: u16,
}

/// The name of a Modbus exception code.
pub fn exception_name(code: u8) -> &'static str {
    match code {
        1 => "Illegal Function",
        2 => "Illegal Data Address",
        3 => "Illegal Data Value",
        4 => "Slave Device Failure",
        5 => "Acknowledge",
        6 => "Slave Device Busy",
        7 => "Negative Acknowledge",
        8 => "Memory Parity Error",
        10 => "Gateway Path Unavailable",
        11 => "Gateway Target Device Failed to Respond",
        _ => "unknown exception",
    }
}

fn pair(addr: u16, second: u16) -> Vec<u8> {
    let mut v = addr.to_be_bytes().to_vec();
    v.extend_from_slice(&second.to_be_bytes());
    v
}

impl Session {
    /// Connect to `host:port` and address `unit_id`.
    pub fn open(host: &str, port: u16, unit_id: u8) -> Result<Self, String> {
        let (host, port) = parse_endpoint(host, &port.to_string())?;
        let stream = connect(&host, port, true).map_err(|e| format!("cannot connect to {host}:{port}: {e}"))?;
        // Requests are small and latency matters more than packing.
        stream.set_nodelay(true).ok();
        Ok(Self { stream, engine: PollEngine::new(unit_id), host, port })
    }

    /// Unit id used in requests.
    pub fn unit_id(&self) -> u8 {
        self.engine.unit_id
    }

    fn next_txn(&mut self) -> u16 {
        let t = self.engine.txn;
        self.engine.txn = t.wrapping_add(1);
        t
    }

    /// Read `count` registers (FC3 or FC4) from `addr`.
    pub fn read(&mut self, input: bool, addr: u16, count: u16, timeout: Duration) -> Result<Vec<u16>, Fail> {
        let func = if input { Function::ReadInput } else { Function::ReadHolding };
        let txn = self.next_txn();
        let req = protocol::build(txn, self.engine.unit_id, func as u8, &pair(addr, count));
        let pdu = self.exchange(&req, timeout)?;
        let (Some(&bytes), Some(data)) = (pdu.first(), pdu.get(1..)) else {
            return Err(Fail::soft("malformed reply: empty read response"));
        };
        if usize::from(bytes) != usize::from(count).saturating_mul(2) || data.len() != usize::from(bytes) {
            return Err(Fail::soft(format!(
                "malformed reply: asked for {count} registers, got {} data bytes (byte count {bytes})",
                data.len()
            )));
        }
        Ok(data.as_chunks::<2>().0.iter().map(|p| u16::from_be_bytes(*p)).collect())
    }

    /// FC16: write `values` from `addr`.
    pub fn write_multiple(&mut self, addr: u16, values: &[u16], timeout: Duration) -> Result<(), Fail> {
        let count = u16::try_from(values.len()).map_err(|_| Fail::soft("too many values"))?;
        let mut data = pair(addr, count);
        data.push(u8::try_from(values.len().saturating_mul(2)).map_err(|_| Fail::soft("too many values"))?);
        for v in values {
            data.extend_from_slice(&v.to_be_bytes());
        }
        let txn = self.next_txn();
        let req = protocol::build(txn, self.engine.unit_id, Function::WriteMultiple as u8, &data);
        let pdu = self.exchange(&req, timeout)?;
        check_echo(&pdu, &pair(addr, count))
    }

    /// FC6: write one register.
    pub fn write_single(&mut self, addr: u16, value: u16, timeout: Duration) -> Result<(), Fail> {
        let req = self.engine.build_manual(ManualCommand::WriteSingle { addr, value });
        let pdu = self.exchange(&req, timeout)?;
        check_echo(&pdu, &pair(addr, value))
    }

    /// FC22: mask write one register.
    pub fn mask_write(&mut self, addr: u16, and_mask: u16, or_mask: u16, timeout: Duration) -> Result<(), Fail> {
        let req = self.engine.build_manual(ManualCommand::MaskWrite { addr, and_mask, or_mask });
        let pdu = self.exchange(&req, timeout)?;
        let mut expected = pair(addr, and_mask);
        expected.extend_from_slice(&or_mask.to_be_bytes());
        check_echo(&pdu, &expected)
    }

    /// FC7: read the exception status byte.
    pub fn exception_status(&mut self, timeout: Duration) -> Result<u8, Fail> {
        let req = self.engine.build_manual(ManualCommand::ReadExceptionStatus);
        let pdu = self.exchange(&req, timeout)?;
        pdu.first().copied().ok_or_else(|| Fail::soft("malformed reply: empty exception status"))
    }

    /// Send `request` and return the PDU data of the matching reply.
    fn exchange(&mut self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, Fail> {
        let (sent, func, _) = protocol::frame(request).map_err(|e| Fail::soft(e.to_string()))?;
        self.stream.set_read_timeout(Some(timeout)).ok();
        self.stream.set_write_timeout(Some(timeout)).ok();
        self.stream.write_all(request).map_err(|e| Fail::fatal(format!("send failed: {e}")))?;
        for _ in 0..MAX_STALE {
            let buf = match read_frame(&mut self.stream) {
                Ok(b) => b,
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                    return Err(Fail::soft(format!(
                        "no reply from the device within {} ms (timeout); wrong unit id, address, or the device is not answering",
                        timeout.as_millis()
                    )));
                }
                Err(e) => return Err(Fail::fatal(format!("connection lost while waiting for the reply: {e}"))),
            };
            let (hdr, reply_func, pdu) =
                protocol::frame(&buf).map_err(|e| Fail::soft(format!("malformed reply: {e}")))?;
            if hdr.transaction_id != sent.transaction_id {
                continue; // a late reply to an earlier, timed-out request
            }
            if reply_func == func | 0x80 {
                let code = pdu.first().copied().unwrap_or(0);
                return Err(Fail::soft(format!(
                    "device answered with Modbus exception {code} ({})",
                    exception_name(code)
                )));
            }
            if reply_func != func {
                return Err(Fail::soft(format!(
                    "unexpected reply function code {reply_func} to request {func}"
                )));
            }
            return Ok(pdu.to_vec());
        }
        Err(Fail::soft("no reply with a matching transaction id"))
    }
}

fn check_echo(pdu: &[u8], expected: &[u8]) -> Result<(), Fail> {
    if pdu == expected {
        Ok(())
    } else {
        Err(Fail::soft("the device's reply does not echo the request; the write may not have taken effect"))
    }
}
