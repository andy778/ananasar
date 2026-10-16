//! The menu bar as data: a pure function builds the tree from the app state,
//! and a small renderer draws it with egui and reports which action was chosen.
//! Keeping the tree free of egui drawing is what lets a test compare it with
//! the original program's menus without a display.

use crate::theme;
use ananasar_core::protocol::Exception;
use ananasar_core::registers::{Bank, InitMode};
use ananasar_core::server::ServerConfig;
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use std::collections::BTreeSet;

/// The keyboard shortcuts of the menu items (the same constants drive the key handling).
pub mod sc {
    use super::{Key, KeyboardShortcut, Modifiers};

    const CMD_ALT: Modifiers = Modifiers { alt: true, ..Modifiers::COMMAND };
    pub const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
    pub const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
    pub const EXIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::ALT, Key::F4);
    pub const VL_EDIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::E);
    pub const VL_MONITOR: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::M);
    pub const VL_COPY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::C);
    pub const SET_REGISTER: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::R);
    pub const HEX: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::H);
    pub const ADDR_HEX: KeyboardShortcut = KeyboardShortcut::new(Modifiers { shift: true, ..Modifiers::COMMAND }, Key::H);
    pub const ADDRESS_OFFSET: KeyboardShortcut = KeyboardShortcut::new(CMD_ALT, Key::O);
    pub const OUT_OF_SYNCH: KeyboardShortcut = KeyboardShortcut::new(CMD_ALT, Key::S);
    pub const SCANNER: KeyboardShortcut = KeyboardShortcut::new(CMD_ALT, Key::C);
    pub const LISTEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::P);
    pub const CHANGE_PORT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
    pub const TOOLTIPS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::T);
}

/// Function codes of Options > Modbus > Active MB Functions, with their captions.
pub const FUNCTION_TOGGLES: [(u8, &str); 8] = [
    (0x03, "FC3"),
    (0x04, "FC4"),
    (0x06, "FC6"),
    (0x07, "FC7"),
    (0x08, "FC8"),
    (0x10, "FC16"),
    (0x16, "FC22"),
    (0x17, "FC23"),
];

/// Entries of Options > Modbus > Return Exception. There is no entry for code 9.
pub const EXCEPTION_CHOICES: [(Exception, &str); 10] = [
    (Exception::IllegalFunction, "Illegal Function"),
    (Exception::IllegalDataAddress, "Illegal Data Address"),
    (Exception::IllegalDataValue, "Illegal Data Value"),
    (Exception::SlaveDeviceFailure, "Slave Device Failure"),
    (Exception::Acknowledge, "Acknowledge"),
    (Exception::SlaveDeviceBusy, "Slave Device Busy"),
    (Exception::NegativeAcknowledge, "Negative Acknowledge"),
    (Exception::MemoryParityError, "Memory Parity Error"),
    (Exception::GatewayPathUnavailable, "Gateway Path Unavailable"),
    (Exception::GatewayTargetFailed, "Gateway Target Device Failed to Respond"),
];

/// What choosing a menu entry does; the app dispatches it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Open,
    SaveAs,
    Exit,
    About,
    ValueListEdit,
    ValueListMonitor,
    CopyList,
    SetRegister,
    Initialize(Bank, InitMode),
    AddrHex,
    Hex,
    ToggleFunction(u8),
    ForceException(Exception),
    ToggleFault(Fault),
    Scanner,
    MonitorPort,
    ChangePort,
    /// Only on Windows, where the DDE server exists.
    #[cfg(windows)]
    DdeServer,
    Tooltips,
}

impl Action {
    /// Whether the menu closes after the entry is chosen (toggles that are
    /// meant to be clicked repeatedly leave it open).
    fn closes_menu(self) -> bool {
        !matches!(
            self,
            Self::Exit | Self::AddrHex | Self::Hex | Self::ToggleFunction(_) | Self::ForceException(_) | Self::ToggleFault(_) | Self::Tooltips
        )
    }
}

/// The on/off server options of Options > Modbus besides the function codes and the forced exception.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    TransactionId,
    ProtocolId,
    LengthHigh,
    AddressOffset,
    OutOfSynch,
}

impl Fault {
    /// The switch in the server configuration that belongs to this option.
    pub fn flag(self, cfg: &mut ServerConfig) -> &mut bool {
        match self {
            Self::TransactionId => &mut cfg.wrong_transaction_id,
            Self::ProtocolId => &mut cfg.wrong_protocol_id,
            Self::LengthHigh => &mut cfg.wrong_length_high,
            Self::AddressOffset => &mut cfg.address_offset,
            Self::OutOfSynch => &mut cfg.out_of_synch,
        }
    }

