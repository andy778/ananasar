//! Shared TCP framing helpers for reading a whole Modbus/TCP frame.

use ananasar_core::protocol;
use std::io::Read;
use std::net::TcpStream;

/// Read one complete frame: the MBAP prefix + function byte, then the rest as
/// declared by the length field.
pub fn read_frame(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut head = [0u8; protocol::MBAP_PREFIX + 1];
    read_exact(stream, &mut head)?;
    let total = 6 + u16::from_be_bytes([head[4], head[5]]) as usize;
    let mut out = head.to_vec();
    out.resize(total.max(head.len()), 0);
    read_exact(stream, &mut out[head.len()..])?;
    Ok(out)
}

pub fn read_exact(stream: &mut TcpStream, buf: &mut [u8]) -> std::io::Result<()> {
    let mut filled = 0;
    while filled < buf.len() {
        match stream.read(&mut buf[filled..]) {
            Ok(0) => return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof")),
            Ok(n) => filled += n,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
