//! Server data log formatting ("raw" and "interpretive" logs).
//!
//! This module is pure: it turns one recorded exchange (query bytes plus the
//! reply or a detected error) into the text of a log entry. Opening files and
//! timing live in the networking crate.
//!
//! An entry looks like this (raw mode):
//!
//! ```text
//! #1 0 ms Connection: 1
//! Query:
//! 00 07 00 00 00 06 01 03 00 0A 00 02
//! Reply:
//! 00 07 00 00 00 07 01 03 04 00 05 00 09
//! ```
//!
//! Interpretive mode adds a decoded description after the query and a decoded
//! table after the reply.

use crate::protocol::frame;
use std::fmt::Write as _;

/// How much detail a log entry carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogMode {
    /// Hex bytes only.
    Raw,
    /// Hex bytes plus a decoded description of query and reply.
    Interpretive,
}

/// What the server did with a query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response<'a> {
    /// The complete reply frame that was sent.
    Reply(&'a [u8]),
    /// No reply was sent; the text says why (dropped or malformed frame).
    Error(&'a str),
}

/// One recorded exchange on a server connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    /// Running recording number, starting at 1.
    pub number: u64,
    /// Milliseconds since logging started.
    pub elapsed_ms: u64,
    /// Connection slot, 1 or 2.
    pub connection: u8,
    /// The query frame as received (empty if nothing usable was read).
    pub query: &'a [u8],
    /// The reply or the error.
    pub response: Response<'a>,
}

/// Format `bytes` as uppercase hex separated by single spaces.
pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        let _ = write!(s, "{b:02X}");
    }
    s
}

/// Render one log entry, ending with a blank separator line.
pub fn format_entry(entry: &Entry<'_>, mode: LogMode) -> String {
    let interpret = mode == LogMode::Interpretive;
    let mut out = format!("#{} {} ms Connection: {}\nQuery:\n", entry.number, entry.elapsed_ms, entry.connection);
    if entry.query.is_empty() {
        out.push_str("(no data)\n");
    } else {
        let _ = writeln!(out, "{}", hex(entry.query));
    }
    if interpret && !entry.query.is_empty() {
        out.push('\n');
        out.push_str(&describe_query(entry.query));
        out.push('\n');
    }
    match entry.response {
        Response::Reply(reply) => {
            let _ = writeln!(out, "Reply:\n{}", hex(reply));
            if interpret {
                out.push('\n');
                out.push_str(&describe_reply(entry.query, reply));
            }
        }
        Response::Error(text) => {
            let _ = writeln!(out, "Error: {text}");
        }
    }
    out.push('\n');
    out
}

fn be16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*d.get(at)?, *d.get(at + 1)?]))
}

/// Longest register list printed in full; longer ones are cut with `...`.
const LIST_LIMIT: u16 = 125;

fn register_list(start: u16, count: u16) -> String {
    let shown = count.min(LIST_LIMIT);
    let mut s = String::new();
    for i in 0..shown {
        if i > 0 {
            s.push(' ');
        }
        let _ = write!(s, "{}", u32::from(start) + u32::from(i));
    }
    if count > shown {
        s.push_str(" ...");
    }
    s
}

fn function_name(code: u8) -> Option<&'static str> {
    Some(match code {
        0x03 => "Read holding registers",
        0x04 => "Read input registers",
        0x06 => "Write single register",
        0x07 => "Read exception status",
        0x08 => "Diagnostics",
        0x10 => "Write holding registers",
        0x16 => "Mask write register",
        0x17 => "Read/write (holding) registers",
        _ => return None,
    })
}

fn exception_name(code: u8) -> &'static str {
    match code {
        1 => "Illegal function",
        2 => "Illegal data address",
        3 => "Illegal data value",
        4 => "Slave device failure",
        5 => "Acknowledge",
        6 => "Slave device busy",
        7 => "Negative acknowledge",
        8 => "Memory parity error",
        10 => "Gateway path unavailable",
        11 => "Gateway target failed to respond",
        _ => "Unknown exception",
    }
}

