//! FC6 and FC16 writes, boundaries, malformed write PDUs, FC22 mask writes.

use super::{Builder, Scenario, ramp};

pub(super) fn scenarios(unit: u8) -> Vec<Scenario> {
    let b = |name, about| Builder::new(name, about, unit);
    vec![
        b("write_single", "FC6 writes, echo reply, and read back (including writing 0).")
            .pdu(6, &[0, 5, 0x12, 0x34])
            .read(3, 5, 1)
            .pdu(6, &[0, 6, 0, 0])
            .read(3, 6, 1)
            .done(),
        b("write_single_extremes", "FC6 with value 0xFFFF and 0x8000.")
            .pdu(6, &[0, 20, 0xFF, 0xFF])
            .pdu(6, &[0, 21, 0x80, 0x00])
            .read(3, 20, 2)
            .done(),
        b("write_single_short_pdu", "FC6 with address only.")
            .pollutes().pdu(6, &[0, 5]).done(),
        b("write_single_empty_pdu", "FC6 with no data.")
            .pollutes().pdu(6, &[]).done(),
        b("write_single_three_byte_pdu", "FC6 with a value cut to one byte.")
            .pollutes().pdu(6, &[0, 5, 0x12]).done(),
        b("write_single_long_pdu", "FC6 with extra bytes after the value.")
            .pdu(6, &[0, 22, 0x12, 0x34, 0xBE, 0xEF])
            .read(3, 22, 1)
            .done(),
        b("write_single_address_edge", "FC6 at register 65535.")
            .pdu(6, &[0xFF, 0xFF, 0xAB, 0xCD])
            .read(3, 65535, 1)
            .done(),
        b("write_multiple", "FC16 of three registers and read back.")
            .write_multiple(10, &[0x0A0B, 0x0C0D, 0x0E0F])
            .read(3, 10, 3)
            .done(),
        b("write_multiple_one_register", "FC16 with a single register.")
            .write_multiple(30, &[0x7777])
            .read(3, 30, 1)
            .done(),
        b("write_multiple_limit", "FC16 of 123 registers, the maximum.")
            .write_multiple(0, &ramp(123))
            .read(3, 0, 123)
            .done(),
        b("write_multiple_over_limit", "FC16 of 124 registers, one over the maximum.")
            .write_multiple(4200, &ramp(124))
            .done(),
        b("write_multiple_zero_count", "FC16 with quantity 0.").pdu(0x10, &[0x0F, 0xA0, 0, 0, 0]).done(),
        b("write_multiple_byte_count_mismatch", "FC16 with byte count 3 for two registers.")
            .pdu(0x10, &[0x0F, 0xA2, 0, 2, 3, 0, 1, 0, 2])
            .done(),
        b("write_multiple_odd_byte_count", "FC16 with quantity 1 and byte count 1.")
            .pdu(0x10, &[0x0F, 0xA4, 0, 1, 1, 0])
            .done(),
        b("write_multiple_byte_count_zero", "FC16 with quantity 1 and byte count 0.")
            .pdu(0x10, &[0x0F, 0xA5, 0, 1, 0])
            .done(),
        b("write_multiple_truncated_data", "FC16 declaring four data bytes and carrying two.")
            .pdu(0x10, &[0x0F, 0xA6, 0, 2, 4, 0, 1])
            .done(),
        b("write_multiple_extra_data", "FC16 with more data bytes than the byte count says.")
            .pdu(0x10, &[0, 40, 0, 1, 2, 0x11, 0x11, 0x22, 0x22])
            .read(3, 40, 2)
            .done(),
        b("write_multiple_short_pdu", "FC16 cut after the quantity.")
            .pollutes().pdu(0x10, &[0x0F, 0xA8, 0, 1]).done(),
        b("write_multiple_empty_pdu", "FC16 with no data.")
            .pollutes().pdu(0x10, &[]).done(),
        b("write_multiple_address_edge", "FC16 ending at, and one past, register 65535.")
            .pollutes()
            .write_multiple(65535, &[1])
            .write_multiple(65535, &[1, 2])
            .write_multiple(65534, &[1, 2])
            .done(),
        b("write_multiple_last_fit", "FC16 of 123 registers ending exactly at register 65535.")
            .write_multiple(65413, &ramp(123))
            .read(3, 65413, 3)
            .done(),
        b("write_multiple_one_past_last_fit", "FC16 of 123 registers running one past register 65535.")
            .pollutes()
            .write_multiple(65414, &ramp(123))
            .done(),
        // Modbus spec example: register 0x0012, masks 0x00F2 / 0x0025, result 0x0017.
        b("mask_write", "FC22 spec example: 0x0012 with AND 0x00F2 and OR 0x0025 gives 0x0017.")
            .pdu(6, &[0, 200, 0, 0x12])
            .pdu(0x16, &[0, 200, 0, 0xF2, 0, 0x25])
            .read(3, 200, 1)
            .done(),
        b("mask_write_and_zero", "FC22 with AND mask 0: result is the OR mask.")
            .pdu(6, &[0, 201, 0x12, 0x34])
            .pdu(0x16, &[0, 201, 0, 0, 0xA5, 0x5A])
            .read(3, 201, 1)
            .done(),
        b("mask_write_or_zero", "FC22 with OR mask 0: result is current AND mask.")
            .pdu(6, &[0, 202, 0x12, 0x34])
            .pdu(0x16, &[0, 202, 0xF0, 0xF0, 0, 0])
            .read(3, 202, 1)
            .done(),
        b("mask_write_and_ffff", "FC22 with AND mask 0xFFFF: the register keeps its value.")
            .pdu(6, &[0, 203, 0x12, 0x34])
            .pdu(0x16, &[0, 203, 0xFF, 0xFF, 0xAA, 0xAA])
            .read(3, 203, 1)
            .done(),
        b("mask_write_overlapping_masks", "FC22 where the OR mask has bits the AND mask keeps.")
            .pdu(6, &[0, 204, 0x00, 0x00])
            .pdu(0x16, &[0, 204, 0x0F, 0x0F, 0xFF, 0xFF])
            .read(3, 204, 1)
            .done(),
        b("mask_write_address_edge", "FC22 at register 65535.")
            .pdu(0x16, &[0xFF, 0xFF, 0, 0xF2, 0, 0x25])
            .read(3, 65535, 1)
            .done(),
        b("mask_write_short_pdu", "FC22 with only one mask.")
            .pollutes().pdu(0x16, &[0, 200, 0, 0xF2]).done(),
        b("mask_write_empty_pdu", "FC22 with no data.")
            .pollutes().pdu(0x16, &[]).done(),
        b("mask_write_long_pdu", "FC22 with extra bytes after the masks.")
            .pdu(0x16, &[0, 205, 0, 0xF2, 0, 0x25, 0xDE, 0xAD])
            .read(3, 205, 1)
            .done(),
    ]
}
