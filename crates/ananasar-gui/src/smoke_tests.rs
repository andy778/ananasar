//! Headless smoke tests: each window opens, its main controls are found by
//! their accessible names and its main button can be clicked. No renderer is
//! used, and the embedded server listens on loopback only.
//!
//! The windows that are native viewports in the app are drawn as embedded
//! egui windows here, since the harness has no windowing backend, so each is
//! found by its title. The harness also fails a test when a widget it shows
//! has no accessible name.

use super::{App, theme};
use ananasar_core::registers::Bank;
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

/// Run the frames after a click that starts a worker thread (a connection or
/// a storm run). The worker asks for a repaint from its own thread each time
/// it reports progress, so `Harness::run` would only settle once the worker
/// goes quiet, after a number of frames that depends on thread timing (#192).
/// Two frames are enough: one handles the click, the next shows the new state.
/// Wait for the worker's results with [`wait_for`].
fn run_after_starting_work(harness: &mut Harness<'_, App>) {
    harness.run_steps(2);
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
    run_after_starting_work(&mut harness);

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
fn decode_window_shows_the_float_in_the_right_order() {
    const TITLE: &str = "Decode Registers";
    let mut harness = harness();
    {
        let mut regs = ananasar_core::sync::lock(&harness.state().regs);
        // 1.5f32 as ABCD at 0–1 and as CDAB at 10–11.
        regs.set_holding(0, 0x3FC0);
        regs.set_holding(11, 0x3FC0);
    }
    harness.state_mut().show_decode = true;
    harness.run();

    let decode = window(&harness, TITLE);
    for label in ["Input", "Holding", "Start address", "ABCD", "CDAB", "BADC", "DCBA"] {
        decode.query_all_by_label(label).next().unwrap_or_else(|| panic!("{label} is missing"));
    }
    // Only one cell reads 1.5: the ABCD f32 at 0–1, then the CDAB f32 at 10–11.
    decode.get_by_label("1.5");
    decode.get_by_label("Register 0: u16 16320   i16 16320   hex 0x3FC0");

    replace_text(&mut harness, TITLE, "Start address", "10");
    window(&harness, TITLE).get_by_label("1.5");
    window(&harness, TITLE).get_by_label("Register 10: u16 0   i16 0   hex 0x0000");
}

#[test]
fn sunspec_window_decodes_a_device_in_the_register_file() {
    const TITLE: &str = "SunSpec Device";
    let mut harness = harness();
    {
        let state = harness.state_mut();
        *ananasar_core::sync::lock(&state.regs) = crate::sunspecview::tests::device();
        let model = crate::sunspecview::parse_model(crate::sunspecview::tests::TEST_MODEL).unwrap();
        state.sunspec.models.insert(model.id, model);
        state.show_sunspec = true;
    }
    harness.run();

    let win = window(&harness, TITLE);
    for label in ["Models folder", "Browse…", "Load", "Holding", "Input"] {
        win.get_by_label(label);
    }
    win.get_by_label("SunS at 40000 (register 40001), 2 models");
    // The common model has no file loaded here; the test model is decoded.
    win.get_by_label("Model 1 at 40002, L=66: no model file loaded");
    win.get_by_label("Model 64900 · Test Model · at 40070, L=15");
    for value in ["452.0", "50.03", "MPPT (4)", "GROUND_FAULT, OVER_TEMP (0x9)", "ABC", "12.3"] {
        win.get_by_label(value);
    }
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
    run_after_starting_work(&mut harness);

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
    run_after_starting_work(&mut harness);
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

const CLIENT: &str = "Modbus/TCP Client";

/// Open the client window and connect it to the app's own server.
fn connected_client() -> Harness<'static, App> {
    const DIALOG: &str = "Server connection";
    let mut harness = harness();
    let port = harness.state().server_port;
    harness.state_mut().show_client = true;
    harness.run();
    window(&harness, CLIENT).get_by_label("Connect\u{2026}").click();
    harness.run();
    replace_text(&mut harness, DIALOG, "Remote port", &port.to_string());
    window(&harness, DIALOG).get_by_label("OK").click();
    run_after_starting_work(&mut harness);
    wait_for(&mut harness, "the client to connect", |app| app.client.is_connected());
    harness
}

/// Open the client window without connecting it.
fn unconnected_client() -> Harness<'static, App> {
    let mut harness = harness();
    harness.state_mut().show_client = true;
    harness.run();
    harness
}

