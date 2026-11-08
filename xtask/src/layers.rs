//! `cargo xtask layers`: checks the workspace's dependency graph against the
//! rules in ARCHITECTURE.md.
//!
//! The graph comes from `cargo tree` (normal and build dependencies, every
//! target, every feature), so `xtask` needs no JSON parser. Dev-dependencies
//! are not checked: tests may use whatever they need, and nothing that ships
//! depends on them.

use std::path::Path;
use std::process::Command;

/// Each workspace crate and the workspace crates it may depend on. This is the
/// graph in ARCHITECTURE.md; a test keeps the two the same.
pub const ALLOWED: &[(&str, &[&str])] = &[
    ("ananasar-core", &[]),
    ("ananasar-net", &["ananasar-core"]),
    ("ananasar-dde", &["ananasar-core"]),
    ("ananasar-storm", &["ananasar-net", "ananasar-core"]),
    ("ananasar-mcp", &["ananasar-net", "ananasar-core"]),
    ("ananasar-parity", &["ananasar-net", "ananasar-core"]),
    ("ananasar-gui", &["ananasar-storm", "ananasar-net", "ananasar-core", "ananasar-dde"]),
    ("ananasar-cli", &["ananasar-gui", "ananasar-mcp", "ananasar-storm", "ananasar-net", "ananasar-core"]),
];

/// Crates that may have no dependencies at all, internal or external.
const STD_ONLY: &[&str] = &["ananasar-core"];

/// Crates that must build without a GUI toolkit: the headless CLI and the
/// MCP server are built on servers without a desktop.
const HEADLESS: &[&str] =
    &["ananasar-core", "ananasar-net", "ananasar-dde", "ananasar-storm", "ananasar-mcp", "ananasar-parity"];

/// GUI toolkits a headless crate must not pull in, directly or through another crate.
const GUI_TOOLKITS: &[&str] = &["egui", "eframe", "winit", "wgpu"];

/// Workspace members that are repository tooling, not part of the product.
const UNCHECKED: &[&str] = &["xtask"];

/// One crate's dependencies, by package name.
#[derive(Debug, Default, PartialEq)]
pub struct Deps {
    pub direct: Vec<String>,
    /// Direct and indirect.
    pub all: Vec<String>,
}

/// Reads `cargo tree --prefix depth` output: each line is the depth followed by
/// `name vX.Y.Z ...`, with the crate itself at depth 0.
pub fn parse_tree(output: &str) -> Deps {
    let mut deps = Deps::default();
    for line in output.lines() {
        let rest = line.trim_start_matches(|c: char| c.is_ascii_digit());
        let depth_len = line.len().saturating_sub(rest.len());
        let Some(depth) = line.get(..depth_len).and_then(|d| d.parse::<u32>().ok()) else {
            // Blank lines and section headers carry no package.
            continue;
        };
        let Some(name) = rest.split_whitespace().next() else {
            continue;
        };
        if depth == 0 {
            continue;
        }
        if depth == 1 {
            deps.direct.push(name.to_owned());
        }
        deps.all.push(name.to_owned());
    }
    for list in [&mut deps.direct, &mut deps.all] {
        list.sort();
        list.dedup();
    }
    deps
}

/// Every broken rule, as a message naming the crate and the dependency.
/// `members` are the workspace's packages; `graph` holds the dependencies of
/// each one that is checked.
pub fn check(members: &[String], graph: &[(String, Deps)]) -> Vec<String> {
    let mut problems = Vec::new();

    for member in members {
        let name = member.as_str();
        if !UNCHECKED.contains(&name) && allowed(name).is_none() {
            problems.push(format!(
                "{name} is not in the layer table: add it to ALLOWED in xtask/src/layers.rs and to the graph in ARCHITECTURE.md"
            ));
        }
    }

    for (krate, deps) in graph {
        let krate = krate.as_str();
        if STD_ONLY.contains(&krate) {
            for dep in &deps.direct {
                problems.push(format!("{krate} depends on {dep}, but it must depend on std only"));
            }
        }
        if let Some(may_use) = allowed(krate) {
            for dep in deps.direct.iter().filter(|d| d.starts_with("ananasar-")) {
                if !may_use.contains(&dep.as_str()) {
                    let rule =
                        if may_use.is_empty() { "no other workspace crate".to_owned() } else { may_use.join(", ") };
                    problems.push(format!(
                        "{krate} depends on {dep}, which ARCHITECTURE.md does not allow (it may use: {rule})"
                    ));
                }
            }
        }
        if HEADLESS.contains(&krate) {
            for dep in deps.all.iter().filter(|d| GUI_TOOLKITS.contains(&d.as_str())) {
                problems.push(format!("{krate} pulls in {dep}, but it must build without a GUI toolkit"));
            }
        }
    }

    problems
}

