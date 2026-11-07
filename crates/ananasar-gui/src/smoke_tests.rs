//! Headless smoke tests: each window opens, its main controls are found by
//! their accessible names and its main button can be clicked. No renderer is
//! used, and the embedded server listens on loopback only.
//!
//! The windows that are native viewports in the app are drawn as embedded
//! egui windows here, since the harness has no windowing backend, so each is
//! found by its title. The harness also fails a test when a widget it shows
//! has no accessible name.

use super::{App, theme};
use eframe::egui;
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::{Harness, Node};
use std::net::Ipv4Addr;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// The app with its server on a free loopback port, in a canvas large enough
/// that the dialogs need not cover the main window's controls.
fn harness() -> Harness<'static, App> {
    let mut harness = Harness::builder().with_size(egui::vec2(1600.0, 1200.0)).build_eframe(|cc| {
        theme::apply(&cc.egui_ctx, true);
        App::listening_on(&cc.egui_ctx, Some(0), Ipv4Addr::LOCALHOST.into())
    });
    harness.run();
    harness
}

/// The window titled `title`, to look up controls whose names repeat in other windows.
fn window<'a>(harness: &'a Harness<'_, App>, title: &'a str) -> Node<'a> {
    harness.get_by_role_and_label(Role::Window, title)
}

fn is_disabled(node: &Node<'_>) -> bool {
    node.accesskit_node().is_disabled()
}

/// Step the app until `done` holds, for worker threads that answer later.
fn wait_for(harness: &mut Harness<'_, App>, what: &str, mut done: impl FnMut(&App) -> bool) {
    let deadline = Instant::now().checked_add(Duration::from_secs(10)).expect("a deadline 10 s away");
    while !done(harness.state()) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        harness.step();
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Replace the text of the field named `label` in window `title` with `text`.
fn replace_text(harness: &mut Harness<'_, App>, title: &str, label: &str, text: &str) {
    window(harness, title).get_by_label(label).focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    window(harness, title).get_by_label(label).type_text(text);
    harness.run();
    assert_eq!(window(harness, title).get_by_label(label).value().as_deref(), Some(text));
}

#[test]
fn main_window_shows_its_controls_and_opens_the_client() {
    let mut harness = harness();
    let app = harness.state();
    assert!(app.server.listening.load(Ordering::SeqCst), "the server listens");
    assert!(app.message.is_none(), "no startup error: {:?}", app.message);

    for label in [
        "File",
        "Edit",
        "View",
        "Options",
        "Tools",
        "Input",
        "Holding",
        "Start Address",
        "UID",
        "Interpretive",
        "Start",
    ] {
        harness.get_by_label(label);
    }
    harness.get_by_label("Show").click();
    harness.run();

    assert!(harness.state().show_client);
    harness.get_by_label("Hide");
    window(&harness, "Modbus/TCP Client");
}

#[test]
fn client_connects_to_the_embedded_server() {
    const CLIENT: &str = "Modbus/TCP Client";
    const DIALOG: &str = "Server connection";
    let mut harness = harness();
    let port = harness.state().server_port;
    harness.state_mut().show_client = true;
    harness.run();

    let client = window(&harness, CLIENT);
    for label in [
        "Edit",
        "Modbus",
        "Disconnect",
        "Read address",
        "Write address",
        "Cyclic read/write",
        "Pause",
        "Cycle",
        "Delay",
    ] {
        client.get_by_label(label);
    }
    client.get_by_label("Connect\u{2026}").click();
    harness.run();

    // The Server connection dialog: point it at this app's own server.
    let dialog = window(&harness, DIALOG);
    let ip: Vec<_> =
        (1..=4).map(|i| dialog.get_by_label(&format!("IP part {i}")).value().unwrap_or_default()).collect();
    assert_eq!(ip, ["127", "0", "0", "1"]);
    dialog.get_by_label("Cancel");
    replace_text(&mut harness, DIALOG, "Remote port", &port.to_string());
    window(&harness, DIALOG).get_by_label("OK").click();
    harness.run();

    wait_for(&mut harness, "the client to connect", |app| app.client.is_connected());
    wait_for(&mut harness, "the server to see the client", |app| app.server.clients.load(Ordering::SeqCst) == 1);
}

#[test]
fn set_value_window_sets_a_register() {
    const TITLE: &str = "Set Register Value";
    let mut harness = harness();
    harness.state_mut().show_setval = true;
    harness.run();

    let setval = window(&harness, TITLE);
    for label in ["Edit", "Input", "Holding", "Register", "Close"] {
        setval.get_by_label(label);
    }
    setval.get_by_label("Set").click();
    harness.run();
    assert!(harness.state().show_setval, "Set keeps the window open");

    window(&harness, TITLE).get_by_label("Close").click();
    harness.run();
    assert!(!harness.state().show_setval);
}

#[test]
fn scanner_connects_to_the_embedded_server() {
    let mut harness = harness();
    harness.state_mut().show_scanner = true;
    harness.run();

    let scanner = window(&harness, "Register Scanner");
    for label in ["Close!", "Disconnect", "Start", "Stop"] {
        scanner.get_by_label(label);
    }
    scanner.get_by_label("Connect...").click();
    harness.run();

    // The scanner's default target is this app's own server.
    let connect = window(&harness, "Connect to server");
    for label in ["IP:", "Port:", "Cancel"] {
        connect.get_by_label(label);
    }
    connect.get_by_label("Connect").click();
    harness.run();

    wait_for(&mut harness, "the server to see the scanner", |app| app.server.clients.load(Ordering::SeqCst) == 1);
}

#[test]
fn storm_window_opens_from_the_menu_and_runs_against_the_embedded_server() {
    const TITLE: &str = "Storm Test";
    let mut harness = harness();
    let port = harness.state().server_port;
    harness.get_by_label("Tools").click();
    harness.run();
    harness.get_by_label("Storm Test\u{2026}").click();
    harness.run();
    assert!(harness.state().show_storm);

    let storm = window(&harness, TITLE);
    for label in ["Unit id:", "I am authorised to test this equipment", "Close!"] {
        storm.get_by_label(label);
    }
    replace_text(&mut harness, TITLE, "Target (host:port):", &format!("127.0.0.1:{port}"));
    replace_text(&mut harness, TITLE, "Duration (s):", "1");
    assert!(
        !is_disabled(&window(&harness, TITLE).get_by_label("Start")),
        "a short read-only run needs no authorisation"
    );
    assert!(is_disabled(&window(&harness, TITLE).get_by_label("Stop")));

    window(&harness, TITLE).get_by_label("Start").click();
    harness.run();
    assert!(harness.state().storm.running(), "the run did not start");
    assert!(!is_disabled(&window(&harness, TITLE).get_by_label("Stop")));

    wait_for(&mut harness, "the storm to finish", |app| !app.storm.running());
    harness.run();
    assert!(is_disabled(&window(&harness, TITLE).get_by_label("Stop")));
}

/// The DDE server is Windows only: elsewhere its menu entry is there but greyed out.
#[cfg(not(windows))]
#[test]
fn dde_server_entry_is_greyed_out() {
    let mut harness = harness();
    harness.get_by_label("Options").click();
    harness.run();
    assert!(is_disabled(&harness.get_by_label("DDE Server\u{2026}")));
}