fn set_holding(harness: &Harness<'_, App>, addr: u16, value: u16) {
    harness.state().regs.lock().expect("register lock").set_holding(addr, value);
}

/// Open a menu of the client window and then the entries below it, one per path element.
fn choose_in_client(harness: &mut Harness<'_, App>, path: &[&str]) {
    let (first, rest) = path.split_first().expect("a menu path");
    window(harness, CLIENT).get_by_label(first).click();
    harness.run();
    for label in rest {
        menu_item(harness, label).click();
        harness.run();
    }
}

/// The menu entry named `label`. Entries with a keyboard shortcut (such as `Ctrl+Alt+S`) have it appended to their accessible name.
fn menu_item<'a>(harness: &'a Harness<'_, App>, label: &'a str) -> Node<'a> {
    harness.get_by(move |node| {
        node.label().is_some_and(|l| l.strip_prefix(label).is_some_and(|rest| rest.is_empty() || rest.contains('+')))
    })
}

/// Open Modbus > `entry` and fill in the address.
fn open_write_dialog(harness: &mut Harness<'_, App>, entry: &str, title: &str, addr: u16) {
    choose_in_client(harness, &["Modbus", entry]);
    replace_text(harness, title, "Address", &addr.to_string());
}

fn click_apply(harness: &mut Harness<'_, App>, title: &str) {
    window(harness, title).get_by_label("Apply").click();
    run_after_starting_work(harness);
}

#[test]
fn manual_write_sets_a_register_and_closes() {
    const TITLE: &str = "Write single register";
    let mut harness = connected_client();
    open_write_dialog(&mut harness, "Write Single Register\u{2026}", TITLE, 7);
    replace_text(&mut harness, TITLE, "Value", "0x1234");
    assert!(!is_disabled(&window(&harness, TITLE).get_by_label("Use Function 16")));
    click_apply(&mut harness, TITLE);
    wait_for(&mut harness, "the register to change", |app| {
        app.regs.lock().expect("register lock").get(Bank::Holding, 7) == 0x1234
    });

    window(&harness, TITLE).get_by_label("Close").click();
    harness.run();
    assert!(harness.query_by_role_and_label(Role::Window, TITLE).is_none(), "the dialog is gone");
}

#[test]
fn manual_write_with_function_16_sets_a_register() {
    const TITLE: &str = "Write single register";
    let mut harness = connected_client();
    open_write_dialog(&mut harness, "Write Single Register\u{2026}", TITLE, 8);
    replace_text(&mut harness, TITLE, "Value", "4321");
    window(&harness, TITLE).get_by_label("Use Function 16").click();
    harness.run();
    click_apply(&mut harness, TITLE);
    wait_for(&mut harness, "the register to change", |app| {
        app.regs.lock().expect("register lock").get(Bank::Holding, 8) == 4321
    });
}

#[test]
fn manual_mask_write_combines_and_or() {
    const WRITE: &str = "Write single register";
    const MASK: &str = "Mask single register";
    let mut harness = connected_client();
    set_holding(&harness, 9, 0x1234);
    open_write_dialog(&mut harness, "Write Single Register\u{2026}", WRITE, 9);
    window(&harness, WRITE).get_by_label("Mask AND + OR").click();
    harness.run();
    window(&harness, MASK);
    assert!(is_disabled(&window(&harness, MASK).get_by_label("Use Function 16")));

    replace_text(&mut harness, MASK, "Value", "F0F0");
    replace_text(&mut harness, MASK, "OR (Hex)", "0A0A");
    click_apply(&mut harness, MASK);
    let (value, and, or) = (0x1234_u16, 0xF0F0_u16, 0x0A0A_u16);
    let expected = (value & and) | (or & !and);
    assert_ne!(expected, value);
    wait_for(&mut harness, "the register to change", |app| {
        app.regs.lock().expect("register lock").get(Bank::Holding, 9) == expected
    });
}

