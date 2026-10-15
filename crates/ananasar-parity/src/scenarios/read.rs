//! FC3 and FC4 reads, quantity and address boundaries, malformed read PDUs.

use super::{Builder, Scenario, ramp};

pub(super) fn scenarios(unit: u8) -> Vec<Scenario> {
    let b = |name, about| Builder::new(name, about, unit);
    vec![
        b("seed", "Fill holding registers 0..125 with a ramp so later reads return known values.")
            .write_multiple(0, &ramp(100))
            .write_multiple(100, &ramp(25))
            .done(),
        b("read_holding", "FC3 reads of 1, 10 and 5 registers (the last near the seeded end).")
            .read(3, 0, 1)
            .read(3, 0, 10)
            .read(3, 120, 5)
            .done(),
        b("read_input", "FC4 reads; the input bank is all zero.").read(4, 0, 1).read(4, 0, 10).done(),
        b("read_holding_limit", "FC3 for the 125 register maximum.").read(3, 0, 125).done(),
        b("read_holding_over_limit", "FC3 for 126 registers, one over the maximum.").read(3, 0, 126).done(),
        b("read_input_limit", "FC4 for the 125 register maximum.").read(4, 0, 125).done(),
        b("read_input_over_limit", "FC4 for 126 registers, one over the maximum.").read(4, 0, 126).done(),
        b("read_zero_count", "FC3 and FC4 with quantity 0.").read(3, 0, 0).read(4, 0, 0).done(),
        b("read_address_edge", "Reads ending exactly at, and one past, register 65535.")
            .read(3, 65411, 125)
            .read(3, 65412, 125)
            .read(3, 65535, 1)
            .read(3, 65535, 2)
            .read(4, 65411, 125)
            .read(4, 65412, 125)
            .done(),
        b("read_first_register_each_bank", "Single-register reads at address 0 in both banks.")
            .read(3, 0, 1)
            .read(4, 0, 1)
            .done(),
        b("read_holding_empty_pdu", "FC3 with no address or quantity.").pdu(3, &[]).done(),
        b("read_holding_short_pdu", "FC3 with address but no quantity.").pdu(3, &[0, 0]).done(),
        b("read_holding_three_byte_pdu", "FC3 with a quantity cut to one byte.").pdu(3, &[0, 0, 0]).done(),
        b("read_holding_long_pdu", "FC3 with extra bytes after the quantity.")
            .pdu(3, &[0, 0, 0, 2, 0xDE, 0xAD])
            .done(),
        b("read_input_empty_pdu", "FC4 with no address or quantity.").pdu(4, &[]).done(),
        b("read_input_short_pdu", "FC4 with address but no quantity.").pdu(4, &[0, 0]).done(),
        b("read_input_long_pdu", "FC4 with extra bytes after the quantity.").pdu(4, &[0, 0, 0, 2, 0xDE, 0xAD]).done(),
    ]
}
