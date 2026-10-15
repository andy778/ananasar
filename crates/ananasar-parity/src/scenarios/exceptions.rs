//! Every exception the server can raise in the default profile: illegal
//! function (unknown codes), illegal data address and illegal data value.
//! Disabled functions and forced exceptions are separate profiles.

use super::{Builder, Scenario};

pub(super) fn scenarios(unit: u8) -> Vec<Scenario> {
    let b = |name, about| Builder::new(name, about, unit);
    vec![
        b("unknown_function", "Function codes 0, 1, 0x2B and 0x63 get illegal function.")
            .pdu(0x00, &[])
            .pdu(0x01, &[0, 0, 0, 1])
            .pdu(0x2B, &[0x0E, 1, 0])
            .pdu(0x63, &[1, 2, 3])
            .done(),
        b(
            "unknown_function_boundaries",
            "Unassigned and out-of-range function codes: 5, 15, 17, 20, 21, 24, 0x7F, 0x80, 0xFF.",
        )
        .pdu(0x05, &[0, 0, 0xFF, 0])
        .pdu(0x0F, &[0, 0, 0, 1, 1, 1])
        .pdu(0x11, &[])
        .pdu(0x14, &[0x07, 6, 0, 4, 0, 1, 0, 2])
        .pdu(0x15, &[0x0B, 6, 0, 4, 0, 1, 0, 1, 0, 1])
        .pdu(0x18, &[0, 0])
        .pdu(0x7F, &[])
        .pdu(0x80, &[])
        .pdu(0xFF, &[])
        .done(),
        b("exception_illegal_address_read", "Reads that run past the register file give illegal data address.")
            .read(3, 65535, 2)
            .read(4, 65000, 1000)
            .done(),
        b("exception_illegal_value_read", "Read quantities 0 and 126 give illegal data value.")
            .read(3, 0, 0)
            .read(4, 0, 126)
            .done(),
        b("exception_illegal_value_write", "Write quantities 0 and 124 and a wrong byte count give illegal data value.")
            .pdu(0x10, &[0x0F, 0xC0, 0, 0, 0])
            .write_multiple(1000, &[0; 124])
            .pdu(0x10, &[0x0F, 0xC2, 0, 2, 3, 0, 1, 0, 2])
            .done(),
        b(
            "exception_precedence_address_vs_value",
            "A request with both a bad quantity and a bad address: which exception wins.",
        )
        .read(3, 65535, 126)
        .pdu(0x10, &[0xFF, 0xFF, 0, 124, 248])
        .done(),
        b("exception_reply_keeps_unit", "An illegal function reply keeps the unit id and sets the top bit of the function code.")
            .pdu_for_unit(7, 0x42, &[1, 2, 3])
            .done(),
    ]
}
