//! Run the real `ananasar` binary: help, version, about, completions, and a
//! `server` answering `client` and `scan` on a local port.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: a panic is a failed test"
)]

use std::io::{BufRead, BufReader};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ananasar")).args(args).stdin(Stdio::null()).output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn help_and_version() {
    let help = run(&["--help"]);
    assert!(help.status.success());
    let text = stdout(&help);
    for sub in ["server", "client", "scan", "storm", "completions", "about"] {
        assert!(text.contains(sub), "--help lacks {sub}");
    }
    let version = run(&["--version"]);
    assert!(version.status.success());
    assert_eq!(stdout(&version).trim(), format!("ananasar {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn bare_invocation_and_bad_arguments_fail_with_an_error_on_stderr() {
    for args in [&[][..], &["frobnicate"], &["server", "-p", "70000"], &["client"]] {
        let o = run(args);
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert!(
            String::from_utf8_lossy(&o.stderr).contains("error:")
                || String::from_utf8_lossy(&o.stderr).contains("Usage"),
            "{args:?}"
        );
    }
}

#[test]
fn about_prints_version_and_sections() {
    let o = run(&["about"]);
    assert!(o.status.success());
    let text = stdout(&o);
    assert!(text.starts_with(&format!("Ananasar {}", env!("CARGO_PKG_VERSION"))));
    assert!(text.contains("== Dependencies ==") && text.contains("== Advisories"));
    assert!(!text.contains("== Licence texts =="));
    let deps = stdout(&run(&["about", "--deps"]));
    assert!(deps.contains("== Dependencies ==") && !deps.contains("== Advisories"));
    assert!(stdout(&run(&["about", "--licenses"])).contains("== Licence texts =="));
}

#[test]
fn completions_for_each_shell() {
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let o = run(&["completions", shell]);
        assert!(o.status.success(), "{shell}");
        assert!(stdout(&o).contains("ananasar"), "{shell}");
    }
    assert_eq!(run(&["completions", "tcsh"]).status.code(), Some(2));
}

/// A `server` child, killed when dropped.
struct Server {
    child: Child,
    port: u16,
    /// Kept open: with the read end closed the server's next `println!` would panic.
    _stdout: BufReader<std::process::ChildStdout>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

fn start_server(extra: &[&str]) -> Server {
    // The server prints no port when given 0, so pick a free one and hand it over.
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ananasar"))
        .args(["server", "-p", &port.to_string()])
        .args(extra)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut banner = String::new();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    stdout.read_line(&mut banner).unwrap();
    let server = Server { child, port, _stdout: stdout };
    assert!(banner.contains(&format!("listening on 0.0.0.0:{port}")), "banner: {banner:?}");
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "server never accepted a connection");
        std::thread::sleep(Duration::from_millis(20));
    }
    server
}

#[test]
fn client_polls_a_running_server() {
    let s = start_server(&[]);
    let target = format!("127.0.0.1:{}", s.port);
    let o = run(&["client", &target, "--read-holding", "0,1", "--write", "2", "-n", "3", "--interval-ms", "1"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let text = stdout(&o);
    assert!(text.contains(&format!("connected to {target}")));
    assert_eq!(text.matches("ok,").count(), 3, "{text}");
}

#[test]
fn client_without_anything_to_poll_exits_2() {
    let o = run(&["client", "127.0.0.1:1"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("nothing to poll"));
}

#[test]
fn client_and_scan_report_a_refused_connection() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let target = format!("127.0.0.1:{port}");
    for args in [vec!["client", &target, "--read-holding", "0"], vec!["scan", &target]] {
        let o = run(&args);
        assert_eq!(o.status.code(), Some(1), "{args:?}");
        assert!(String::from_utf8_lossy(&o.stderr).contains("failed"), "{args:?}");
    }
}

#[test]
fn scan_reports_every_register_in_the_range() {
    let s = start_server(&[]);
    let target = format!("127.0.0.1:{}", s.port);
    let o = run(&["scan", &target, "--start", "0", "--end", "4"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let text = stdout(&o);
    assert!(text.contains("5 registers scanned, 5 succeeded, 0 failed"), "{text}");
    let input = stdout(&run(&["scan", &target, "-b", "input", "--end", "1", "--only-ok"]));
    assert!(input.contains("2 registers scanned"), "{input}");
}

#[test]
fn server_refuses_a_port_already_in_use() {
    let held = TcpListener::bind("0.0.0.0:0").unwrap();
    let port = held.local_addr().unwrap().port().to_string();
    let o = run(&["server", "-p", &port]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("Socket initialisation failed"));
}

#[test]
fn server_fails_on_a_missing_load_file() {
    let o = run(&["server", "-p", "0", "--load", "/nonexistent/regs.ananas"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("cannot load"));
}