#[test]
fn mask_entry_of_the_modbus_menu_opens_the_mask_dialog() {
    let mut harness = unconnected_client();
    choose_in_client(&mut harness, &["Modbus", "Mask Write Register\u{2026}"]);
    window(&harness, "Mask single register");
}

#[test]
fn manual_write_cannot_be_applied_without_a_connection() {
    const TITLE: &str = "Write single register";
    let mut harness = unconnected_client();
    open_write_dialog(&mut harness, "Write Single Register\u{2026}", TITLE, 3);
    assert!(is_disabled(&window(&harness, TITLE).get_by_label("Apply")));
    assert!(is_disabled(&window(&harness, CLIENT).get_by_label("Disconnect")));
    // The exception status request needs a connection too.
    window(&harness, CLIENT).get_by_label("Modbus").click();
    harness.run();
    assert!(is_disabled(&menu_item(&harness, "Read Exception Status\u{2026}")));
}

/// Like [`replace_text`], for a field whose label is also shown as a separate text label.
fn replace_input(harness: &mut Harness<'_, App>, title: &str, label: &str, text: &str) {
    fn input<'a>(harness: &'a Harness<'_, App>, title: &'a str, label: &'a str) -> Node<'a> {
        window(harness, title).get_by_role_and_label(Role::TextInput, label)
    }
    input(harness, title, label).focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    input(harness, title, label).type_text(text);
    harness.run();
    assert_eq!(input(harness, title, label).value().as_deref(), Some(text));
}

/// The first button named `label` in the client window: the read row comes before the write row.
fn read_row_button<'a>(harness: &'a Harness<'_, App>, label: &'a str) -> Node<'a> {
    window(harness, CLIENT).get_all_by_role_and_label(Role::Button, label).next().expect("a button in the read row")
}

/// Whether the read list holds `addr`: the Remove button next to the
/// address box is only enabled for an address that is in the list.
fn read_list_holds(harness: &mut Harness<'_, App>, addr: u16) -> bool {
    replace_text(harness, CLIENT, "Read address", &addr.to_string());
    !is_disabled(&read_row_button(harness, "Remove"))
}

fn add_series(harness: &mut Harness<'_, App>, from: &str, to: &str, remove: bool) {
    choose_in_client(harness, &["Edit", "Add Serie", "Holding Registers", "Read"]);
    replace_input(harness, "Series", "From:", from);
    replace_input(harness, "Series", "To:", to);
    if remove {
        window(harness, "Series").get_by_label("Remove").click();
        harness.run();
    }
}

#[test]
fn add_and_remove_a_series_of_read_addresses() {
    let mut harness = unconnected_client();
    add_series(&mut harness, "10", "12", false);
    window(&harness, "Series").get_by_label("OK").click();
    harness.run();
    assert!(harness.query_by_role_and_label(Role::Window, "Series").is_none(), "OK closes the dialog");
    for addr in [10, 11, 12] {
        assert!(read_list_holds(&mut harness, addr), "{addr} is in the list");
    }
    for addr in [9, 13] {
        assert!(!read_list_holds(&mut harness, addr), "{addr} is not in the list");
    }

    add_series(&mut harness, "10", "12", true);
    window(&harness, "Series").get_by_label("OK").click();
    harness.run();
    for addr in [10, 11, 12] {
        assert!(!read_list_holds(&mut harness, addr), "{addr} was removed");
    }
}

