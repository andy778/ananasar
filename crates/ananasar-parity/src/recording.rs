//! The recording format shared by the scenario runner, the proxy, the diff
//! tool and the golden-fixture tests.
//!
//! A recording is plain text. `## name` starts a section (a scenario, or one
//! proxied connection), `#` lines are comments, `> hex` is a frame sent to the
//! server, `< hex` a frame the server answered with, `< none` means no reply
//! within the timeout and `< closed` means the server closed the connection.
//! Whitespace inside the hex is ignored; the writer groups it by MBAP field.
//!
//! This file uses only `std` so the core crate's tests can include it
//! directly and replay recordings without a dependency on this crate.

use std::fmt::Write as _;

/// One line of a recording.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// Bytes a client sent to the server.
    Sent(Vec<u8>),
    /// A frame (or stray bytes) the server sent back.
    Received(Vec<u8>),
    /// The server stayed silent until the timeout.
    NoReply,
    /// The server closed the connection.
    Closed,
}

/// A named run of entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// Scenario name or connection label.
    pub name: String,
    /// The entries, in wire order.
    pub entries: Vec<Entry>,
}

/// A whole recording file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recording {
    /// Sections in file order.
    pub sections: Vec<Section>,
}

/// Hex with a space after the MBAP fields: `0001 0000 0006 01 03 0000 0001`.
pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2 + 6);
    for (i, b) in bytes.iter().enumerate() {
        if matches!(i, 2 | 4 | 6 | 7 | 8) {
            out.push(' ');
        }
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Parse hex, ignoring whitespace.
pub fn parse_hex(text: &str) -> Result<Vec<u8>, String> {
    let digits: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    if !digits.len().is_multiple_of(2) {
        return Err(format!("odd number of hex digits in {text:?}"));
    }
    digits
        .chunks(2)
        .map(|pair| {
            let s: String = pair.iter().collect();
            u8::from_str_radix(&s, 16).map_err(|_| format!("bad hex {s:?}"))
        })
        .collect()
}

/// Split a byte stream into Modbus/TCP frames using the MBAP length field.
/// Whatever cannot be a complete frame is returned as the last element.
pub fn split_frames(mut bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    while !bytes.is_empty() {
        let total = (bytes.len() >= 6).then(|| 6 + usize::from(u16::from_be_bytes([bytes[4], bytes[5]])));
        match total {
            Some(t) if t >= 8 && t <= bytes.len() => {
                frames.push(bytes[..t].to_vec());
                bytes = &bytes[t..];
            }
            _ => {
                frames.push(bytes.to_vec());
                break;
            }
        }
    }
    frames
}

impl Entry {
    /// The recording line for this entry.
    pub fn to_line(&self) -> String {
        match self {
            Self::Sent(b) => format!("> {}", hex(b)),
            Self::Received(b) => format!("< {}", hex(b)),
            Self::NoReply => "< none".to_owned(),
            Self::Closed => "< closed".to_owned(),
        }
    }
}

impl Recording {
    /// Render as recording text.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for section in &self.sections {
            let _ = writeln!(out, "## {}", section.name);
            for entry in &section.entries {
                let _ = writeln!(out, "{}", entry.to_line());
            }
            out.push('\n');
        }
        out
    }

    /// Parse recording text; errors carry the 1-based line number.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut rec = Self::default();
        for (i, raw) in text.lines().enumerate() {
            let line = raw.trim();
            let at = |e: String| format!("line {}: {e}", i + 1);
            if line.is_empty() {
                continue;
            }
            if let Some(name) = line.strip_prefix("##") {
                rec.sections.push(Section { name: name.trim().to_owned(), entries: Vec::new() });
            } else if line.starts_with('#') {
                continue;
            } else {
                let entry = if let Some(h) = line.strip_prefix('>') {
                    Entry::Sent(parse_hex(h).map_err(at)?)
                } else if let Some(h) = line.strip_prefix('<') {
                    match h.trim() {
                        "none" => Entry::NoReply,
                        "closed" => Entry::Closed,
                        other => Entry::Received(parse_hex(other).map_err(at)?),
                    }
                } else {
                    return Err(at(format!("expected '>', '<' or '#', got {line:?}")));
                };
                match rec.sections.last_mut() {
                    Some(s) => s.entries.push(entry),
                    None => return Err(at("entry before the first '## section'".to_owned())),
                }
            }
        }
        Ok(rec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let rec = Recording {
            sections: vec![Section {
                name: "a".into(),
                entries: vec![
                    Entry::Sent(vec![0, 1, 0, 0, 0, 6, 1, 3, 0, 0, 0, 1]),
                    Entry::Received(vec![0, 1, 0, 0, 0, 5, 1, 3, 2, 0, 9]),
                    Entry::Sent(vec![1, 2, 3]),
                    Entry::NoReply,
                    Entry::Closed,
                ],
            }],
        };
        assert_eq!(Recording::parse(&rec.to_text()).unwrap(), rec);
    }

    #[test]
    fn rejects_garbage() {
        assert!(Recording::parse("> 00\n").is_err());
        assert!(Recording::parse("## a\n? 00\n").is_err());
        assert!(Recording::parse("## a\n> 0\n").is_err());
        assert!(Recording::parse("## a\n> zz\n").is_err());
    }

    #[test]
    fn splits_pipelined_frames() {
        let a = [0, 1, 0, 0, 0, 6, 1, 3, 0, 0, 0, 1];
        let b = [0, 2, 0, 0, 0, 6, 1, 4, 0, 0, 0, 1];
        let both = [a, b].concat();
        assert_eq!(split_frames(&both), vec![a.to_vec(), b.to_vec()]);
        let cut = [a.as_slice(), &b[..5]].concat();
        assert_eq!(split_frames(&cut), vec![a.to_vec(), b[..5].to_vec()]);
    }
}
