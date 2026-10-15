//! FC23: write-then-read ordering, limits, boundaries, malformed PDUs.

use super::{Builder, Scenario};

pub(super) fn scenarios(unit: u8) -> Vec<Scenario> {
    let b = |name, about| Builder::new(name, about, unit);
    vec![
        b("read_write_multiple", "FC23 writing two registers at 1 and reading elsewhere, then FC3 read back.")
            .pdu(0x17, &[0, 0, 0, 4, 1, 0x2C, 0, 2, 4, 0xAA, 0xAA, 0xBB, 0xBB])
            .read(3, 300, 2)
            .done(),
        b("read_write_multiple_ordering", "FC23 whose read range overlaps its write range: the write is applied first.")
            .pdu(0x17, &[0x01, 0xF3, 0, 4, 0x01, 0xF4, 0, 2, 4, 0x11, 0x11, 0x22, 0x22])
            .done(),
        b("read_write_multiple_limits", "FC23 at 125 read and 121 written, 126 read, and 122 written.")
            .pdu(0x17, &[&[0, 0, 0, 125, 0, 0, 0, 121, 242][..], &[0x55; 242]].concat())
            .pdu(0x17, &[0, 0, 0, 126, 0x0F, 0xF5, 0, 1, 2, 0, 1])
            .pdu(0x17, &[&[0, 0, 0, 1, 0x10, 0x04, 0, 122, 244][..], &[0x66; 244]].concat())
            .done(),
        b("read_write_zero_read_count", "FC23 with read quantity 0.")
            .pdu(0x17, &[0, 0, 0, 0, 0x0F, 0xB0, 0, 1, 2, 0, 1])
            .done(),
        b("read_write_zero_write_count", "FC23 with write quantity 0.")
            .pdu(0x17, &[0, 0, 0, 1, 0x0F, 0xB2, 0, 0, 0])
            .done(),
        b("read_write_read_address_edge", "FC23 reading past register 65535, then ending exactly at it.")
            .pdu(0x17, &[0xFF, 0xFF, 0, 2, 0, 0, 0, 1, 2, 0, 1])
            .pdu(0x17, &[0xFF, 0xFF, 0, 1, 0, 0, 0, 1, 2, 0, 1])
            .done(),
        b("read_write_write_address_edge", "FC23 writing past register 65535, then ending exactly at it.")
            .pollutes()
            .pdu(0x17, &[0, 0, 0, 1, 0xFF, 0xFF, 0, 2, 4, 0, 1, 0, 2])
            .pdu(0x17, &[0, 0, 0, 1, 0xFF, 0xFF, 0, 1, 2, 0x12, 0x34])
            .done(),
        b("read_write_byte_count_mismatch", "FC23 with byte count 3 for two registers.")
            .pdu(0x17, &[0, 0, 0, 1, 0x0F, 0xB4, 0, 2, 3, 0, 1, 0, 2])
            .done(),
        b("read_write_truncated_data", "FC23 declaring four data bytes and carrying two.")
            .pdu(0x17, &[0, 0, 0, 1, 0x0F, 0xB6, 0, 2, 4, 0, 1])
            .done(),
        b("read_write_short_pdu", "FC23 cut after the write address.")
            .pollutes().pdu(0x17, &[0, 0, 0, 1, 0, 0]).done(),
        b("read_write_empty_pdu", "FC23 with no data.")
            .pollutes().pdu(0x17, &[]).done(),
    ]
}