#[test]
fn a_series_needs_from_not_above_to_and_numbers() {
    let mut harness = unconnected_client();
    choose_in_client(&mut harness, &["Edit", "Add Serie", "Input Registers"]);
    replace_input(&mut harness, "Series", "From:", "5");
    replace_input(&mut harness, "Series", "To:", "7");
    assert!(!is_disabled(&window(&harness, "Series").get_by_label("OK")));
    for (from, to) in [("8", "7"), ("x", "7"), ("5", "y"), ("-1", "7"), ("5", "65536")] {
        replace_input(&mut harness, "Series", "From:", from);
        replace_input(&mut harness, "Series", "To:", to);
        assert!(is_disabled(&window(&harness, "Series").get_by_label("OK")), "{from:?}..{to:?}");
    }
    window(&harness, "Series").get_by_label("Cancel").click();
    harness.run();
    assert!(harness.query_by_role_and_label(Role::Window, "Series").is_none());
}

#[test]
fn clear_empties_a_list_and_is_disabled_while_it_is_empty() {
    let mut harness = unconnected_client();
    choose_in_client(&mut harness, &["Edit", "Clear"]);
    assert!(is_disabled(&menu_item(&harness, "Input Registers")), "an empty list has nothing to clear");

    // Fill the input list with the Add button.
    harness.key_press(egui::Key::Escape);
    harness.run();
    window(&harness, CLIENT).get_by_label("Input registers").click();
    harness.run();
    replace_text(&mut harness, CLIENT, "Read address", "4");
    read_row_button(&harness, "Add").click();
    harness.run();
    assert!(!is_disabled(&read_row_button(&harness, "Remove")));

    choose_in_client(&mut harness, &["Edit", "Clear"]);
    assert!(!is_disabled(&menu_item(&harness, "Input Registers")));
    menu_item(&harness, "Input Registers").click();
    harness.run();
    assert!(!read_list_holds(&mut harness, 4), "the input list is empty");
    choose_in_client(&mut harness, &["Edit", "Clear"]);
    assert!(is_disabled(&menu_item(&harness, "Input Registers")));
}

/// The `n`th (0-based) button named `label` in the client window: the read row comes first, then the write row.
fn row_button<'a>(harness: &'a Harness<'_, App>, label: &'a str, n: usize) -> Node<'a> {
    window(harness, CLIENT).get_all_by_role_and_label(Role::Button, label).nth(n).expect("a row button")
}

/// Put `addr` in the list of address row `n` (0 = read, 1 = write) with its Add button.
fn add_address(harness: &mut Harness<'_, App>, n: usize, addr: u16) {
    let field = ["Read address", "Write address"][n];
    replace_text(harness, CLIENT, field, &addr.to_string());
    row_button(harness, "Add", n).click();
    harness.run();
}

/// Whether the Remove button of address row `n` is enabled for `addr`, that is, `addr` is in the list.
fn row_holds(harness: &mut Harness<'_, App>, n: usize, addr: u16) -> bool {
    let field = ["Read address", "Write address"][n];
    replace_text(harness, CLIENT, field, &addr.to_string());
    !is_disabled(&row_button(harness, "Remove", n))
}

#[test]
fn remove_takes_out_only_the_selected_address() {
    for n in [0, 1] {
        let mut harness = unconnected_client();
        for addr in [1, 2, 3] {
            add_address(&mut harness, n, addr);
        }
        replace_text(&mut harness, CLIENT, ["Read address", "Write address"][n], "2");
        row_button(&harness, "Remove", n).click();
        harness.run();
        assert!(row_holds(&mut harness, n, 1), "row {n}: 1 stays");
        assert!(!row_holds(&mut harness, n, 2), "row {n}: 2 was removed");
        assert!(row_holds(&mut harness, n, 3), "row {n}: 3 stays");
    }
}