/// The "Register <- Value" lines for the words in `data`, starting at
/// register `start`.
fn write_table(start: u16, data: &[u8]) -> String {
    let mut s = String::new();
    for (i, w) in data.as_chunks::<2>().0.iter().enumerate() {
        let _ = writeln!(s, "{} <- {}", u32::from(start) + i as u32, u16::from_be_bytes(*w));
    }
    s
}

/// Decoded description of a query frame.
fn describe_query(query: &[u8]) -> String {
    let Ok((hdr, func, d)) = frame(query) else {
        return "Malformed frame\n".into();
    };
    let uid = hdr.unit_id;
    let Some(name) = function_name(func) else {
        return format!("Function 0x{func:02X} from UID {uid}\n");
    };
    let malformed = || format!("{name}. UID: {uid}\nMalformed request ({} data bytes)\n", d.len());
    let mut s = String::new();
    match func {
        0x03 | 0x04 => {
            let (Some(a), Some(n)) = (be16(d, 0), be16(d, 2)) else { return malformed() };
            let _ = writeln!(s, "{name}\nReference: {a}. Word count: {n}. UID: {uid}");
            let _ = writeln!(s, "Register(s): {}", register_list(a, n));
        }
        0x06 => {
            let (Some(a), Some(v)) = (be16(d, 0), be16(d, 2)) else { return malformed() };
            let _ = writeln!(s, "{name}\nReference: {a}. UID: {uid}");
            let _ = writeln!(s, "Register <- Value:\n{a} <- {v}");
        }
        0x07 => {
            let _ = writeln!(s, "{name}\nUID: {uid}");
        }
        0x08 => {
            let (Some(sub), Some(v)) = (be16(d, 0), be16(d, 2)) else { return malformed() };
            let _ = writeln!(s, "Diagnostics function from UID {uid}");
            let _ = writeln!(s, "Sub-function: 0x{sub:04X}. Data: 0x{v:04X}");
        }
        0x10 => {
            let (Some(a), Some(n)) = (be16(d, 0), be16(d, 2)) else { return malformed() };
            let _ = writeln!(s, "{name}\nReference: {a}. Word count: {n}. UID: {uid}");
            let _ = writeln!(s, "Register <- Value:");
            s.push_str(&write_table(a, d.get(5..).unwrap_or(&[])));
        }
        0x16 => {
            let (Some(a), Some(and), Some(or)) = (be16(d, 0), be16(d, 2), be16(d, 4)) else {
                return malformed();
            };
            let _ = writeln!(s, "{name} from UID {uid}  AND: 0x{and:04X} OR: 0x{or:04X}");
            let _ = writeln!(s, "Reference: {a}");
        }
        _ => {
            // 0x17
            let (Some(r), Some(rn), Some(w), Some(wn)) = (be16(d, 0), be16(d, 2), be16(d, 4), be16(d, 6)) else {
                return malformed();
            };
            let _ = writeln!(s, "{name}. UID: {uid}");
            let _ = writeln!(
                s,
                "Read reference: {r}. Write reference: {w}. Registers to read: {rn}. Registers to write: {wn}"
            );
            let _ = writeln!(s, "Register(s) to read: {}", register_list(r, rn));
            let _ = writeln!(s, "Write: Register <- Value:");
            s.push_str(&write_table(w, d.get(9..).unwrap_or(&[])));
        }
    }
    s
}

