# GUI manual

A tour of `ananasar-gui`, window by window: the main window with its embedded server, the client window, and the tool windows opened from the menus.

In shortcuts, **Ctrl** is **Cmd** on macOS. Menu entries are written as File > Open…, with the same captions as in the program.

## Starting the GUI

```bash
ananasar-gui              # default port, see below
ananasar-gui --port 15020 # or -p 15020: use exactly this port
```

The GUI starts a Modbus/TCP server straight away. The default port is **502 on Windows** and **15020 on other platforms**. Without `--port`, a busy default port is not an error: the GUI tries the next free port (up to 20 ports in a row) and shows a notice, "Port changed to N. Default port D is currently used by other instance of Ananas." With `--port`, the port is used exactly; if it cannot be bound, a message box says so and the server stays off. You can change the port later with Options > TCP > Port > Change Port….

The window title shows the machine's outgoing IP address (or the port, if none can be found).

## Main window

| Area | What it shows |
| --- | --- |
| Registers (left) | A list of 26 registers with the columns Register, Value and R/W. |
| Type Shown | Input or Holding: which bank the list displays. |
| Start Address | First address in the list. Pick or type it. |
| UID | The unit id the server answers to; 0 answers any unit id (the default). |
| Client | Show / Hide button for the client window, and Connected / Disconnected. |
| Logging | Start / Stop button for the data log, and the Interpretive check box. |
| Server | Live status of the server, one column per connection. |
| Status bar | Date and time. Hover for the server state, port and number of clients. |

### Register list

