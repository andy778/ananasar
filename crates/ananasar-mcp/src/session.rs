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
                return Err(Fail::soft(format!("unexpected reply function code {reply_func} to request {func}")));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::{TcpListener, TcpStream};
    use std::thread::{self, JoinHandle};

    const T: Duration = Duration::from_secs(5);

    /// A peer that reads one request and answers it with `reply(request)`.
    fn peer(reply: impl FnOnce(&[u8]) -> Vec<u8> + Send + 'static) -> (Session, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let handle = thread::spawn(move || {
            let (mut s, _) = listener.accept().expect("accept");
            // Fail instead of hanging when the code under test never sends a request.
            s.set_read_timeout(Some(T)).expect("read timeout");
            let mut req = [0u8; 12];
            s.read_exact(&mut req).expect("request");
            s.write_all(&reply(&req)).expect("reply");
        });
        (Session::open("127.0.0.1", port, 1).expect("open"), handle)
    }

    fn read_against(reply: impl FnOnce(&[u8]) -> Vec<u8> + Send + 'static, count: u16) -> Result<Vec<u16>, Fail> {
        let (mut session, handle) = peer(reply);
        let r = session.read(false, 0, count, T);
        handle.join().expect("peer");
        r
    }

    fn txn(req: &[u8]) -> u16 {
        u16::from_be_bytes([req[0], req[1]])
    }

    #[test]
    fn exception_names_cover_every_known_code() {
        let known = [
            (1, "Illegal Function"),
            (2, "Illegal Data Address"),
            (3, "Illegal Data Value"),
            (4, "Slave Device Failure"),
            (5, "Acknowledge"),
            (6, "Slave Device Busy"),
            (7, "Negative Acknowledge"),
            (8, "Memory Parity Error"),
            (10, "Gateway Path Unavailable"),
            (11, "Gateway Target Device Failed to Respond"),
        ];
        for (code, name) in known {
            assert_eq!(exception_name(code), name, "code {code}");
        }
        for code in [0, 9, 12, 255] {
            assert_eq!(exception_name(code), "unknown exception", "code {code}");
        }
    }

    #[test]
    fn empty_read_response_is_a_malformed_reply() {
        let r = read_against(|req| protocol::build(txn(req), 1, 3, &[]), 1);
        let e = r.expect_err("must fail");
        assert_eq!(e.msg, "malformed reply: empty read response");
        assert!(!e.fatal);
    }

    #[test]
    fn mismatched_byte_count_is_a_malformed_reply() {
        // Asked for one register (2 bytes); the peer claims and sends 4.
        let r = read_against(|req| protocol::build(txn(req), 1, 3, &[4, 0, 1, 0, 2]), 1);
        let e = r.expect_err("must fail");
        assert!(e.msg.starts_with("malformed reply: asked for 1 registers"), "{}", e.msg);
        assert!(!e.fatal);
    }

    #[test]
    fn byte_count_larger_than_the_data_is_a_malformed_reply() {
        // The byte count matches the request but the data is short.
        let r = read_against(|req| protocol::build(txn(req), 1, 3, &[2, 0]), 1);
        assert!(r.expect_err("must fail").msg.contains("got 1 data bytes (byte count 2)"));
    }

    #[test]
    fn well_formed_read_response_is_accepted() {
        let r = read_against(|req| protocol::build(txn(req), 1, 3, &[2, 0x12, 0x34]), 1);
        assert_eq!(r.ok(), Some(vec![0x1234]));
    }

    /// Read one whole request: the MBAP header says how many bytes follow it.
    fn read_request(s: &mut TcpStream) -> Vec<u8> {
        let mut head = [0u8; 6];
        s.read_exact(&mut head).expect("request header");
        let mut rest = vec![0u8; usize::from(u16::from_be_bytes([head[4], head[5]]))];
        s.read_exact(&mut rest).expect("request body");
        let mut req = head.to_vec();
        req.extend(rest);
        req
    }

    /// A peer for unit `unit` that runs `serve` on the accepted connection.
    fn serve(unit: u8, serve: impl FnOnce(TcpStream) + Send + 'static) -> (Session, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let handle = thread::spawn(move || {
            let (s, _) = listener.accept().expect("accept");
            s.set_read_timeout(Some(T)).expect("read timeout");
            serve(s);
        });
        (Session::open("127.0.0.1", port, unit).expect("open"), handle)
    }

    /// A peer that answers one request with function `func` and PDU data `pdu`.
    fn echo_peer(func: u8, pdu: Vec<u8>) -> (Session, JoinHandle<()>) {
        serve(1, move |mut s| {
            let req = read_request(&mut s);
            s.write_all(&protocol::build(txn(&req), 1, func, &pdu)).expect("reply");
        })
    }

    #[test]
    fn unit_id_is_the_configured_one_and_goes_into_the_request() {
        let (mut session, handle) = serve(7, |mut s| {
            let req = read_request(&mut s);
            assert_eq!(req[6], 7, "unit id in the request");
            s.write_all(&protocol::build(txn(&req), 7, 3, &[2, 0, 5])).expect("reply");
        });
        assert_eq!(session.unit_id(), 7);
        assert_eq!(session.read(false, 0, 1, T).ok(), Some(vec![5]));
        handle.join().expect("peer");
    }

    #[test]
    fn consecutive_requests_carry_different_transaction_ids() {
        let (mut session, handle) = serve(1, |mut s| {
            let first = txn(&read_request(&mut s));
            s.write_all(&protocol::build(first, 1, 3, &[2, 0, 1])).expect("reply");
            let second = txn(&read_request(&mut s));
            assert_ne!(first, second, "transaction ids must differ");
            s.write_all(&protocol::build(second, 1, 3, &[2, 0, 2])).expect("reply");
        });
        assert_eq!(session.read(false, 0, 1, T).ok(), Some(vec![1]));
        assert_eq!(session.read(false, 0, 1, T).ok(), Some(vec![2]));
        handle.join().expect("peer");
    }

    #[test]
    fn a_stale_reply_is_skipped_and_the_matching_one_returned() {
        let (mut session, handle) = serve(1, |mut s| {
            let t = txn(&read_request(&mut s));
            s.write_all(&protocol::build(t.wrapping_add(1), 1, 3, &[2, 0, 9])).expect("stale reply");
            s.write_all(&protocol::build(t, 1, 3, &[2, 0x12, 0x34])).expect("reply");
        });
        assert_eq!(session.read(false, 0, 1, T).ok(), Some(vec![0x1234]));
        handle.join().expect("peer");
    }

    #[test]
    fn a_connection_closed_mid_reply_is_fatal() {
        let (mut session, handle) = serve(1, |mut s| {
            let req = read_request(&mut s);
            let reply = protocol::build(txn(&req), 1, 3, &[2, 0, 1]);
            s.write_all(&reply[..4]).expect("partial reply");
        });
        let e = session.read(false, 0, 1, T).expect_err("must fail");
        assert!(e.fatal, "{}", e.msg);
        assert!(e.msg.contains("connection lost"), "{}", e.msg);
        handle.join().expect("peer");
    }

    #[test]
    fn a_peer_that_never_answers_is_a_soft_timeout() {
        let (done, wait) = std::sync::mpsc::channel::<()>();
        let (mut session, handle) = serve(1, move |mut s| {
            read_request(&mut s);
            // Keep the connection open until the client has given up.
            wait.recv_timeout(T).expect("client finished");
        });
        let e = session.read(false, 0, 1, Duration::from_millis(100)).expect_err("must fail");
        assert!(!e.fatal, "{}", e.msg);
        assert!(e.msg.contains("no reply from the device within 100 ms"), "{}", e.msg);
        done.send(()).expect("peer alive");
        handle.join().expect("peer");
    }

    const NO_ECHO: &str = "the device's reply does not echo the request";

    #[test]
    fn write_single_checks_the_echo() {
        let (mut ok, h) = echo_peer(6, vec![0, 10, 0x12, 0x34]);
        assert!(ok.write_single(10, 0x1234, T).is_ok());
        h.join().expect("peer");
        for pdu in [vec![0, 11, 0x12, 0x34], vec![0, 10, 0x12, 0x35]] {
            let (mut bad, h) = echo_peer(6, pdu);
            let e = bad.write_single(10, 0x1234, T).expect_err("must fail");
            assert!(e.msg.starts_with(NO_ECHO) && !e.fatal, "{}", e.msg);
            h.join().expect("peer");
        }
    }

    #[test]
    fn write_multiple_checks_the_echo() {
        let (mut ok, h) = echo_peer(16, vec![0, 10, 0, 2]);
        assert!(ok.write_multiple(10, &[1, 2], T).is_ok());
        h.join().expect("peer");
        for pdu in [vec![0, 11, 0, 2], vec![0, 10, 0, 1]] {
            let (mut bad, h) = echo_peer(16, pdu);
            let e = bad.write_multiple(10, &[1, 2], T).expect_err("must fail");
            assert!(e.msg.starts_with(NO_ECHO) && !e.fatal, "{}", e.msg);
            h.join().expect("peer");
        }
    }

    #[test]
    fn mask_write_checks_the_echo() {
        let (mut ok, h) = echo_peer(22, vec![0, 10, 0xF0, 0xF0, 0x0A, 0x0A]);
        assert!(ok.mask_write(10, 0xF0F0, 0x0A0A, T).is_ok());
        h.join().expect("peer");
        for pdu in [
            vec![0, 11, 0xF0, 0xF0, 0x0A, 0x0A],
            vec![0, 10, 0xF0, 0xF1, 0x0A, 0x0A],
            vec![0, 10, 0xF0, 0xF0, 0x0A, 0x0B],
        ] {
            let (mut bad, h) = echo_peer(22, pdu);
            let e = bad.mask_write(10, 0xF0F0, 0x0A0A, T).expect_err("must fail");
            assert!(e.msg.starts_with(NO_ECHO) && !e.fatal, "{}", e.msg);
            h.join().expect("peer");
        }
    }
}
