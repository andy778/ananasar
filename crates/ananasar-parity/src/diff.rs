//! Compare two recordings. Transaction ids are ignored (they are zeroed
//! before comparing); recordings carry no timing, so timing never matters.
//! Section names are shown in reports but not compared, so a scenario run can
//! be compared with a proxied session of the same requests.

use crate::recording::{Entry, Recording};

/// A one-line decoded view of a frame, for reports.
pub fn describe(bytes: &[u8]) -> String {
    if bytes.len() < 8 {
        return format!("{} bytes, too short for a frame", bytes.len());
    }
    let fc = bytes[7];
    let mut s = format!("unit {} fc {:#04x}", bytes[6], fc & 0x7f);
    if fc & 0x80 != 0 {
        match bytes.get(8) {
            Some(code) => s.push_str(&format!(" exception {code:#04x}")),
            None => s.push_str(" exception (no code)"),
        }
    }
    s
}

fn normalize(entry: &Entry) -> Entry {
    let zero_txn = |b: &[u8]| {
        let mut b = b.to_vec();
        for byte in b.iter_mut().take(2) {
            *byte = 0;
        }
        b
    };
    match entry {
        Entry::Sent(b) => Entry::Sent(zero_txn(b)),
        Entry::Received(b) => Entry::Received(zero_txn(b)),
        other => other.clone(),
    }
}

fn show(entry: Option<&Entry>) -> String {
    match entry {
        None => "(nothing)".to_owned(),
        Some(e @ (Entry::Sent(b) | Entry::Received(b))) => format!("{}   [{}]", e.to_line(), describe(b)),
        Some(e) => e.to_line(),
    }
}

/// Every difference between `a` and `b`, one report block per difference.
/// Empty means the recordings are equivalent.
pub fn diff(a: &Recording, b: &Recording) -> Vec<String> {
    let mut out = Vec::new();
    if a.sections.len() != b.sections.len() {
        out.push(format!("section count differs: {} vs {}", a.sections.len(), b.sections.len()));
    }
    for (sa, sb) in a.sections.iter().zip(&b.sections) {
        let label = if sa.name == sb.name {
            sa.name.clone()
        } else {
            format!("{} / {}", sa.name, sb.name)
        };
        let n = sa.entries.len().max(sb.entries.len());
        for i in 0..n {
            let (ea, eb) = (sa.entries.get(i), sb.entries.get(i));
            if ea.map(normalize) != eb.map(normalize) {
                out.push(format!("[{label}] entry {}\n  a: {}\n  b: {}", i + 1, show(ea), show(eb)));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::Section;

    fn rec(entries: Vec<Entry>) -> Recording {
        Recording { sections: vec![Section { name: "s".into(), entries }] }
    }

    #[test]
    fn ignores_transaction_ids() {
        let a = rec(vec![Entry::Sent(vec![0, 1, 0, 0, 0, 2, 1, 7]), Entry::Received(vec![0, 1, 0, 0, 0, 3, 1, 7, 0])]);
        let b = rec(vec![Entry::Sent(vec![9, 9, 0, 0, 0, 2, 1, 7]), Entry::Received(vec![9, 9, 0, 0, 0, 3, 1, 7, 0])]);
        assert!(diff(&a, &b).is_empty());
    }

    #[test]
    fn reports_payload_and_reply_differences() {
        let a = rec(vec![Entry::Received(vec![0, 1, 0, 0, 0, 3, 1, 0x83, 2])]);
        let b = rec(vec![Entry::Received(vec![0, 1, 0, 0, 0, 3, 1, 0x83, 3]), Entry::NoReply]);
        let d = diff(&a, &b);
        assert_eq!(d.len(), 2);
        assert!(d[0].contains("exception 0x02") && d[0].contains("exception 0x03"));
    }
}
