//! Modbus/TCP wire protocol: the MBAP header and PDU encoding.
//!
//! Field layout matches the outbound buffer the original builds at `DAT_006d41f9`
//! and the server parse in `FUN_00686ef0`:
//!
//! ```text
//! offset  field
//! 0..2    transaction id   (big-endian)
//! 2..4    protocol id      (always 0 for Modbus/TCP)
//! 4..6    length           (bytes following = unit id + PDU)
//! 6       unit id          (a.k.a. UID / slave address)
//! 7       function code
//! 8..     PDU data
//! ```

/// Length of the MBAP header up to and including the unit id.
pub const MBAP_PREFIX: usize = 7;
/// Protocol identifier for Modbus/TCP.
pub const PROTOCOL_ID: u16 = 0;
/// The server listens here by default (manual §2.1).
pub const DEFAULT_PORT: u16 = 502;

/// Modbus function codes the application understands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Function {
    ReadHolding = 0x03,
    ReadInput = 0x04,
    WriteSingle = 0x06,
    ReadExceptionStatus = 0x07,
    Diagnostics = 0x08,
    WriteMultiple = 0x10,
    MaskWrite = 0x16,
    ReadWriteMultiple = 0x17,
}

impl Function {
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0x03 => Self::ReadHolding,
            0x04 => Self::ReadInput,
            0x06 => Self::WriteSingle,
            0x07 => Self::ReadExceptionStatus,
            0x08 => Self::Diagnostics,
            0x10 => Self::WriteMultiple,
            0x16 => Self::MaskWrite,
            0x17 => Self::ReadWriteMultiple,
            _ => return None,
        })
    }
}

/// Modbus exception codes (manual §1.4). The server returns these with the
/// function code's MSB set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Exception {
    IllegalFunction = 0x01,
    IllegalDataAddress = 0x02,
    IllegalDataValue = 0x03,
    SlaveDeviceFailure = 0x04,
    Acknowledge = 0x05,
    SlaveDeviceBusy = 0x06,
    NegativeAcknowledge = 0x07,
    MemoryParityError = 0x08,
    GatewayPathUnavailable = 0x0A,
    GatewayTargetFailed = 0x0B,
}

/// A parsed MBAP header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mbap {
    pub transaction_id: u16,
    pub protocol_id: u16,
    pub length: u16,
    pub unit_id: u8,
}

impl Mbap {
    pub fn write_prefix(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.transaction_id.to_be_bytes());
        out.extend_from_slice(&self.protocol_id.to_be_bytes());
        out.extend_from_slice(&self.length.to_be_bytes());
        out.push(self.unit_id);
    }
}

/// Errors from framing a received buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// Fewer than `MBAP_PREFIX + 1` bytes — cannot even read the header + FC.
    TooShort,
    /// Protocol id was non-zero, or the length high byte was set
    /// (the two header checks the server performs first).
    BadHeader,
    /// `length` disagrees with the number of bytes actually present.
    BadLength,
}

/// Split a received buffer into its header, function byte and PDU payload.
///
/// Mirrors the server's leading validation: protocol id must be 0, the length
/// field's upper byte must be 0, and `length` must equal the trailing byte
/// count (`received - 6`).
pub fn frame(buf: &[u8]) -> Result<(Mbap, u8, &[u8]), FrameError> {
    if buf.len() < MBAP_PREFIX + 1 {
        return Err(FrameError::TooShort);
    }
    let transaction_id = u16::from_be_bytes([buf[0], buf[1]]);
    let protocol_id = u16::from_be_bytes([buf[2], buf[3]]);
    let length = u16::from_be_bytes([buf[4], buf[5]]);
    let unit_id = buf[6];
    // protocol id 0 and length high byte 0 (buf[4]) — the original's first checks.
    if protocol_id != PROTOCOL_ID || buf[4] != 0 {
        return Err(FrameError::BadHeader);
    }
    // length counts unit id + PDU, i.e. everything after the length field.
    if length as usize != buf.len() - 6 {
        return Err(FrameError::BadLength);
    }
    let function = buf[7];
    Ok((
        Mbap { transaction_id, protocol_id, length, unit_id },
        function,
        &buf[8..],
    ))
}

/// Build a complete frame from a PDU (function byte + data), filling in the
/// MBAP header and the length field automatically.
pub fn build(transaction_id: u16, unit_id: u8, function: u8, data: &[u8]) -> Vec<u8> {
    let length = (2 + data.len()) as u16; // unit id + function + data
    let mut out = Vec::with_capacity(MBAP_PREFIX + 1 + data.len());
    Mbap { transaction_id, protocol_id: PROTOCOL_ID, length, unit_id }.write_prefix(&mut out);
    out.push(function);
    out.extend_from_slice(data);
    out
}
