//! Compares our menu tree with the original program's, as recorded in
//! `original_menus.txt` (its format is described at the top of that file).
//! Runs without a display, so a missing entry, a wrong shortcut or an empty
//! submenu fails in CI. Differences we keep on purpose are written down in the
//! spec as `~` deviations or `+` additions, with the reason.

#![allow(clippy::arithmetic_side_effects)]

use super::{build, Kind, MenuItem, MenuState};
use ananasar_core::server::ServerConfig;
use eframe::egui::KeyboardShortcut;

const SPEC: &str = include_str!("original_menus.txt");
/// Stands in for `<n>` in the spec.
const TEST_PORT: u16 = 1234;

#[derive(Debug)]
struct SpecItem {
    caption: String,
    shortcut: Option<String>,
    separator: bool,
    check: bool,
    checked: bool,
    enabled: bool,
    extra: bool,
    children: Vec<SpecItem>,
    /// Alternative values for `kind` ("check"/"action"), `checked` and `enabled` that are allowed too.
    deviations: Vec<(String, String)>,
}

struct Spec {
    menus: Vec<SpecItem>,
    hidden: Vec<String>,
}

fn parse_item(text: &str, extra: bool) -> SpecItem {
    let (head, flags) = match text.find('[') {
        Some(i) => (&text[..i], &text[i..]),
        None => (text, ""),
    };
    let (caption, shortcut) = match head.split_once(" @ ") {
        Some((c, s)) => (c.trim(), Some(s.trim().to_owned())),
        None => (head.trim(), None),
    };
    let has = |flag: &str| flags.contains(&format!("[{flag}]"));
    SpecItem {
        caption: caption.replace("<n>", &TEST_PORT.to_string()),
        shortcut,
        separator: false,
        check: has("check") || has("checked"),
        checked: has("checked"),
        enabled: !has("disabled"),
        extra,
        children: Vec::new(),
        deviations: Vec::new(),
    }
}

fn parse(text: &str) -> Spec {
    let mut spec = Spec { menus: Vec::new(), hidden: Vec::new() };
    // The chain of open parents: (indent, path of child indexes from the top level).
    let mut stack: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut last: Vec<usize> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim_end();
        let body = line.trim_start();
        if body.is_empty() || body.starts_with('#') {
            continue;
        }
        let fail = |what: &str| -> ! { std::panic::panic_any(format!("original_menus.txt line {}: {what}", n + 1)) };
        if let Some(hidden) = body.strip_prefix("!hidden ") {
            spec.hidden.push(hidden.trim().to_owned());
            continue;
        }
        if let Some(dev) = body.strip_prefix('~') {
            let Some((assign, _reason)) = dev.split_once(" : ") else { fail("a deviation needs ` : reason`") };
            let Some((field, value)) = assign.trim().split_once('=') else { fail("a deviation is field=value") };
            if !matches!(field, "kind" | "checked" | "enabled") {
                fail("deviation field must be kind, checked or enabled");
            }
            node_mut(&mut spec.menus, &last).deviations.push((field.to_owned(), value.to_owned()));
            continue;
        }
        let indent = line.len() - body.len();
        if indent % 2 != 0 {
            fail("indent must be a multiple of two spaces");
        }
        while stack.last().is_some_and(|(i, _)| *i >= indent) {
            stack.pop();
        }
        let (body, extra) = body.strip_prefix("+ ").map_or((body, false), |b| (b, true));
        let mut item = parse_item(body, extra);
        if body == "---" {
            item.separator = true;
        }
        let siblings = match stack.last() {
            Some((_, path)) => &mut node_mut(&mut spec.menus, path).children,
            None => &mut spec.menus,
        };
        if !extra && siblings.iter().any(|s| s.extra) {
            fail("an original entry cannot follow a `+` addition");
        }
        siblings.push(item);
        let mut path = stack.last().map(|(_, p)| p.clone()).unwrap_or_default();
        path.push(siblings.len() - 1);
        last = path.clone();
        stack.push((indent, path));
    }
    spec
}

fn node_mut<'a>(menus: &'a mut [SpecItem], path: &[usize]) -> &'a mut SpecItem {
    let Some((first, rest)) = path.split_first() else { std::panic::panic_any("a `~` line needs an entry above it".to_owned()) };
    let mut node = menus.get_mut(*first).unwrap_or_else(|| std::panic::panic_any("bad spec path".to_owned()));
    for i in rest {
        node = node.children.get_mut(*i).unwrap_or_else(|| std::panic::panic_any("bad spec path".to_owned()));
    }
    node
}