fn allowed(krate: &str) -> Option<&'static [&'static str]> {
    ALLOWED.iter().find(|(name, _)| *name == krate).map(|(_, may_use)| *may_use)
}

/// Runs the check on the workspace at `root`, printing what it finds.
pub fn run(root: &Path) -> bool {
    let Some(members) = cargo_tree(root, &["--workspace", "--depth", "0", "--prefix", "none", "-e", "normal"]) else {
        return false;
    };
    // With `--prefix none` every non-blank line is one member, name first.
    let members: Vec<String> =
        members.lines().filter_map(|line| line.split_whitespace().next()).map(str::to_owned).collect();

    let mut graph = Vec::new();
    for member in members.iter().filter(|m| allowed(m).is_some()) {
        let Some(output) = cargo_tree(
            root,
            &["-p", member, "-e", "normal,build", "--prefix", "depth", "--target", "all", "--all-features"],
        ) else {
            return false;
        };
        graph.push((member.clone(), parse_tree(&output)));
    }

    let problems = check(&members, &graph);
    for problem in &problems {
        println!("{problem}");
    }
    if problems.is_empty() {
        println!("the dependency graph matches ARCHITECTURE.md ({} crates checked)", graph.len());
    }
    problems.is_empty()
}

fn cargo_tree(root: &Path, args: &[&str]) -> Option<String> {
    let output = match Command::new("cargo").arg("tree").arg("--locked").args(args).current_dir(root).output() {
        Ok(output) => output,
        Err(err) => {
            println!("could not start cargo tree: {err}");
            return None;
        }
    };
    if !output.status.success() {
        println!("cargo tree {} failed:\n{}", args.join(" "), String::from_utf8_lossy(&output.stderr));
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deps(direct: &[&str], indirect: &[&str]) -> Deps {
        let mut all: Vec<String> = direct.iter().chain(indirect).map(|s| (*s).to_owned()).collect();
        all.sort();
        all.dedup();
        Deps { direct: direct.iter().map(|s| (*s).to_owned()).collect(), all }
    }

    /// A graph that follows every rule.
    fn clean_graph() -> Vec<(String, Deps)> {
        vec![
            ("ananasar-core".to_owned(), deps(&[], &[])),
            ("ananasar-net".to_owned(), deps(&["ananasar-core", "socket2"], &["libc"])),
            ("ananasar-storm".to_owned(), deps(&["ananasar-core", "ananasar-net"], &["socket2"])),
            ("ananasar-gui".to_owned(), deps(&["ananasar-core", "ananasar-net", "eframe"], &["egui", "winit"])),
        ]
    }

    fn members(graph: &[(String, Deps)]) -> Vec<String> {
        graph.iter().map(|(name, _)| name.clone()).chain(["xtask".to_owned()]).collect()
    }

    fn set(graph: &mut [(String, Deps)], krate: &str, value: Deps) {
        let entry = graph.iter_mut().find(|(name, _)| name == krate).unwrap();
        entry.1 = value;
    }

    #[test]
    fn a_clean_graph_passes() {
        let graph = clean_graph();
        assert_eq!(check(&members(&graph), &graph), Vec::<String>::new());
    }

    #[test]
    fn net_depending_on_storm_fails() {
        let mut graph = clean_graph();
        set(&mut graph, "ananasar-net", deps(&["ananasar-core", "ananasar-storm", "socket2"], &[]));
        assert_eq!(
            check(&members(&graph), &graph),
            [
                "ananasar-net depends on ananasar-storm, which ARCHITECTURE.md does not allow (it may use: ananasar-core)"
            ]
        );
    }

    #[test]
    fn core_with_any_dependency_fails() {
        let mut graph = clean_graph();
        set(&mut graph, "ananasar-core", deps(&["ananasar-net", "bytes"], &[]));
        assert_eq!(
            check(&members(&graph), &graph),
            [
                "ananasar-core depends on ananasar-net, but it must depend on std only",
                "ananasar-core depends on bytes, but it must depend on std only",
                "ananasar-core depends on ananasar-net, which ARCHITECTURE.md does not allow (it may use: no other workspace crate)",
            ]
        );
    }

    #[test]
    fn a_headless_crate_pulling_in_egui_fails() {
        let mut graph = clean_graph();
        set(&mut graph, "ananasar-storm", deps(&["ananasar-core", "egui_plot"], &["egui"]));
        assert_eq!(
            check(&members(&graph), &graph),
            ["ananasar-storm pulls in egui, but it must build without a GUI toolkit"]
        );
    }

    #[test]
    fn a_new_crate_must_be_added_to_the_table() {
        let graph = clean_graph();
        let mut members = members(&graph);
        members.push("ananasar-rtu".to_owned());
        assert_eq!(
            check(&members, &graph),
            [
                "ananasar-rtu is not in the layer table: add it to ALLOWED in xtask/src/layers.rs and to the graph in ARCHITECTURE.md"
            ]
        );
    }

    #[test]
    fn parses_cargo_tree_depth_output() {
        let output = "\
0ananasar-gui v0.1.0 (/src/crates/ananasar-gui)
1ananasar-core v0.1.0 (/src/crates/ananasar-core)
1ananasar-dde v0.1.0 (/src/crates/ananasar-dde)
2ananasar-core v0.1.0 (/src/crates/ananasar-core)
2windows-sys v0.61.2
3windows-link v0.2.1
1eframe v0.37.0
2windows-sys v0.61.2 (*)

1cc v1.2.0
";
        assert_eq!(
            parse_tree(output),
            Deps {
                direct: ["ananasar-core", "ananasar-dde", "cc", "eframe"].map(str::to_owned).to_vec(),
                all: ["ananasar-core", "ananasar-dde", "cc", "eframe", "windows-link", "windows-sys"]
                    .map(str::to_owned)
                    .to_vec(),
            }
        );
    }

    /// The table must say what ARCHITECTURE.md's graph says, so the document
    /// and the check cannot drift apart.
    #[test]
    fn table_matches_architecture_md() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ARCHITECTURE.md");
        let text = std::fs::read_to_string(path).unwrap();
        let mut documented: Vec<(String, Vec<String>)> = text
            .lines()
            .filter(|line| line.starts_with("ananasar-"))
            .filter_map(|line| line.split_once("->"))
            .map(|(krate, targets)| {
                let krate = krate.split_whitespace().next().unwrap().to_owned();
                let mut targets: Vec<String> = if targets.trim() == "std only" {
                    Vec::new()
                } else {
                    targets
                        .split(',')
                        .map(|t| t.split_whitespace().next().unwrap().trim_end_matches('?'))
                        .map(|t| format!("ananasar-{t}"))
                        .collect()
                };
                targets.sort();
                (krate, targets)
            })
            .collect();
        documented.sort();

        let mut table: Vec<(String, Vec<String>)> = ALLOWED
            .iter()
            .map(|(krate, may_use)| {
                let mut may_use: Vec<String> = may_use.iter().map(|s| (*s).to_owned()).collect();
                may_use.sort();
                ((*krate).to_owned(), may_use)
            })
            .collect();
        table.sort();

        assert_eq!(documented, table);
    }
}
