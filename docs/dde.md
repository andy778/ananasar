# DDE server manual

On Windows, Ananasar can serve its registers to other programs through DDE, so that a spreadsheet cell can show a register and follow it as it changes.

Dynamic Data Exchange (DDE) is an old Windows mechanism for programs to share values by name. A DDE client connects to a **service**, picks a **topic** and asks for an **item**; it can read the item once, write ("poke") it, or ask to be told whenever it changes (a **hot link**). Excel and LibreOffice Calc are DDE clients. The DDE server is part of `ananasar-gui` and exists on Windows only.

## Names

| Part | Name | Meaning |
| --- | --- | --- |
| Service | `Ananasar` | Not case-sensitive. |
| Topic | `input` or `holding` | The register bank. Not case-sensitive. |
| Item | `r<N>`, N from 0 to 65535 | Register N in decimal, with a lower-case `r`. `r100` is valid; `R100`, `r0x10` and `r 100` are not. |

In Excel the three parts form a cell reference, `=Service|Topic!Item`:

```text
=Ananasar|holding!r100
=Ananasar|input!r8862
```

Values are exchanged as text. A read returns the register as a decimal number.

## Enabling it

1. Start `ananasar-gui` and choose Options > DDE Server….
2. In **Server activation** choose which registers are offered:
   - All Input and Holding registers
   - Input registers
   - Range of Input registers
   - Holding registers
   - Range of Holding registers (the choice every time the dialog opens)
   - Range of Input and Holding registers (the same range in both banks)
3. For the range choices enter **From** and **To**: whole numbers from 0 to 65535, with From not larger than To. Otherwise the dialog shows "Range error (0 - 65535) - check values".
4. Press **OK**. The dialog closes and the service is registered. Cancel closes the dialog without changing anything.

The **i** button in the dialog explains the names. Choosing OK again with another selection replaces the running server; the service name can be registered only once. There is no stop button: the server runs until you quit Ananasar. Items outside the activated range, and topics other than `input` and `holding`, are refused.

## What clients can do

| Operation | DDE name | Behaviour |
| --- | --- | --- |
| Read | Request | Returns the register's current value as decimal text. |
| Write | Poke | Sets the register. The text may be decimal, or hexadecimal with a `$` or `0x` prefix. Values above 65535 wrap to 16 bits (70000 becomes 4464). Text that is not a non-negative whole number is acknowledged but changes nothing. |
| Hot link | Advise | The server compares the served registers about every 100 ms and notifies linked clients of the ones that changed, at most 512 per round; the rest follow in the next. |

Pokes work on both banks, including input registers, which are read-only for Modbus clients. Only the plain text clipboard format is served. DDE "execute" commands are refused.

The registers are the same ones the Modbus server answers from: a value changed by a Modbus client appears in a linked cell, and a poke is visible to Modbus clients on the next read.

## Examples

**Excel, read and follow a register.** Type the formula in a cell:

```text
=Ananasar|holding!r100
```

Depending on the Excel version and settings, links to other applications may need to be enabled (a security prompt about updating links). The cell then follows holding register 100.

**Excel, write a register.** Cell formulas only read. A macro can poke a value; this VBA sketch writes 42 to holding register 100:

```vb
Sub PokeRegister()
    Dim ch As Variant
    ch = Application.DDEInitiate("Ananasar", "holding")
    Application.DDEPoke ch, "r100", "42"
    Application.DDETerminate ch
End Sub
```

**LibreOffice Calc.** Calc's `DDE` function takes the service, topic and item as separate arguments:

```text
=DDE("Ananasar";"holding";"r100")
```

The Excel and Calc examples follow those programs' documented syntax; they have not been run as part of writing this page, so check them against your version.

## Limitations

- Windows only. On other platforms the menu entry is greyed out.
- One DDE server per running Ananasar.
- Registers only: no coils, no strings, no other clipboard formats.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| The client says it cannot connect | The dialog must have been closed with OK (the server is not started by opening it). Check the spelling of the service `Ananasar` and the topic `input` or `holding`. |
| A cell shows an error for one register but not others | The register is outside the activated range, or the item name is wrong (lower-case `r`, decimal number). |
| "Cannot start the DDE server" message | A DDE server of this process is already running, DDE could not initialise, or the service name could not be registered. Restart Ananasar and try once. |
| The cell does not update | Make sure the link is a hot link (the usual behaviour of `=Service|Topic!Item` in Excel) and that Excel's setting for updating links to other applications is on. |
| A poke has no effect | The text was not a whole number; send digits, or `$`/`0x` hex. |

## See also

- [GUI manual](gui.md) for the rest of the program
- [Troubleshooting](troubleshooting.md) for general problems
- [`crates/ananasar-dde`](../crates/ananasar-dde/README.md) for the design of the DDE crate