fn shortcut_text(sc: &KeyboardShortcut) -> String {
    let m = sc.modifiers;
    let mut out = String::new();
    for (on, name) in [(m.command || m.ctrl, "Ctrl"), (m.alt, "Alt"), (m.shift, "Shift")] {
        if on {
            out.push_str(name);
            out.push('+');
        }
    }
    out.push_str(sc.logical_key.name());
    out
}

fn our_menus() -> Vec<MenuItem> {
    let cfg = ServerConfig::default();
    build(&MenuState {
        value_list_edit: false,
        addr_hex: false,
        hex: false,
        listening: true,
        port: TEST_PORT,
        tooltips: true,
        supported: &cfg.supported,
        forced_exception: cfg.forced_exception,
        faults: &cfg,
    })
}

/// A menu tree in the spec's own notation, for the failure message.
fn render(items: &[MenuItem], depth: usize, out: &mut String) {
    for item in items {
        let pad = "  ".repeat(depth + 1);
        if item.kind == Kind::Separator {
            out.push_str(&format!("{pad}---\n"));
            continue;
        }
        let mut line = format!("{pad}{}", item.caption);
        if let Some(sc) = &item.shortcut {
            line.push_str(&format!("  @ {}", shortcut_text(sc)));
        }
        if item.kind == Kind::Check {
            line.push_str(if item.checked { "  [checked]" } else { "  [check]" });
        }
        if !item.enabled {
            line.push_str("  [disabled]");
        }
        out.push_str(&line);
        out.push('\n');
        if let Kind::Submenu(children) = &item.kind {
            render(children, depth + 1, out);
        }
    }
}

fn describe(item: &MenuItem) -> String {
    let mut s = String::new();
    render(std::slice::from_ref(item), 0, &mut s);
    s.lines().next().unwrap_or("").trim().to_owned()
}

fn allowed(spec: &SpecItem, field: &str, original: &str) -> Vec<String> {
    let mut v = vec![original.to_owned()];
    v.extend(spec.deviations.iter().filter(|(f, _)| f == field).map(|(_, value)| value.clone()));
    v
}

fn compare(spec: &[SpecItem], ours: &[MenuItem], path: &str, diffs: &mut Vec<String>) {
    for i in 0..spec.len().max(ours.len()) {
        let at = format!("{path} [{}]", i + 1);
        let (s, o) = match (spec.get(i), ours.get(i)) {
            (Some(s), Some(o)) => (s, o),
            (Some(s), None) => {
                diffs.push(format!("{at}: missing {}", if s.separator { "separator".to_owned() } else { format!("'{}'", s.caption) }));
                continue;
            }
            (None, Some(o)) => {
                diffs.push(format!("{at}: unexpected extra entry '{}' (add it to the spec as a `+` addition if intended)", describe(o)));
                continue;
            }
            (None, None) => continue,
        };
        if s.separator || o.kind == Kind::Separator {
            if !(s.separator && o.kind == Kind::Separator) {
                let found = if o.kind == Kind::Separator { "a separator".to_owned() } else { format!("'{}'", o.caption) };
                let wanted = if s.separator { "a separator".to_owned() } else { format!("'{}'", s.caption) };
                diffs.push(format!("{at}: expected {wanted}, found {found}"));
            }
            continue;
        }
        let here = format!("{path} > {}", s.caption);
        if o.caption != s.caption {
            diffs.push(format!("{at}: expected caption '{}', found '{}'", s.caption, o.caption));
            continue;
        }
        let want_sc = s.shortcut.clone();
        let have_sc = o.shortcut.as_ref().map(shortcut_text);
        if want_sc != have_sc {
            diffs.push(format!("{here}: shortcut expected {want_sc:?}, found {have_sc:?}"));
        }
        let is_sub = matches!(o.kind, Kind::Submenu(_));
        if is_sub != !s.children.is_empty() {
            diffs.push(format!("{here}: expected {} submenu, found {}", if s.children.is_empty() { "no" } else { "a" }, if is_sub { "one" } else { "none" }));
        }
        if !is_sub {
            let kind = if o.kind == Kind::Check { "check" } else { "action" };
            if !allowed(s, "kind", if s.check { "check" } else { "action" }).iter().any(|k| k == kind) {
                diffs.push(format!("{here}: expected kind {}, found {kind}", if s.check { "check" } else { "action" }));
            }
            if o.kind == Kind::Check && !allowed(s, "checked", &s.checked.to_string()).contains(&o.checked.to_string()) {
                diffs.push(format!("{here}: expected checked={}, found checked={}", s.checked, o.checked));
            }
        }
        if !allowed(s, "enabled", &s.enabled.to_string()).contains(&o.enabled.to_string()) {
            diffs.push(format!("{here}: expected enabled={}, found enabled={}", s.enabled, o.enabled));
        }
        if let Kind::Submenu(children) = &o.kind {
            compare(&s.children, children, &here, diffs);
        }
    }
}

