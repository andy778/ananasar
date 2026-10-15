//! Golden fixtures: recordings of the original Ananas, committed as plain
//! frames, with notes the comparison understands.
//!
//! A golden file is a normal recording (see [`crate::recording`]) plus
//! special comment lines that start with `#!`:
//!
//! - `#! quirk: text` inside a section: the original's reply looks like a bug
//!   or an oddity. Informational; Ananasar is still expected to match.
//! - `#! deviation: reason` inside a section: Ananasar differs here on
//!   purpose. The comparison expects the section to differ and fails if it
//!   no longer does, so stale entries get removed.
//! - `#! default-deviation: reason` before the first section: every section
//!   without its own `deviation` that differs is allowed to, for the stated
//!   reason (used for options Ananasar does not implement yet). Sections that
//!   match need no entry.
//!
//! Plain `#` comments are ignored everywhere.

use crate::diff::{Options, verdicts};
use crate::recording::Recording;

/// What a note says about its section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteKind {
    /// The original behaves oddly here.
    Quirk,
    /// Ananasar deliberately differs here.
    Deviation,
}

/// One `#!` note attached to a section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// The section it appears in.
    pub section: String,
    /// What kind of note it is.
    pub kind: NoteKind,
    /// The text after the colon.
    pub text: String,
}

/// A parsed golden file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Golden {
    /// The frames.
    pub recording: Recording,
    /// Section notes in file order.
    pub notes: Vec<Note>,
    /// File-wide reason for allowed differences, if given.
    pub default_deviation: Option<String>,
}

/// Parse golden text: the recording plus its `#!` notes.
pub fn parse(text: &str) -> Result<Golden, String> {
    let recording = Recording::parse(text)?;
    let mut golden = Golden { recording, ..Golden::default() };
    let mut section: Option<String> = None;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if let Some(name) = line.strip_prefix("##") {
            section = Some(name.trim().to_owned());
        } else if let Some(note) = line.strip_prefix("#!") {
            let (key, value) = note
                .split_once(':')
                .map(|(k, v)| (k.trim(), v.trim().to_owned()))
                .ok_or_else(|| format!("line {}: expected '#! kind: text'", i + 1))?;
            match (key, &section) {
                ("default-deviation", None) => golden.default_deviation = Some(value),
                ("quirk", Some(s)) => {
                    golden.notes.push(Note { section: s.clone(), kind: NoteKind::Quirk, text: value });
                }
                ("deviation", Some(s)) => {
                    golden.notes.push(Note { section: s.clone(), kind: NoteKind::Deviation, text: value });
                }
                _ => return Err(format!("line {}: '#! {key}' is not allowed here", i + 1)),
            }
        }
    }
    Ok(golden)
}

/// The outcome of comparing a golden file with a recording of Ananasar.
#[derive(Debug, Default)]
pub struct Report {
    /// Differences nobody listed: these fail the test.
    pub unexpected: Vec<String>,
    /// Sections marked `deviation` that no longer differ: these fail too.
    pub stale: Vec<String>,
    /// Differences that are allowed, with their reasons.
    pub allowed: Vec<(String, String)>,
}

impl Report {
    /// Whether the comparison passes.
    pub fn is_clean(&self) -> bool {
        self.unexpected.is_empty() && self.stale.is_empty()
    }

    /// A readable summary of what failed.
    pub fn failures(&self) -> String {
        let mut out = String::new();
        for d in &self.unexpected {
            out.push_str(&format!("unexpected difference:\n{d}\n"));
        }
        for s in &self.stale {
            out.push_str(&format!("[{s}] is marked as a deviation but now matches: remove the note\n"));
        }
        out
    }
}

/// Compare `golden` (side a) with `ours` (side b), scenario by scenario.
pub fn compare(golden: &Golden, ours: &Recording, opts: Options) -> Report {
    let mut report = Report::default();
    for verdict in verdicts(&golden.recording, ours, opts) {
        let own = golden
            .notes
            .iter()
            .find(|n| n.kind == NoteKind::Deviation && n.section == verdict.name)
            .map(|n| n.text.clone());
        match (verdict.matches(), own) {
            (true, None) => {}
            (true, Some(_)) => report.stale.push(verdict.name),
            (false, Some(reason)) => report.allowed.push((verdict.name, reason)),
            (false, None) => match &golden.default_deviation {
                Some(reason) => report.allowed.push((verdict.name, reason.clone())),
                None => report.unexpected.extend(verdict.differences),
            },
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::{Entry, Section};

    fn rec(name: &str, reply: u8) -> Recording {
        Recording {
            sections: vec![Section {
                name: name.into(),
                entries: vec![Entry::Sent(vec![0, 1, 0, 0, 0, 2, 1, 7]), Entry::Received(vec![0, 1, 0, 0, 0, 3, 1, 7, reply])],
            }],
        }
    }

    const BASE: &str = "## a\n> 0001 0000 0002 01 07\n< 0001 0000 0003 01 07 00\n";

    #[test]
    fn parses_notes() {
        let text = format!("#! default-deviation: not built\n{BASE}#! quirk: odd\n#! deviation: on purpose\n");
        let g = parse(&text).unwrap();
        assert_eq!(g.default_deviation.as_deref(), Some("not built"));
        assert_eq!(g.notes.len(), 2);
        assert_eq!(g.notes[1].kind, NoteKind::Deviation);
    }

    #[test]
    fn rejects_misplaced_or_unknown_notes() {
        assert!(parse("#! deviation: x\n## a\n").is_err());
        assert!(parse("## a\n#! default-deviation: x\n").is_err());
        assert!(parse("## a\n#! whatever: x\n").is_err());
        assert!(parse("## a\n#! no colon\n").is_err());
    }

    #[test]
    fn matching_golden_is_clean_and_difference_is_reported() {
        let g = parse(BASE).unwrap();
        assert!(compare(&g, &rec("a", 0), Options::default()).is_clean());
        let report = compare(&g, &rec("a", 9), Options::default());
        assert_eq!(report.unexpected.len(), 1);
    }

    #[test]
    fn listed_deviation_is_allowed_but_must_still_differ() {
        let g = parse(&format!("{BASE}#! deviation: on purpose\n")).unwrap();
        let report = compare(&g, &rec("a", 9), Options::default());
        assert!(report.is_clean());
        assert_eq!(report.allowed.len(), 1);
        let stale = compare(&g, &rec("a", 0), Options::default());
        assert_eq!(stale.stale, vec!["a".to_owned()]);
        assert!(!stale.is_clean());
    }

    #[test]
    fn default_deviation_allows_any_difference() {
        let g = parse(&format!("#! default-deviation: not built\n{BASE}")).unwrap();
        assert!(compare(&g, &rec("a", 9), Options::default()).is_clean());
        assert!(compare(&g, &rec("a", 0), Options::default()).is_clean());
    }

    #[test]
    fn missing_sections_are_differences() {
        let g = parse(BASE).unwrap();
        let report = compare(&g, &rec("b", 0), Options::default());
        assert_eq!(report.unexpected.len(), 2);
    }
}
