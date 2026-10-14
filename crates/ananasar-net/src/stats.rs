//! Live server statistics for a status display: per connection, what the last
//! request looked like and the traffic totals, plus error flags.

use ananasar_core::protocol::{self, Function};

/// Number of client slots, matching [`crate::MAX_CLIENTS`].
pub const SLOTS: usize = 2;

/// What one connected client has sent so far.
#[derive(Clone, Debug, Default)]
pub struct ConnStats {
    /// The client's IP address.
    pub ip: String,
    /// Whether Nagle's algorithm is enabled on the socket.
    pub nagle: bool,
    /// Time between the last two requests, in milliseconds.
    pub poll_interval_ms: Option<u64>,
    /// Size in bytes of the last request.
    pub packet_size: usize,
    /// Result of the last header check: `None` before any request, otherwise
    /// `Ok(())` or a description of what was wrong.
    pub header_check: Option<Result<(), String>>,
    /// Transaction id of the last request.
    pub transaction_id: Option<u16>,
    /// Protocol id of the last request.
    pub protocol_id: Option<u16>,
    /// MBAP length field of the last request.
    pub length: Option<u16>,
    /// Unit id of the last request.
    pub unit_id: Option<u8>,
    /// Function code of the last request.
    pub function: Option<u8>,
    /// Start address of the last request, for functions that carry one.
    pub start_address: Option<u16>,
    /// Register count of the last request, for functions that carry one.
    pub word_count: Option<u16>,
    /// Bytes received from this client.
    pub bytes_received: u64,
    /// Bytes sent to this client.
    pub bytes_sent: u64,
}

/// Statistics for both client slots plus error flags for the last request.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    /// The two client slots; `None` while a slot is free.
    pub slots: [Option<ConnStats>; SLOTS],
    /// The last request had a length field the stream could not follow.
    pub out_of_sync: bool,
    /// The last request's length field disagreed with the bytes received.
    pub incorrect_length: bool,
    /// The last request had a non-zero protocol id.
    pub incorrect_protocol_id: bool,
    /// Exception code of the last reply, if it was an exception.
    pub return_exception: Option<u8>,
}

impl Stats {
    /// Claim the lowest free slot for a new client and return its index.
    pub fn connect(&mut self, ip: &str, nagle: bool) -> Option<usize> {
        let slot = self.slots.iter().position(Option::is_none)?;
        self.slots[slot] = Some(ConnStats { ip: ip.to_owned(), nagle, ..ConnStats::default() });
        Some(slot)
    }

    /// Free a slot when its client disconnects.
    pub fn disconnect(&mut self, slot: usize) {
        if let Some(s) = self.slots.get_mut(slot) {
            *s = None;
        }
    }

    /// Record a request received on `slot` (and reset the per-request flags).
    pub fn record_request(&mut self, slot: usize, request: &[u8], interval_ms: Option<u64>) {
        self.out_of_sync = false;
        self.incorrect_length = false;
        self.incorrect_protocol_id = false;
        self.return_exception = None;
        let Some(Some(c)) = self.slots.get_mut(slot) else { return };
        c.packet_size = request.len();
        c.bytes_received += request.len() as u64;
        if interval_ms.is_some() {
            c.poll_interval_ms = interval_ms;
        }
        match protocol::frame(request) {
            Ok((hdr, func, pdu)) => {
                c.header_check = Some(Ok(()));
                c.transaction_id = Some(hdr.transaction_id);
                c.protocol_id = Some(hdr.protocol_id);
                c.length = Some(hdr.length);
                c.unit_id = Some(hdr.unit_id);
                c.function = Some(func);
                c.start_address = pdu.get(..2).map(|b| u16::from_be_bytes([b[0], b[1]]));
                c.word_count = match Function::from_u8(func) {
                    Some(Function::ReadHolding | Function::ReadInput | Function::WriteMultiple) => {
                        pdu.get(2..4).map(|b| u16::from_be_bytes([b[0], b[1]]))
                    }
                    _ => None,
                };
            }
            Err(e) => {
                c.header_check = Some(Err(e.to_string()));
                match e {
                    protocol::FrameError::BadHeader => self.incorrect_protocol_id = true,
                    protocol::FrameError::BadLength => self.incorrect_length = true,
                    protocol::FrameError::TooShort => {}
                }
            }
        }
    }

    /// Record the reply sent on `slot` for the last request.
    pub fn record_reply(&mut self, slot: usize, reply: &[u8]) {
        if let Some(Some(c)) = self.slots.get_mut(slot) {
            c.bytes_sent += reply.len() as u64;
        }
        // The MBAP prefix is followed by the function code; an exception reply
        // has its high bit set and the exception code next.
        self.return_exception = match reply.get(protocol::MBAP_PREFIX..protocol::MBAP_PREFIX + 2) {
            Some([func, code]) if func & 0x80 != 0 => Some(*code),
            _ => None,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::build;

    #[test]
    fn slots_fill_lowest_first_and_free_again() {
        let mut s = Stats::default();
        assert_eq!(s.connect("10.0.0.1", true), Some(0));
        assert_eq!(s.connect("10.0.0.2", true), Some(1));
        assert_eq!(s.connect("10.0.0.3", true), None);
        s.disconnect(0);
        assert_eq!(s.connect("10.0.0.4", false), Some(0));
        assert_eq!(s.slots[0].as_ref().unwrap().ip, "10.0.0.4");
    }

    #[test]
    fn records_request_fields_and_flags() {
        let mut s = Stats::default();
        let slot = s.connect("10.0.0.1", true).unwrap();
        s.record_request(slot, &build(9, 1, 0x03, &[0, 10, 0, 4]), Some(250));
        let c = s.slots[slot].as_ref().unwrap();
        assert_eq!((c.transaction_id, c.unit_id, c.function), (Some(9), Some(1), Some(3)));
        assert_eq!((c.start_address, c.word_count, c.poll_interval_ms), (Some(10), Some(4), Some(250)));
        assert_eq!(c.header_check, Some(Ok(())));
        assert_eq!(c.bytes_received, 12);
        let mut bad = build(9, 1, 0x03, &[0, 10, 0, 4]);
        bad[2] = 1;
        s.record_request(slot, &bad, None);
        assert!(s.incorrect_protocol_id);
        assert!(matches!(s.slots[slot].as_ref().unwrap().header_check, Some(Err(_))));
        s.record_request(slot, &build(9, 1, 0x03, &[0, 10, 0, 4]), None);
        assert!(!s.incorrect_protocol_id, "flags reset on the next request");
    }

    #[test]
    fn exception_replies_are_noted() {
        let mut s = Stats::default();
        let slot = s.connect("10.0.0.1", true).unwrap();
        s.record_reply(slot, &build(1, 1, 0x83, &[2]));
        assert_eq!(s.return_exception, Some(2));
        s.record_reply(slot, &build(1, 1, 0x03, &[2, 0, 1]));
        assert_eq!(s.return_exception, None);
        assert_eq!(s.slots[slot].as_ref().unwrap().bytes_sent, 9 + 11);
    }
}
