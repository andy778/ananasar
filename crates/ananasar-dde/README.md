# ananasar-dde

The DDE server behind Options > DDE Server. It exposes the shared register
file to DDE clients (Excel, LibreOffice Calc, ...) through the Windows DDEML
API (`user32`).

- `item`: item/topic parsing, poke parsing and value formatting. Pure, safe,
  tested on every platform.
- `server` (Windows only): the DDEML server on its own message-pump thread.

## Why this crate allows `unsafe`

The workspace forbids `unsafe_code`. DDEML is a C API with raw handles and a
callback, so some FFI is unavoidable. To keep the risk small:

- the FFI lives in this one crate, in one module (`server`), using
  `windows-sys` for the declarations (already in the dependency tree through
  the GUI) instead of hand-written `extern` blocks;
- every other module and crate keeps `unsafe_code` denied or forbidden;
- the module only calls the DDEML functions it needs and never keeps a raw
  pointer beyond the call that received it.

## Naming

Service `Ananasar`; topics `input` and `holding`; items `r<N>` with N in
0..=65535, for example `=Ananasar|holding!r100` in a spreadsheet.

## See also

- [DDE manual](../../docs/dde.md) for how to use the server from Excel and other DDE clients