/// Decoded description of a reply frame.
fn describe_reply(query: &[u8], reply: &[u8]) -> String {
    let Ok((_, func, d)) = frame(reply) else {
        return "Malformed frame\n".into();
    };
    if func & 0x80 != 0 {
        let code = d.first().copied().unwrap_or(0);
        return format!("Exception {code}, {}\n", exception_name(code));
    }
    // Where the returned registers start comes from the query.
    let qd = frame(query).map_or(&[][..], |(_, _, d)| d);
    let mut s = String::new();
    match func {
        0x03 | 0x04 | 0x17 => {
            let start = be16(qd, 0).unwrap_or(0);
            s.push_str("Register : Value\n");
            for (i, w) in d.get(1..).unwrap_or(&[]).as_chunks::<2>().0.iter().enumerate() {
                let _ = writeln!(s, "{}:{}", u32::from(start) + i as u32, u16::from_be_bytes(*w));
            }
        }
        0x06 => {
            if let (Some(a), Some(v)) = (be16(d, 0), be16(d, 2)) {
                let _ = writeln!(s, "Register : Value\n{a}:{v}");
            }
        }
        0x07 => {
            if let Some(b) = d.first() {
                let _ = writeln!(s, "Exception status: 0x{b:02X}");
            }
        }
        0x08 => {
            if let (Some(sub), Some(v)) = (be16(d, 0), be16(d, 2)) {
                let _ = writeln!(s, "Sub-function: 0x{sub:04X}. Data: 0x{v:04X}");
            }
        }
        0x10 => {
            if let (Some(a), Some(n)) = (be16(d, 0), be16(d, 2)) {
                let _ = writeln!(s, "Written: Reference: {a}. Word count: {n}");
            }
        }
        0x16 => {
            if let (Some(a), Some(and), Some(or)) = (be16(d, 0), be16(d, 2), be16(d, 4)) {
                let _ = writeln!(s, "Reference: {a}. AND: 0x{and:04X} OR: 0x{or:04X}");
            }
        }
        _ => {}
    }
    s
}

