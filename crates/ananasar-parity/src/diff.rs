//! Compare two recordings. Transaction ids are ignored (they are zeroed
//! before comparing); recordings carry no timing, so timing never matters.
//! Section names are shown in reports but not compared, so a scenario run can
//! be compared with a proxied session of the same requests.

use crate::recording::{Entry, Recording, Section};

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

/// How strictly two recordings are compared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Compare transaction ids too. Needed for the fault options that change
    /// the id in a reply; the scenarios send fixed ids, so it is repeatable.
    pub strict_txn: bool,
}

fn normalize(entry: &Entry, opts: Options) -> Entry {
    let zero_txn = |b: &[u8]| {
        let mut b = b.to_vec();
        if !opts.strict_txn {
            for byte in b.iter_mut().take(2) {
                *byte = 0;
            }
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
    diff_with(a, b, Options::default())
}

/// [`diff`] with explicit [`Options`]. Sections are paired by position.
pub fn diff_with(a: &Recording, b: &Recording, opts: Options) -> Vec<String> {
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
        out.extend(section_differences(&label, sa, sb, opts));
    }
    out
}

fn section_differences(label: &str, a: &Section, b: &Section, opts: Options) -> Vec<String> {
    let n = a.entries.len().max(b.entries.len());
    (0..n)
        .filter_map(|i| {
            let (ea, eb) = (a.entries.get(i), b.entries.get(i));
            (ea.map(|e| normalize(e, opts)) != eb.map(|e| normalize(e, opts)))
                .then(|| format!("[{label}] entry {}\n  a: {}\n  b: {}", i + 1, show(ea), show(eb)))
        })
        .collect()
}

/// The outcome for one scenario.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// Scenario (section) name.
    pub name: String,
    /// Empty when the two sides agree; otherwise one block per difference,
    /// or a note that one side lacks the scenario.
    pub differences: Vec<String>,
}

impl Verdict {
    /// Whether both sides agree on this scenario.
    pub fn matches(&self) -> bool {
        self.differences.is_empty()
    }
}

/// Compare two recordings scenario by scenario, pairing sections by name, in
/// the order of `a` followed by any sections only `b` has.
pub fn verdicts(a: &Recording, b: &Recording, opts: Options) -> Vec<Verdict> {
    let mut out: Vec<Verdict> = a
        .sections
        .iter()
        .map(|sa| {
            let differences = match b.sections.iter().find(|s| s.name == sa.name) {
                Some(sb) => section_differences(&sa.name, sa, sb, opts),
                None => vec![format!("[{}] missing from b", sa.name)],
            };
            Verdict { name: sa.name.clone(), differences }
        })
        .collect();
    for sb in &b.sections {
        if !a.sections.iter().any(|s| s.name == sb.name) {
            out.push(Verdict { name: sb.name.clone(), differences: vec![format!("[{}] missing from a", sb.name)] });
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