#[test]
fn clear_works_for_the_holding_write_and_read_lists() {
    for (n, entry) in [(1, "Write"), (0, "Read")] {
        let mut harness = unconnected_client();
        choose_in_client(&mut harness, &["Edit", "Clear", "Holding Registers"]);
        assert!(is_disabled(&menu_item(&harness, entry)), "{entry}: an empty list has nothing to clear");
        harness.key_press(egui::Key::Escape);
        harness.run();

        add_address(&mut harness, n, 4);
        assert!(row_holds(&mut harness, n, 4));
        choose_in_client(&mut harness, &["Edit", "Clear", "Holding Registers"]);
        assert!(!is_disabled(&menu_item(&harness, entry)), "{entry}: a filled list can be cleared");
        menu_item(&harness, entry).click();
        harness.run();
        assert!(!row_holds(&mut harness, n, 4), "{entry}: cleared");
        choose_in_client(&mut harness, &["Edit", "Clear", "Holding Registers"]);
        assert!(is_disabled(&menu_item(&harness, entry)), "{entry}: empty again");
    }
}

#[test]
fn the_read_radio_shows_the_selected_choice() {
    let mut harness = unconnected_client();
    let checked = |harness: &Harness<'_, App>, label: &str| {
        window(harness, CLIENT).get_by_label(label).accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
    };
    assert!(checked(&harness, "Holding registers"));
    assert!(!checked(&harness, "Input registers"));
    window(&harness, CLIENT).get_by_label("Input registers").click();
    harness.run();
    assert!(checked(&harness, "Input registers"));
    assert!(!checked(&harness, "Holding registers"));
}

#[test]
fn a_write_address_reaches_the_worker() {
    let mut harness = connected_client();
    add_address(&mut harness, 1, 7);
    window(&harness, CLIENT).get_by_label("Cyclic read/write").click();
    run_after_starting_work(&mut harness);
    let deadline = Instant::now().checked_add(Duration::from_secs(10)).expect("a deadline");
    // A write poll is logged as "TI n: FC 0x06 ok" (or 0x10); a read poll would say "read @".
    while harness.query_by_label_contains(" ok").is_none() {
        assert!(Instant::now() < deadline, "the worker never wrote the address");
        harness.step();
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Send one wheel notch of `lines` (positive is wheel up) over the Registers
/// list, run several frames, and return the start address after each frame.
fn wheel_over_registers(harness: &mut Harness<'_, App>, lines: f32) -> Vec<u16> {
    // A point inside the list, a few rows below its header.
    let over_list = egui::pos2(60.0, 150.0);
    harness.input_mut().events.push(egui::Event::PointerMoved(over_list));
    harness.run_steps(2);
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, lines),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: egui::Modifiers::NONE,
    });
    let mut seen = Vec::new();
    for _ in 0..8 {
        harness.step();
        seen.push(harness.state().start_address);
    }
    seen
}

#[test]
fn wheel_down_over_registers_moves_a_few_registers_forward_once() {
    let mut harness = harness();
    harness.state_mut().start_address = 10;
    harness.run_steps(2);
    let seen = wheel_over_registers(&mut harness, -1.0);
    let last = *seen.last().expect("frames ran");
    assert!((11..=16).contains(&last), "one notch moved the list to {last}: {seen:?}");
    assert_eq!(seen.get(seen.len() - 3..), Some(&[last; 3][..]), "the list keeps moving: {seen:?}");
}

#[test]
fn wheel_up_over_registers_moves_a_few_registers_back_once() {
    let mut harness = harness();
    harness.state_mut().start_address = 100;
    harness.run_steps(2);
    let seen = wheel_over_registers(&mut harness, 1.0);
    let last = *seen.last().expect("frames ran");
    assert!((84..=99).contains(&last), "one notch moved the list to {last}: {seen:?}");
    assert_eq!(seen.get(seen.len() - 3..), Some(&[last; 3][..]), "the list keeps moving: {seen:?}");
}

#[test]
fn wheel_up_over_registers_stops_at_address_zero() {
    let mut harness = harness();
    harness.state_mut().start_address = 1;
    harness.run_steps(2);
    let seen = wheel_over_registers(&mut harness, 5.0);
    assert_eq!(seen.last(), Some(&0), "{seen:?}");
}