    /// Whether the option is on in `cfg`.
    pub fn is_on(self, cfg: &ServerConfig) -> bool {
        match self {
            Self::TransactionId => cfg.wrong_transaction_id,
            Self::ProtocolId => cfg.wrong_protocol_id,
            Self::LengthHigh => cfg.wrong_length_high,
            Self::AddressOffset => cfg.address_offset,
            Self::OutOfSynch => cfg.out_of_synch,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Action,
    Check,
    Submenu(Vec<MenuItem>),
    Separator,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuItem {
    pub caption: String,
    pub shortcut: Option<KeyboardShortcut>,
    pub kind: Kind,
    pub checked: bool,
    pub enabled: bool,
    /// What choosing the item does; none for submenus, separators and greyed-out entries.
    pub action: Option<Action>,
    /// Minimum width of the dropdown of a submenu.
    pub min_width: Option<f32>,
}

impl MenuItem {
    fn new(caption: &str, kind: Kind) -> Self {
        Self { caption: caption.to_owned(), shortcut: None, kind, checked: false, enabled: true, action: None, min_width: None }
    }
    fn action(caption: &str, action: Action) -> Self {
        Self { action: Some(action), ..Self::new(caption, Kind::Action) }
    }
    fn check(caption: &str, checked: bool, action: Action) -> Self {
        Self { checked, action: Some(action), ..Self::new(caption, Kind::Check) }
    }
    /// An entry of the original that has no counterpart in the engine yet: shown greyed out.
    fn off(caption: &str) -> Self {
        Self { enabled: false, ..Self::new(caption, Kind::Action) }
    }
    fn submenu(caption: &str, items: Vec<Self>) -> Self {
        Self::new(caption, Kind::Submenu(items))
    }
    fn separator() -> Self {
        Self::new("", Kind::Separator)
    }
    fn key(mut self, shortcut: KeyboardShortcut) -> Self {
        self.shortcut = Some(shortcut);
        self
    }
    fn width(mut self, width: f32) -> Self {
        self.min_width = Some(width);
        self
    }
}

/// The parts of the app state that decide the menus' check marks and captions.
pub struct MenuState<'a> {
    pub value_list_edit: bool,
    pub addr_hex: bool,
    pub hex: bool,
    pub listening: bool,
    pub port: u16,
    pub tooltips: bool,
    pub supported: &'a BTreeSet<u8>,
    pub forced_exception: Option<Exception>,
    pub faults: &'a ServerConfig,
}

/// The menu bar: its top-level menus in order.
pub fn build(s: &MenuState<'_>) -> Vec<MenuItem> {
    use MenuItem as M;
    let file = vec![
        M::action("Open…", Action::Open).key(sc::OPEN),
        M::action("Save As…", Action::SaveAs).key(sc::SAVE),
        M::separator(),
        M::action("Exit", Action::Exit).key(sc::EXIT),
        M::separator(),
        M::action("About…", Action::About),
    ];
    let value_list = vec![
        M::check("Edit", s.value_list_edit, Action::ValueListEdit).key(sc::VL_EDIT),
        M::check("Monitor", !s.value_list_edit, Action::ValueListMonitor).key(sc::VL_MONITOR),
        M::separator(),
        M::action("Copy", Action::CopyList).key(sc::VL_COPY),
    ];
    let initialize = [("Input Registers", Bank::Input), ("Holding Registers", Bank::Holding)]
        .into_iter()
        .map(|(name, bank)| {
            M::submenu(
                name,
                vec![M::action("All to '0'", Action::Initialize(bank, InitMode::Zero)), M::action("Ascending", Action::Initialize(bank, InitMode::Ascending))],
            )
        })
        .collect();
    let edit = vec![
        M::submenu("Value List", value_list).width(190.0),
        M::separator(),
        M::action("Set Register Value", Action::SetRegister).key(sc::SET_REGISTER),
        M::separator(),
        M::submenu("Initialize", initialize),
    ];
    let view = vec![
        M::check("Registers as Hex", s.addr_hex, Action::AddrHex).key(sc::ADDR_HEX),
        M::check("Values as Hex", s.hex, Action::Hex).key(sc::HEX),
    ];
    let functions = FUNCTION_TOGGLES.iter().map(|&(fc, name)| M::check(name, s.supported.contains(&fc), Action::ToggleFunction(fc))).collect();
    let exceptions = EXCEPTION_CHOICES.iter().map(|&(exc, name)| M::check(name, s.forced_exception == Some(exc), Action::ForceException(exc))).collect();
    let modbus = vec![
        M::submenu("Active MB Functions", functions),
        M::separator(),
        M::check("Return Incorrect TI", Fault::TransactionId.is_on(s.faults), Action::ToggleFault(Fault::TransactionId)),
        M::check("Return Incorrect Protocol ID", Fault::ProtocolId.is_on(s.faults), Action::ToggleFault(Fault::ProtocolId)),
        M::check("Return Incorrect Length High Byte", Fault::LengthHigh.is_on(s.faults), Action::ToggleFault(Fault::LengthHigh)),
        M::submenu("Return Exception", exceptions),
        M::separator(),
        M::check("Use Address Offset", Fault::AddressOffset.is_on(s.faults), Action::ToggleFault(Fault::AddressOffset)).key(sc::ADDRESS_OFFSET),
        M::separator(),
        M::check("Reply Out of Synch", Fault::OutOfSynch.is_on(s.faults), Action::ToggleFault(Fault::OutOfSynch)).key(sc::OUT_OF_SYNCH),
        M::separator(),
        M::action("Register Scanner…", Action::Scanner).key(sc::SCANNER),
    ];
    let port = vec![
        M::check(&format!("Monitor Port {}", s.port), s.listening, Action::MonitorPort).key(sc::LISTEN),
        M::action("Change Port…", Action::ChangePort).key(sc::CHANGE_PORT),
        M::separator(),
        M::off("Purge Port Usage Blocking(s)"),
    ];
    // The original enables the Nagle entries per connected client; the server
    // does not switch TCP_NODELAY on a live connection yet.
    let nagle = vec![M::off("Connection #1"), M::off("Connection #2")];
    let tcp = vec![
        M::submenu("Port", port).width(260.0),
        M::submenu("Disable Nagle", nagle),
        M::separator(),
        // The server always drops a connection silent for a minute.
        M { checked: true, enabled: false, ..M::new("Release Connection if Unused >1 min", Kind::Check) },
    ];
    let options = vec![
        M::submenu("Modbus", modbus).width(260.0),
        M::submenu("TCP", tcp).width(260.0),
        M::separator(),
        // The DDE server is Windows only; elsewhere the entry stays greyed out.
        #[cfg(windows)]
        M::action("DDE Server…", Action::DdeServer),
        #[cfg(not(windows))]
        M::off("DDE Server…"),
        M::separator(),
        M::check("Show Tooltips", s.tooltips, Action::Tooltips).key(sc::TOOLTIPS),
    ];
    vec![
        M::submenu("File", file).width(190.0),
        M::submenu("Edit", edit).width(190.0),
        M::submenu("View", view).width(190.0),
        M::submenu("Options", options).width(190.0),
    ]
}

/// Draw `items` into `ui` (the menu bar or the inside of a menu); returns the chosen action, if any.
pub fn show(ui: &mut egui::Ui, items: &[MenuItem]) -> Option<Action> {
    let mut chosen = None;
    for item in items {
        match &item.kind {
            Kind::Separator => {
                ui.separator();
            }
            Kind::Submenu(children) => {
                ui.menu_button(&item.caption, |ui| {
                    if let Some(w) = item.min_width {
                        ui.set_min_width(w);
                    }
                    if let Some(action) = show(ui, children) {
                        chosen = Some(action);
                    }
                });
            }
            Kind::Action | Kind::Check => {
                let shortcut = item.shortcut.map(|sc| ui.ctx().format_shortcut(&sc));
                let clicked = if item.kind == Kind::Check {
                    theme::check_item(ui, item.checked, item.enabled, &item.caption, shortcut)
                } else {
                    let mut button = egui::Button::new(&item.caption);
                    if let Some(text) = shortcut {
                        button = button.shortcut_text(text);
                    }
                    ui.add_enabled(item.enabled, button).clicked()
                };
                if let (true, Some(action)) = (clicked, item.action) {
                    chosen = Some(action);
                    if action.closes_menu() {
                        ui.close();
                    }
                }
            }
        }
    }
    chosen
}

#[cfg(test)]
mod spec_tests;