/// Format Unix seconds as a UTC `YYYY-MM-DD HH-MM-SS` stamp that is valid in
/// Windows file names.
pub fn format_timestamp(unix_secs: u64) -> String {
    let days = (unix_secs / 86_400) as i64;
    let rem = unix_secs % 86_400;
    // Civil-from-days (proleptic Gregorian), after H. Hinnant.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}-{:02}-{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Replace characters that Windows (or any common file system) rejects in
/// file names, such as the colons of an IPv6 address, with `_`.
pub fn sanitize_file_part(part: &str) -> String {
    part.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect()
}

/// The log file name `ananasar-<label>-<time>.log`, safe on every platform.
/// `label` is usually the port; the time is UTC.
pub fn file_name(label: &str, unix_secs: u64) -> String {
    format!("ananasar-{}-{}.log", sanitize_file_part(label), format_timestamp(unix_secs).replace(' ', "_"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::build;

    #[test]
    fn raw_entry_layout() {
        let q = build(7, 1, 0x03, &[0, 10, 0, 2]);
        let r = build(7, 1, 0x03, &[4, 0, 5, 0, 9]);
        let e = Entry { number: 1, elapsed_ms: 0, connection: 1, query: &q, response: Response::Reply(&r) };
        let expected = "#1 0 ms Connection: 1\nQuery:\n00 07 00 00 00 06 01 03 00 0A 00 02\nReply:\n\
                        00 07 00 00 00 07 01 03 04 00 05 00 09\n\n";
        assert_eq!(format_entry(&e, LogMode::Raw), expected);
    }

    #[test]
    fn interpretive_entry_layout() {
        let q = build(7, 1, 0x03, &[0, 10, 0, 2]);
        let r = build(7, 1, 0x03, &[4, 0, 5, 0, 9]);
        let e = Entry { number: 1, elapsed_ms: 0, connection: 1, query: &q, response: Response::Reply(&r) };
        let expected = "#1 0 ms Connection: 1\n\
Query:\n\
00 07 00 00 00 06 01 03 00 0A 00 02\n\
\n\
Read holding registers\n\
Reference: 10. Word count: 2. UID: 1\n\
Register(s): 10 11\n\
\n\
Reply:\n\
00 07 00 00 00 07 01 03 04 00 05 00 09\n\
\n\
Register : Value\n\
10:5\n11:9\n\
\n";
        assert_eq!(format_entry(&e, LogMode::Interpretive), expected);
    }

    #[test]
    fn exception_reply_is_decoded() {
        let q = build(2, 1, 0x04, &[0xFF, 0xFF, 0, 2]);
        let r = build(2, 1, 0x84, &[2]);
        let e = Entry { number: 3, elapsed_ms: 1250, connection: 2, query: &q, response: Response::Reply(&r) };
        let s = format_entry(&e, LogMode::Interpretive);
        assert!(s.starts_with("#3 1250 ms Connection: 2\n"));
        assert!(s.contains("Read input registers\nReference: 65535. Word count: 2."));
        assert!(s.contains("Register(s): 65535 65536\n"));
        assert!(s.ends_with("Reply:\n00 02 00 00 00 03 01 84 02\n\nException 2, Illegal data address\n\n"));
    }

    #[test]
    fn write_functions_are_described() {
        let q6 = build(1, 1, 0x06, &[0, 5, 0x12, 0x34]);
        assert_eq!(describe_query(&q6), "Write single register\nReference: 5. UID: 1\nRegister <- Value:\n5 <- 4660\n");
        let q16 = build(1, 1, 0x10, &[0, 10, 0, 2, 4, 0, 1, 0, 2]);
        assert!(describe_query(&q16)
            .contains("Write holding registers\nReference: 10. Word count: 2. UID: 1\nRegister <- Value:\n10 <- 1\n11 <- 2\n"));
        let q22 = build(1, 1, 0x16, &[0, 4, 0xFF, 0x00, 0x00, 0x12]);
        assert!(describe_query(&q22).starts_with("Mask write register from UID 1  AND: 0xFF00 OR: 0x0012\n"));
        let q23 = build(1, 1, 0x17, &[0, 1, 0, 2, 0, 9, 0, 1, 2, 0, 7]);
        let d = describe_query(&q23);
        assert!(d.contains("Read reference: 1. Write reference: 9. Registers to read: 2. Registers to write: 1"));
        assert!(d.contains("Write: Register <- Value:\n9 <- 7\n"));
        assert!(describe_query(&build(1, 1, 0x07, &[])).starts_with("Read exception status"));
        assert!(describe_query(&build(1, 7, 0x08, &[0, 0, 0xA5, 0x37])).starts_with("Diagnostics function from UID 7"));
        assert!(describe_query(&build(1, 1, 0x2B, &[])).starts_with("Function 0x2B from UID 1"));
    }

    #[test]
    fn errors_and_empty_queries() {
        let q = build(9, 5, 0x03, &[0, 1, 0, 1]);
        let e = Entry { number: 2, elapsed_ms: 7, connection: 1, query: &q, response: Response::Error("unit id mismatch") };
        assert_eq!(
            format_entry(&e, LogMode::Raw),
            "#2 7 ms Connection: 1\nQuery:\n00 09 00 00 00 06 05 03 00 01 00 01\nError: unit id mismatch\n\n"
        );
        let e = Entry { number: 3, elapsed_ms: 8, connection: 1, query: &[], response: Response::Error("bad MBAP length") };
        assert_eq!(
            format_entry(&e, LogMode::Interpretive),
            "#3 8 ms Connection: 1\nQuery:\n(no data)\nError: bad MBAP length\n\n"
        );
        // Truncated payloads never panic.
        let short = build(1, 1, 0x03, &[0, 1]);
        assert!(describe_query(&short).contains("Malformed request (2 data bytes)"));
        assert_eq!(describe_query(&[1, 2, 3]), "Malformed frame\n");
    }

    #[test]
    fn long_register_lists_are_cut() {
        let q = build(1, 1, 0x03, &[0, 0, 0xFF, 0xFF]);
        let s = describe_query(&q);
        assert!(s.contains("Word count: 65535"));
        assert!(s.trim_end().ends_with(" 124 ..."));
    }

    #[test]
    fn timestamps_and_file_names() {
        assert_eq!(format_timestamp(0), "1970-01-01 00-00-00");
        assert_eq!(format_timestamp(1_709_164_800 + 3661), "2024-02-29 01-01-01");
        let n = file_name("fe80::1%eth0", 0);
        assert_eq!(n, "ananasar-fe80__1%eth0-1970-01-01_00-00-00.log");
        assert!(!n.contains([':', '/', '\\', '*', '?', '"', '<', '>', '|']));
    }
}