- The mouse wheel over the list scrolls by ten registers.
- Click a value to select it. The R/W column always reads `-/-`.
- **Monitor** mode (the default) is read-only. Switch to **Edit** with Edit > Value List > Edit (Ctrl+E), then double-click a **holding** register value to type a new one and press Enter. Input registers cannot be edited here; use [Set Register Value](#set-register-value).
- Registers as Hex (View, Ctrl+Shift+H) shows addresses in hex, and Values as Hex (View, Ctrl+H) shows values in hex. Typing a value while Values as Hex is on expects hex.
- Edit > Value List > Copy (Ctrl+C) puts the 26 visible rows on the clipboard as `address<TAB>value` lines. The Value List shortcuts are ignored while a text field has the keyboard focus.

### Server panel

The panel has a column for each of the two possible connections. A dash means no client in that slot.

| Row | Meaning |
| --- | --- |
| Connection IP | Address of the connected client. |
| Nagle | Whether Nagle's algorithm is Enabled or Disabled on that connection. |
| Poll interval | Time between the client's last two requests, in ms. |
| Data packet size | Size of the last request in bytes. |
| Header check | Passed or Failed for the last request; hover a failure for the reason (with tooltips on). |
| Transaction ID, Protocol ID, Message length, Unit ID, Function code, Start address, Word count | The fields of the last request, in hex with the decimal value. |
| Data received / Data sent | Byte totals for the connection. |
| Local port | The port the server listens on. |
| Logged packets | Entries written to the current data log. |
| Address offset, Incorrect TI | On when the matching Options > Modbus entry is checked. |
| Out of Synch, Incorrect length, Incorrect PID | On when the matching Options > Modbus entry is checked, or when the last request had that problem (a length the stream could not follow, a length field that disagreed with the bytes received, a non-zero protocol id). |
| Return exception | On when a forced exception is active or the last reply was an exception. |

## Menus

### File menu

| Entry | Shortcut | Action |
| --- | --- | --- |
| Open… | Ctrl+O | Load register values from a `*.ananas` file. All registers are cleared first, then the file's values are set, and the Start Address jumps to the first address of the saved list for the shown bank. |
| Save As… | Ctrl+S | Save the register values and the visible address list to a `*.ananas` text file (default name `registers.ananas`). Only non-zero registers are stored. |
| Exit | Alt+F4 | Close the program. |
| About… | | Version and credits. The Licences button opens the third-party notices: the dependencies, the advisories known when the build was made and the licence texts (release builds only; other builds show a placeholder). |

Errors from loading or saving appear in a message box.

### Edit menu

| Entry | Shortcut | Action |
| --- | --- | --- |
| Value List > Edit | Ctrl+E | Allow double-click editing of holding registers. |
| Value List > Monitor | Ctrl+M | Read-only list (the default). |
| Value List > Copy | Ctrl+C | Copy the visible rows. |
| Set Register Value | Ctrl+R | Open [Set Register Value](#set-register-value). |
| Initialize > Input Registers / Holding Registers > All to '0' | | Set every register of that bank (all 65 536) to 0. |
| Initialize > Input Registers / Holding Registers > Ascending | | Set register N to the value N. |

### View menu

| Entry | Shortcut | Action |
| --- | --- | --- |
| Registers as Hex | Ctrl+Shift+H | Show addresses in hex. |
| Values as Hex | Ctrl+H | Show values in hex. |

### Options menu

| Entry | Shortcut | Action |
| --- | --- | --- |
| Modbus > … | | See [Modbus options](#modbus-options). |
| TCP > … | | See [TCP options](#tcp-options). |
| DDE Server… | | Windows only; see [DDE Server](#dde-server). Greyed out elsewhere. |
| Show Tooltips | Ctrl+T | Turn the hover hints on or off (on by default). |

## Set Register Value

Edit > Set Register Value (Ctrl+R) edits one register in the server's register file while the server keeps serving.

- **Register**: choose Input or Holding and type the address.
- **Current value**: the register's value now. Double-click it to copy it into Edit value.
- **Edit value**: type a number in decimal, or hex with a `0x` or `$` prefix, then press **Set**. Set is disabled for input registers, which are read-only over Modbus (they can still be changed with Initialize and by loading a file).
- **Bitmask**: 16 check boxes, bit 15 (MSB) on the left to bit 0 (LSB) on the right. Toggling a box changes the pending value; press Set to write it.
- **32-bit Representation**: reads the register and the next one as a 32-bit unsigned integer ("Uint") and as an IEEE 754 float ("Float"). The caption shows the current byte order.

The window's own Edit menu has **32-bit Representation Byte Order**. The digits give which of the four bytes goes where, from the most significant byte of the 32-bit value down; the bytes are numbered in wire order, so byte 0 is the high byte of the first register.

| Order | Meaning |
| --- | --- |
| 0-1-2-3 (default) | Big-endian: the first register is the high word. |
| 1-0-3-2 | Bytes swapped inside each register. |
| 2-3-0-1 | The two registers swapped (low word first). |
| 3-2-1-0 | Fully reversed (little-endian). |

Example: registers `0x3FC0`, `0x0000` read as the float 1.5 in order 0-1-2-3; the same value stored low word first (`0x0000`, `0x3FC0`) reads as 1.5 in order 2-3-0-1.

## Register Scanner

Options > Modbus > Register Scanner… (Ctrl+Alt+C), or Modbus > Register Scanner… in the client window. It sweeps an address range with one single-register read per address, over its own connection, and lists what each address answers. The command-line version is [`ananasar scan`](cli.md#ananasar-scan).

1. Press **Connect…**, enter the server's IP and port (it starts with the embedded server's address) and press Connect.
2. Choose Input or Holding and the **UID** (default 1).
3. Set **From** and **To** (both start at 1) and **Timeout [ms]** (default 100).
4. Press **Start**. **Stop** interrupts the sweep. A progress bar and a status line (registers scanned, succeeded, failed, elapsed time) follow it.

| Option | Effect |
| --- | --- |
| Use address offset | The From and To numbers are one-based; one is subtracted on the wire. |
| List failed polls only | Only exceptions and missing replies go into the Results list. |
| Copy value to register | A successful read is also written into the same register of the program's own register file. |

Results lines show the address and either the value (decimal and hex), `exception N` or `no reply`. Right-click the list for Copy to Clipboard and Clear; double-click it to toggle a horizontal scroll bar. If a request gets no usable reply the scan stops and the connection is closed, because the stream can no longer be matched to requests. The window's menu has **Close!**.

## Client window

Press **Show** in the Client box of the main window. The client window is a separate window with its own connection, poll lists and log. Its UID box defaults to 1.

### Connecting

**Connect…** opens the **Server connection** dialog:

- **IP**: four fields, one per octet. Only dotted IPv4 addresses can be entered here (no host names).
- **Port**: 1-65535. It starts with the embedded server's port.
- **Send Keepalives to server**: enable TCP keepalives.

**OK** connects. While connected the dialog is locked; press **Disconnect** first. The *Connection partner* line shows the server's IP and port. Connection events and errors appear in the log at the bottom; double-click it to clear it and right-click for Copy.

### Poll lists

The client keeps three sorted address lists:

- **Read Input/Holding register**: choose Input registers or Holding registers, pick an address from the drop-down (or type it) and press **Add** or **Remove**.
- **Write Holding register**: the same for registers to write with FC16. The values written are those in the client's own copy of the holding registers, which is filled by holding reads; unread registers are 0.

Edit > Add Serie > Holding Registers > Write / Read, or Input Registers, opens the **Series** dialog: enter **From** and **To** (From must not exceed To) and press OK to add the range to that list, or tick **Remove** to take it out. Edit > Clear empties a list.

On each poll the client takes the lists in turn (holding read, input read, write) and sends one query for the longest run of consecutive addresses, capped by the transaction limit. Edit > Transaction limiter sets the registers per transaction: **Registers to Read** 5, 50, 100 or 125 (default 125) and **Registers to Write** 50, 100 or 123 (default 100).

### Polling

| Mode | Behaviour |
| --- | --- |
| Cyclic read/write | One query every **Cycle** interval (10-60 000 ms, default 1000). |
| Immediate query | The next query as soon as the previous reply has arrived. |
| Wait after response | The next query one **Delay** after the reply (same interval setting). |
| Pause | No polling (the default). |

Two check boxes change the requests: **Use address offset** subtracts one from every address on the wire (for devices documented with one-based addresses), and **TI always '00 00'** sends transaction id 0 in every request. Replies are logged with their transaction id; exceptions show the function and code.

### Manual commands

The client window's **Modbus** menu has one-off commands, each also available with a shortcut:

| Entry | Shortcut | Action |
| --- | --- | --- |
| Write Single Register… | Ctrl+Alt+S | Opens the Write dialog. |
| Mask Write Register… | Ctrl+Alt+M | Opens the dialog in mask mode. |
| Read Exception Status… | Ctrl+Alt+R | Sends FC7 (enabled while connected). |
| Register Scanner… | Ctrl+Alt+C | Opens the [Register Scanner](#register-scanner). |

The **Write single register** dialog has Address, Value (decimal, or hex with `0x`) and the check box **Use Function 16**, which sends FC16 with one register instead of FC6. Ticking **Mask AND + OR** switches to the **Mask single register** form (FC22): the Value box becomes the AND pattern and the **OR (Hex)** box becomes active; both are hex. The result is `(current AND and_mask) OR (or_mask AND NOT and_mask)`. **Apply** sends the request (enabled while connected and the fields are valid) and **Close** closes the dialog.

Edit > Hide Window closes the client window.

## Modbus options

All of these affect the embedded server and apply immediately.

| Entry | Effect |
| --- | --- |
| Active MB Functions > FC3, FC4, FC6, FC7, FC8, FC16, FC22, FC23 | Check marks for the function codes the server answers. All are on by default. A request for an unchecked function gets exception 1 (Illegal Function). |
| Return Exception > … | Answer requests that pass the unit id filter with the chosen exception instead of processing them. A disabled or unknown function still gets Illegal Function (1), and a request whose range runs past register 65535 still gets Illegal Data Address (2). Choose the active entry again to turn it off. The entries are Illegal Function (1), Illegal Data Address (2), Illegal Data Value (3), Slave Device Failure (4), Acknowledge (5), Slave Device Busy (6), Negative Acknowledge (7), Memory Parity Error (8), Gateway Path Unavailable (10) and Gateway Target Device Failed to Respond (11). |
| Return Incorrect TI | Every reply, exceptions included, gets transaction id `DE AD`. |
| Return Incorrect Protocol ID | Normal replies get protocol id `AB CD`; exception replies are left alone. |
| Return Incorrect Length High Byte | Normal replies get `EF` as the high byte of the length field; exception replies are left alone. |
| Use Address Offset (Ctrl+Alt+O) | Wire address n reaches register n + 1, so a request at address 65535, or a span that ends there, gets Illegal Data Address (2). |
| Reply Out of Synch (Ctrl+Alt+S) | Each connection answers one request late: the first request gets no reply, and the reply to request n is sent when request n + 1 arrives. |
| Register Scanner… | See [Register Scanner](#register-scanner). |

FC7 (Read Exception Status) replies with the low byte of holding register 65535, and FC8 (Diagnostics) answers sub-functions 0 (the first two data bytes echoed), 2 (the value of holding register 65534), 0x0A (clear the counters), 0x0D (exception replies sent), 0x0E (requests received) and 0x0F (always 0); every other sub-function gets Illegal Function (1). Requests with a unit id the server does not accept get no reply at all. A request with a non-zero protocol id or length high byte closes the connection; if the function is switched off or an exception is forced, the exception reply (with that protocol id) is sent first.

## TCP options

| Entry | Shortcut | Effect |
| --- | --- | --- |
| Port > Monitor Port N | Ctrl+P | Checked while the server listens. Choose it to stop listening, and again to start. The Logging button is only enabled while the server listens (or a log is running). |
| Port > Change Port… | Ctrl+N | Opens the **Server port** dialog (1-65535). The new port is bound first; if that fails, the old server keeps running and the dialog shows the error. |

Limits that are fixed: at most two clients at a time (further connections are dropped), and a connection that sends nothing for a minute is closed. The "Release Connection if Unused >1 min" entry is shown checked and greyed out to say so.

## DDE Server

Options > DDE Server… (Windows only) opens a dialog to serve registers to DDE clients such as Excel. The full description is in the [DDE manual](dde.md).

## Data log

The Logging box writes every request and reply of the server, per connection, to a file. Press **Start** (it becomes **Stop**); the file `ananasar-<port>-<UTC time>.log` is created in the program's working directory and its path is shown in the status bar tooltip. The **Interpretive** box (on by default, locked while logging) adds a decoded description under each hex entry. The Server panel's *Logged packets* row counts entries. Each entry looks like this:

```text
#1 1250 ms Connection: 1
Query:
00 00 00 00 00 06 01 03 00 0A 00 02
```

followed by the reply (and, in interpretive mode, the descriptions). The same log is available from the CLI with `--log-dir`.

## Not implemented

These entries exist in the menus, to match the original's layout, but are greyed out because the server does not support them yet:

- Options > TCP: Port > Purge Port Usage Blocking(s) and Disable Nagle > Connection #1 / #2.
- Options > DDE Server… on platforms other than Windows.

## See also

- [Getting started](getting-started.md) for a first session
- [CLI manual](cli.md) for the same engine without a window
- [DDE manual](dde.md) for Excel integration
- [Troubleshooting](troubleshooting.md) if something does not answer