fn find_caption(items: &[MenuItem], hidden: &str, path: &str, diffs: &mut Vec<String>) {
    let norm = |s: &str| s.trim_end_matches('…').trim().to_lowercase();
    for item in items {
        if item.kind != Kind::Separator && norm(&item.caption) == norm(hidden) {
            diffs.push(format!("{path} > {}: the original hides this entry, ours must not show it", item.caption));
        }
        if let Kind::Submenu(children) = &item.kind {
            find_caption(children, hidden, &format!("{path} > {}", item.caption), diffs);
        }
    }
}

fn differences(spec: &Spec, ours: &[MenuItem]) -> Vec<String> {
    let mut diffs = Vec::new();
    compare(&spec.menus, ours, "menu bar", &mut diffs);
    for hidden in &spec.hidden {
        find_caption(ours, hidden, "menu bar", &mut diffs);
    }
    diffs
}

#[test]
fn our_menus_match_the_original() {
    let spec = parse(SPEC);
    let ours = our_menus();
    let diffs = differences(&spec, &ours);
    let mut tree = String::new();
    render(&ours, 0, &mut tree);
    assert!(diffs.is_empty(), "\nour menus differ from original_menus.txt:\n  {}\n\nour menus are:\n{tree}", diffs.join("\n  "));
}

#[test]
fn every_enabled_entry_does_something() {
    fn walk(items: &[MenuItem], path: &str, bad: &mut Vec<String>) {
        for item in items {
            match &item.kind {
                Kind::Submenu(children) => {
                    if children.is_empty() {
                        bad.push(format!("{path} > {}: empty submenu", item.caption));
                    }
                    walk(children, &format!("{path} > {}", item.caption), bad);
                }
                Kind::Separator => {}
                _ if item.enabled && item.action.is_none() => bad.push(format!("{path} > {}: enabled but has no action", item.caption)),
                _ => {}
            }
        }
    }
    let mut bad = Vec::new();
    walk(&our_menus(), "menu bar", &mut bad);
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}

#[test]
fn the_comparison_catches_regressions() {
    let spec = parse(SPEC);

    let mut ours = our_menus();
    // Remove Edit > Value List.
    if let Some(MenuItem { kind: Kind::Submenu(items), .. }) = ours.get_mut(1) {
        items.remove(0);
    }
    assert!(!differences(&spec, &ours).is_empty());

    let mut ours = our_menus();
    // Empty Options > Modbus > Active MB Functions.
    if let Some(MenuItem { kind: Kind::Submenu(options), .. }) = ours.get_mut(3)
        && let Some(MenuItem { kind: Kind::Submenu(modbus), .. }) = options.get_mut(0)
        && let Some(MenuItem { kind: Kind::Submenu(functions), .. }) = modbus.get_mut(0)
    {
        functions.clear();
    }
    assert!(!differences(&spec, &ours).is_empty());

    let mut ours = our_menus();
    // A hidden entry added at the end of View.
    if let Some(MenuItem { kind: Kind::Submenu(view), .. }) = ours.get_mut(2) {
        let mut extra = view[0].clone();
        "Set IP…".clone_into(&mut extra.caption);
        view.push(extra);
    }
    let diffs = differences(&spec, &ours);
    assert!(diffs.iter().any(|d| d.contains("hides")), "{diffs:?}");
}

#[test]
fn additions_are_allowed_after_the_original_entries_only() {
    let text = "View\n  Values as Hex  [check]\n  + Theme\n";
    let spec = parse(text);
    let ours = [MenuItem {
        caption: "View".to_owned(),
        shortcut: None,
        kind: Kind::Submenu(vec![
            MenuItem { caption: "Values as Hex".to_owned(), shortcut: None, kind: Kind::Check, checked: false, enabled: true, action: None, min_width: None },
            MenuItem { caption: "Theme".to_owned(), shortcut: None, kind: Kind::Action, checked: false, enabled: true, action: None, min_width: None },
        ]),
        checked: false,
        enabled: true,
        action: None,
        min_width: None,
    }];
    assert_eq!(differences(&spec, &ours), Vec::<String>::new());
    let result = std::panic::catch_unwind(|| parse("View\n  + Theme\n  Values as Hex\n"));
    assert!(result.is_err());
}
