//! Malformed and unusual framing: unit ids, headers, lengths, oversize PDUs,
//! pipelining and requests split across TCP segments.

use super::{Builder, Scenario};
use ananasar_core::protocol::build;

pub(super) fn scenarios(unit: u8) -> Vec<Scenario> {
    let b = |name, about| Builder::new(name, about, unit);
    // A header with explicit protocol id and length, for malformed frames.
    let header = |protocol: u16, length: u16, rest: &[u8]| {
        let mut v = vec![0, 1];
        v.extend_from_slice(&protocol.to_be_bytes());
        v.extend_from_slice(&length.to_be_bytes());
        v.push(unit);
        v.extend_from_slice(rest);
        v
    };
    let read_one = [3u8, 0, 0, 0, 1];
    let read_pdu = [0u8, 0, 0, 1];
    let at_unit = |name, about, u: u8| b(name, about).raw(build(1, u, 3, &read_pdu)).done();
    let frame_a = build(1, unit, 3, &[0, 0, 0, 1]);
    let frame_b = build(2, unit, 4, &[0, 0, 0, 1]);
    let frame_c = build(3, unit, 3, &[0, 1, 0, 1]);

    vec![
        at_unit("unit_zero", "FC3 for unit 0 (broadcast address).", 0),
        at_unit("unit_one", "FC3 for unit 1.", 1),
        at_unit("unit_two", "FC3 for unit 2.", 2),
        at_unit("unit_247", "FC3 for unit 247, the last assigned address.", 247),
        at_unit("unit_248", "FC3 for unit 248, the first reserved address.", 248),
        at_unit("unit_255", "FC3 for unit 255, the TCP default.", 255),
        b("bad_protocol_id", "Protocol id 1 then 0: the bad frame is ignored, the next one answered.")
            .raw(header(1, 6, &read_one))
            .raw(header(0, 6, &read_one))
            .done(),
        b("bad_protocol_id_high_byte", "Protocol id 0x0100.").raw(header(0x0100, 6, &read_one)).done(),
        b("bad_protocol_id_ffff", "Protocol id 0xFFFF.").raw(header(0xFFFF, 6, &read_one)).done(),
        b("bad_length_high_byte", "Length 0x0106 with a six-byte body.").raw(header(0, 0x0106, &read_one)).done(),
        b("bad_length_too_long", "Length 16 with a six-byte body: the server waits for more.")
            .raw(header(0, 16, &read_one))
            .done(),
        b("bad_length_too_short", "Length 5 with a six-byte body.").raw(header(0, 5, &read_one)).done(),
        b("bad_length_too_short_then_valid", "Length 5 with a six-byte body, then a valid request.")
            .raw(header(0, 5, &read_one))
            .pdu(3, &read_pdu)
            .done(),
        b("length_zero", "Length 0.").raw(header(0, 0, &read_one)).done(),
        b("length_one", "Length 1 (unit id only, no function code).").raw(header(0, 1, &read_one)).done(),
        b("length_two_function_only", "Length 2: unit id and a function code with no data.")
            .raw(vec![0, 1, 0, 0, 0, 2, unit, 3])
            .done(),
        b("header_only", "Seven bytes: a header and unit id, no function code.")
            .raw(vec![0, 1, 0, 0, 0, 1, unit])
            .done(),
        b("short_header_five_bytes", "Five bytes only.").raw(vec![0, 1, 0, 0, 0]).done(),
        b("single_byte", "One byte only.").raw(vec![0x2A]).done(),
        b("oversize_pdu_fc3", "FC3 with a 253-byte payload: the PDU is 254 bytes, one over the limit.")
            .pdu(3, &[&[0, 0, 0, 1][..], &[0xEE; 249]].concat())
            .done(),
        b("max_size_pdu_fc16", "FC16 with the largest legal PDU (123 registers, a 252-byte PDU).")
            .write_multiple(2000, &vec![0x4242; 123])
            .read(3, 2000, 2)
            .done(),
        b("oversize_pdu_fc16_trailing", "FC16 of 123 registers plus 2 extra bytes, length 0x00FF.")
            .pdu(0x10, &[&[0x07, 0xD0, 0, 123, 246][..], &[0x43; 246], &[1, 2]].concat())
            .done(),
        b("pipelined", "Two requests in one TCP segment.").raw([frame_a.clone(), frame_b.clone()].concat()).done(),
        b("pipelined_three", "Three requests in one TCP segment.")
            .raw([frame_a.clone(), frame_b.clone(), frame_c].concat())
            .done(),
        b("pipelined_second_bad", "A valid request followed by one with a bad protocol id in one segment.")
            .raw([frame_a.clone(), header(1, 6, &read_one)].concat())
            .done(),
        b("pipelined_partial_second", "A request and the first five bytes of the next, then the rest.")
            .raw([frame_a.clone(), frame_b.get(..5).unwrap_or_default().to_vec()].concat())
            .raw(frame_b.get(5..).unwrap_or_default().to_vec())
            .done(),
        b("trailing_garbage_then_valid", "A request with two stray bytes behind it, then a valid request.")
            .raw([frame_a.clone(), vec![0xDE, 0xAD]].concat())
            .raw(frame_b.clone())
            .done(),
        b("split_after_header", "A request split after the 7-byte MBAP header.")
            .raw_split(frame_a.clone(), &[7])
            .done(),
        b("split_in_length_field", "A request split inside the length field.")
            .raw_split(frame_a.clone(), &[5])
            .done(),
        b("split_before_data", "A request split after the function code.")
            .raw_split(frame_a.clone(), &[8])
            .done(),
        b("split_one_byte_at_a_time", "A request sent one byte per TCP segment.")
            .raw_split(frame_a.clone(), &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11])
            .done(),
        b("split_two_requests_mid_frame", "Two requests in one write, cut inside the second.")
            .raw_split([frame_a, frame_b].concat(), &[16])
            .done(),
    ]
}
