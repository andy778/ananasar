# Differences from the original

Ananasar aims to behave like the original Ananas on the wire. Where the original accepts something the Modbus specification rejects, Ananasar follows the specification. This page lists each such place, and the one place where Ananasar is stricter than the specification to match the original. The parity goldens mark each of them with a `#! deviation:` line (see [Parity testing](parity-testing.md)); the discussion is in [#94](https://github.com/andy778/ananasar/issues/94).

References are to two public documents from [modbus.org](https://modbus.org/modbus-specifications):

- **Application Protocol**: Modbus Application Protocol Specification V1.1b3.
- **TCP/IP Guide**: Modbus Messaging on TCP/IP Implementation Guide V1.0b.

## Where Ananasar follows the specification

| Topic | Original | Ananasar | Specification |
| --- | --- | --- | --- |
| Quantity limits | Answers a read of 126 registers and accepts a write of 124 registers. | Exception 03 (illegal data value) outside the limits: FC3 and FC4 read 1 to 125, FC16 write 1 to 123, FC23 read 1 to 125 and write 1 to 121. | Application Protocol, sections 6.3, 6.4, 6.12 (FC16) and 6.17 (FC23): each gives the permitted quantity and the state diagram answers 03 outside it. |
| Zero quantity | Accepts quantity 0 and answers as if the request were valid. | Exception 03. | The same sections: the permitted range starts at 1. |
| PDU length and byte count | Does not check them. A write with a wrong byte count, a truncated PDU or an empty PDU is carried out, with missing fields filled from earlier traffic. | Exception 03. | Application Protocol, sections 6.12 and 6.17: the state diagrams require the byte count to equal the quantity times 2 and answer 03 otherwise. The specification does not describe truncated PDUs separately; answering 03 is the reading that fits the checks it does state. |
| Exception function code of a request above 0x7F | Flips the top bit: 0x80 is answered as 0x00, 0xFF as 0x7F, 0x00 as 0x80. | Sets the bit, so the answer is the request function code plus 0x80 (modulo 256). | Application Protocol, section 4.5 and section 7: the exception function code is the request function code plus 0x80, with the most significant bit set. Section 4.1 reserves 128 to 255 for exception codes, so a request in that range has no defined answer; Ananasar applies the rule unchanged. |
| FC23 write range past register 65535 | The write spills into the input registers. | Exception 02 (illegal data address). | Application Protocol, section 6.17: the starting address and the starting address plus the quantity must both be valid, for the read range and for the write range, else 02. |
| Order of the checks | Checks the address range first and has no quantity limit. A read of 126 registers that also runs past the end gets 02. | Function, then quantity, then address. The same request gets 03. | Application Protocol, sections 6.3, 6.4, 6.12 and 6.17: the state diagrams test whether the function is supported (01), then the quantity (and byte count) (03), then the addresses (02), then the processing (04). |

Requests that are valid under the specification behave the same as in the original.

## Where Ananasar is stricter than the specification

| Topic | Original | Ananasar | Specification |
| --- | --- | --- | --- |
| Non-zero protocol id or length high byte | Closes the connection. | Closes the connection. | TCP/IP Guide, sections 3.1.3 and 4.4.2.2: the protocol identifier is 0 for Modbus, and an indication with another value is "simply discarded". |

The specification discards such a frame and leaves the connection open. Ananasar closes the connection, as the original does, for two reasons. A frame with a wrong protocol id or an oversized length cannot be framed reliably, so the bytes that follow cannot be trusted to start at a frame boundary. And a client that sends such a frame is not speaking Modbus/TCP, so ending the session is a safe response. The guide does not cover the length high byte; it is treated like the protocol id. See [#93](https://github.com/andy778/ananasar/issues/93).

## Not copied from the original

When the MBAP length field disagrees with the bytes sent, the original answers from whatever bytes arrived, which can include bytes left over from earlier requests. Ananasar frames by the length field and does not copy this. The guide (section 3.1.3) defines the length field as the number of bytes that follow it.

## See also

- [Parity testing](parity-testing.md) for how the differences are recorded in the goldens
- [Troubleshooting](troubleshooting.md) for the exceptions a client may see
