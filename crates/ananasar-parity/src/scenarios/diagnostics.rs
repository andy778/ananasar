//! FC7 (read exception status) and FC8 (diagnostics) with its sub-functions.

use super::{Builder, Scenario};

pub(super) fn scenarios(unit: u8) -> Vec<Scenario> {
    let b = |name, about| Builder::new(name, about, unit);
    let sub = |name, about, sub: u16, data: u16| {
        b(name, about).pdu(8, &[sub.to_be_bytes(), data.to_be_bytes()].concat()).done()
    };
    vec![
        b("read_exception_status", "FC7 with no data.").pdu(7, &[]).done(),
        b("read_exception_status_extra_data", "FC7 with bytes after the function code.").pdu(7, &[1, 2]).done(),
        b("diagnostics", "FC8 sub-function 0 (return query data).").pdu(8, &[0, 0, 0xA5, 0x37]).done(),
        sub("diag_0000_zero_data", "FC8 sub 0 with data 0.", 0x0000, 0),
        sub("diag_0001_restart", "FC8 sub 1 (restart communications), data 0.", 0x0001, 0),
        sub("diag_0001_restart_clear_log", "FC8 sub 1 with data 0xFF00.", 0x0001, 0xFF00),
        sub("diag_0002_register", "FC8 sub 2 (return diagnostic register).", 0x0002, 0),
        sub("diag_0003_delimiter", "FC8 sub 3 (change ASCII input delimiter).", 0x0003, 0x0A00),
        sub("diag_0004_listen_only", "FC8 sub 4 (force listen only mode).", 0x0004, 0),
        sub("diag_000a_clear_counters", "FC8 sub 10 (clear counters and diagnostic register).", 0x000A, 0),
        sub("diag_000b_bus_message_count", "FC8 sub 11 (return bus message count).", 0x000B, 0),
        sub("diag_000c_bus_error_count", "FC8 sub 12 (return bus communication error count).", 0x000C, 0),
        sub("diag_000d_exception_count", "FC8 sub 13 (return bus exception error count).", 0x000D, 0),
        sub("diag_000e_server_message_count", "FC8 sub 14 (return server message count).", 0x000E, 0),
        sub("diag_000f_no_response_count", "FC8 sub 15 (return server no response count).", 0x000F, 0),
        sub("diag_0010_nak_count", "FC8 sub 16 (return server NAK count).", 0x0010, 0),
        sub("diag_0011_busy_count", "FC8 sub 17 (return server busy count).", 0x0011, 0),
        sub("diag_0012_overrun_count", "FC8 sub 18 (return bus character overrun count).", 0x0012, 0),
        sub("diag_0014_clear_overrun", "FC8 sub 20 (clear overrun counter and flag).", 0x0014, 0),
        sub("diag_00ff_unknown_sub", "FC8 with an unassigned sub-function.", 0x00FF, 0),
        sub("diag_ffff_unknown_sub", "FC8 with sub-function 0xFFFF.", 0xFFFF, 0xFFFF),
        b("diag_query_data_long", "FC8 sub 0 echoing 20 data bytes.")
            .pdu(8, &[&[0, 0][..], &(1..=20).collect::<Vec<u8>>()].concat())
            .done(),
        b("diag_empty_pdu", "FC8 with no data.").pdu(8, &[]).done(),
        b("diag_one_byte_pdu", "FC8 with half a sub-function.").pdu(8, &[0]).done(),
        b("diag_sub_only_pdu", "FC8 sub 0 with no data field.").pdu(8, &[0, 0]).done(),
    ]
}
