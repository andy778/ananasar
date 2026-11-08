//! Repository tasks, run through the `cargo xtask` alias in `.cargo/config.toml`.
//!
//! `cargo xtask ci` runs the checks CI runs, so contributors and agents can run
//! them before pushing. A step whose toolchain or tool is not installed is
//! skipped with a message saying how to install it, and the summary at the end
//! lists what passed, failed and was skipped. Every step runs even after one
//! fails, so one run shows everything that needs fixing.

mod layers;

use std::env;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

/// One check: what it runs and, if it needs a tool beyond stable cargo, a
/// probe command that must succeed for the step to run.
struct Step {
    name: &'static str,
    needs: Option<Probe>,
    action: Action,
}

enum Action {
    /// External commands, run in order until one fails.
    Commands(&'static [&'static [&'static str]]),
    /// A check that runs inside xtask, given the workspace root.
    Check(fn(&Path) -> bool),
}

struct Probe {
    command: &'static [&'static str],
    /// Shown when the probe fails: what is missing and how to install it.
    missing: &'static str,
}

enum Outcome {
    Passed,
    Failed,
    Skipped(&'static str),
}

/// The checks in `.github/workflows/build.yml` (`test`, which includes the layer
/// check, `msrv` and `deny`) plus `cargo fmt --check`. Not covered: fuzzing, the Windows and macOS builds,
/// CodeQL and the version stamping check.
const STEPS: &[Step] = &[
    Step {
        name: "format",
        needs: Some(Probe {
            command: &["cargo", "fmt", "--version"],
            missing: "rustfmt is not installed (rustup component add rustfmt)",
        }),
        action: Action::Commands(&[&["cargo", "fmt", "--all", "--check"]]),
    },
    Step {
        name: "clippy",
        needs: None,
        action: Action::Commands(&[&[
            "cargo",
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ]]),
    },
    Step { name: "test", needs: None, action: Action::Commands(&[&["cargo", "test", "--workspace", "--locked"]]) },
    Step { name: "layers", needs: None, action: Action::Check(layers::run) },
    Step {
        // The GUI needs a newer Rust, so these are the crates and features
        // that must still build on the minimum supported version.
        name: "rust 1.92",
        needs: Some(Probe {
            command: &["cargo", "+1.92", "--version"],
            missing: "the Rust 1.92 toolchain is not installed (rustup toolchain install 1.92)",
        }),
        action: Action::Commands(&[
            &[
                "cargo",
                "+1.92",
                "test",
                "--locked",
                "-p",
                "ananasar-core",
                "-p",
                "ananasar-net",
                "-p",
                "ananasar-storm",
                "-p",
                "ananasar-mcp",
            ],
            &["cargo", "+1.92", "test", "--locked", "-p", "ananasar-cli", "--no-default-features"],
            &["cargo", "+1.92", "test", "--locked", "-p", "ananasar-cli", "--no-default-features", "--features", "mcp"],
        ]),
    },
    Step {
        name: "cargo-deny",
        needs: Some(Probe {
            command: &["cargo", "deny", "--version"],
            missing: "cargo-deny is not installed (cargo install --locked cargo-deny)",
        }),
        action: Action::Commands(&[&["cargo", "deny", "check", "advisories", "bans", "sources", "licenses"]]),
    },
    Step {
        name: "release script tests",
        needs: Some(Probe { command: &["python3", "--version"], missing: "python3 is not on PATH" }),
        action: Action::Commands(&[&["python3", "-m", "unittest", "discover", "-s", "scripts"]]),
    },
];

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match (args.next().as_deref(), args.next()) {
        (Some("ci"), None) => ci(),
        (Some("layers"), None) => {
            if layers::run(workspace_root()) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        _ => {
            eprintln!("usage: cargo xtask ci | layers");
            eprintln!();
            eprintln!("  ci      run the checks CI runs: fmt, clippy, tests, the layer check,");
            eprintln!("          the Rust 1.92 builds, cargo-deny and the release script tests");
            eprintln!("  layers  check the crate dependency graph against ARCHITECTURE.md");
            ExitCode::from(2)
        }
    }
}

/// The alias works from any directory in the repository; the commands
/// (`-s scripts` in particular) expect the workspace root.
fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap_or_else(|| Path::new("."))
}

fn ci() -> ExitCode {
    let root = workspace_root();

    let outcomes: Vec<(&str, Outcome)> = STEPS.iter().map(|step| (step.name, run_step(step, root))).collect();

    println!();
    println!("summary:");
    let mut failed = false;
    for (name, outcome) in &outcomes {
        match outcome {
            Outcome::Passed => println!("  passed   {name}"),
            Outcome::Failed => {
                failed = true;
                println!("  FAILED   {name}");
            }
            Outcome::Skipped(why) => println!("  skipped  {name}: {why}"),
        }
    }
    println!("  not run  fuzzing (cargo +nightly fuzz run frame), Windows and macOS builds, CodeQL");

    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

fn run_step(step: &Step, root: &Path) -> Outcome {
    println!();
    println!("==> {}", step.name);
    if let Some(probe) = &step.needs
        && !probe_succeeds(probe.command, root)
    {
        println!("skipped: {}", probe.missing);
        return Outcome::Skipped(probe.missing);
    }
    match step.action {
        Action::Commands(commands) => {
            for command in commands {
                println!("$ {}", command.join(" "));
                if !run(command, root) {
                    return Outcome::Failed;
                }
            }
        }
        Action::Check(check) => {
            if !check(root) {
                return Outcome::Failed;
            }
        }
    }
    Outcome::Passed
}

fn probe_succeeds(command: &[&str], root: &Path) -> bool {
    let Some((program, args)) = command.split_first() else {
        return false;
    };
    Command::new(program)
        .args(args)
        .current_dir(root)
        // Probing must not make rustup download a missing toolchain.
        .env("RUSTUP_AUTO_INSTALL", "0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn run(command: &[&str], root: &Path) -> bool {
    let Some((program, args)) = command.split_first() else {
        return false;
    };
    match Command::new(program).args(args).current_dir(root).status() {
        Ok(status) => status.success(),
        Err(err) => {
            println!("could not start {program}: {err}");
            false
        }
    }
}
