//! Connection bar and the Server connection dialog (IP octets, port, keepalives).

use super::widgets::Canvas;
use crate::theme;
use super::worker::Cmd;
use super::ClientView;
use ananasar_core::sync::lock;
use eframe::egui;

/// The connection partner line: its IP and port, or dashes when not connected.
pub(super) fn partner_text(peer: Option<std::net::SocketAddr>) -> String {
    match peer {
        Some(a) => format!("IP: {}  Port: {}", a.ip(), a.port()),
        None => "IP: --  Port: --".into(),
    }
}

/// Splits a dotted IPv4 address into its four octet strings; `None` for anything else (e.g. a host name).
pub(super) fn split_octets(host: &str) -> Option<[String; 4]> {
    let ip: std::net::Ipv4Addr = host.trim().parse().ok()?;
    Some(ip.octets().map(|o| o.to_string()))
}

/// Joins four octet strings into a dotted address; `None` unless each is a number in 0..=255.
pub(super) fn join_octets(octets: &[String; 4]) -> Option<String> {
    let mut parts = [0u8; 4];
    for (part, text) in parts.iter_mut().zip(octets) {
        *part = text.trim().parse().ok()?;
    }
    Some(std::net::Ipv4Addr::from(parts).to_string())
}

/// Parses the dialog's port field; port 0 is not a usable remote port.
fn parse_port(text: &str) -> Option<u16> {
    text.trim().parse::<u16>().ok().filter(|&p| p != 0)
}

/// Keeps digits only, at most three of them.
fn digits3(text: &mut String) {
    text.retain(|c| c.is_ascii_digit());
    text.truncate(3);
}

/// State of the Server connection dialog; the edits are drafts until OK.
pub(super) struct ConnDialog {
    pub(super) open: bool,
    octets: [String; 4],
    port: String,
    keepalive: bool,
}

impl ConnDialog {
    pub(super) fn new() -> Self {
        Self { open: false, octets: Default::default(), port: String::new(), keepalive: false }
    }
}

impl ClientView {
    /// Opens the Server connection dialog with the current settings as its draft.
    fn open_connection_dialog(&mut self) {
        self.conn.octets = split_octets(&self.host).unwrap_or_else(|| ["127".into(), "0".into(), "0".into(), "1".into()]);
        self.conn.port = self.port.clone();
        self.conn.keepalive = self.keepalive;
        self.conn.open = true;
    }

    /// Connects to the configured server with the chosen UID and sends the current settings to the worker.
    fn connect(&mut self) {
        let uid = u8::try_from(self.uid).unwrap_or(u8::MAX);
        match ananasar_net::parse_endpoint(&self.host, &self.port) {
            Ok((host, port)) => {
                let _ = self.tx.send(Cmd::Connect { host, port, uid, keepalive: self.keepalive });
                self.push_lists();
                let _ = self.tx.send(Cmd::SetMode(self.mode));
                // A new connection starts a fresh engine, so send the chosen settings again.
                let _ = self.tx.send(Cmd::SetLimits { read: self.max_read, write: self.max_write });
                let _ = self.tx.send(Cmd::SetInterval(self.interval_ms));
                let _ = self.tx.send(Cmd::SetBase(if self.base_one { ananasar_core::client::AddressBase::One } else { ananasar_core::client::AddressBase::Zero }));
            }
            Err(e) => self.shared.set_status(format!("Cannot connect: {e}")),
        }
    }

    pub(super) fn connection_ui(&mut self, ui: &mut egui::Ui, c: &Canvas, connected: bool) {
        c.frame(ui, (7.0, 12.0, 208.0, 46.0), "Connection");
        let connect = theme::bevel_button(ui, c.rect((21.0, 26.0, 74.0, 24.0)), "connect", "Connect\u{2026}", !connected, None);
        if connect.on_hover_text(format!("Server {}:{}", self.host, self.port)).clicked() {
            self.open_connection_dialog();
        }
        if theme::bevel_button(ui, c.rect((125.0, 26.0, 76.0, 24.0)), "disconnect", "Disconnect", connected, None).clicked() {
            let _ = self.tx.send(Cmd::Disconnect);
        }
        c.frame(ui, (221.0, 12.0, 72.0, 46.0), "UID");
        theme::combo_u16(ui, c.rect((229.0, 28.0, 56.0, 22.0)), "client_uid", &mut self.uid, 255, 1, 256, false);
        c.frame(ui, (7.0, 68.0, 286.0, 38.0), "Connection partner");
        let text = partner_text(*lock(&self.shared.partner));
        ui.painter().text(c.rect((15.0, 90.0, 0.0, 0.0)).min, egui::Align2::LEFT_CENTER, text, egui::FontId::monospace(13.0), egui::Color32::BLACK);
        self.connection_dialog(ui.ctx(), connected);
    }

    fn connection_dialog(&mut self, ctx: &egui::Context, connected: bool) {
        if !self.conn.open {
            return;
        }
        let (mut ok, mut cancel) = (false, false);
        // A window of its own, sized like the original's dialog.
        let builder = egui::ViewportBuilder::default()
            .with_title("Server connection")
            .with_inner_size([310.0, 159.0])
            .with_resizable(false)
            .with_minimize_button(false)
            .with_maximize_button(false);
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("server_connection"), builder, |ctx, _class| {
          egui::CentralPanel::default().frame(egui::Frame::new().fill(ctx.global_style().visuals.panel_fill)).show(ctx, |ui| {
            let c = Canvas::new(ui, 310.0, 159.0);
            let d = &mut self.conn;
            let valid_ip = join_octets(&d.octets).is_some();
            let valid_port = parse_port(&d.port).is_some();
            c.group(ui, (8.0, 8.0, 209.0, 49.0), "IP", |ui| {
                ui.add_enabled_ui(!connected, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        for octet in &mut d.octets {
                            let r = ui.add_sized([41.0, 20.0], egui::TextEdit::singleline(octet));
                            if r.changed() {
                                digits3(octet);
                            }
                            r.on_hover_text("Set communication partner's IP address");
                        }
                    });
                });
            });
            c.group(ui, (7.0, 64.0, 66.0, 49.0), "Port", |ui| {
                let r = ui.add_enabled(!connected, egui::TextEdit::singleline(&mut d.port).desired_width(50.0));
                r.on_hover_text("Set remote port for connection");
            });
            c.at(ui, (8.0, 120.0, 200.0, 20.0), |ui| {
                ui.add_enabled(!connected, egui::Checkbox::new(&mut d.keepalive, "Send Keepalives to server"))
                    .on_hover_text("Send keepalives to communication partner");
            });
            let ok_button = theme::bevel_button(ui, c.rect((225.0, 20.0, 75.0, 34.0)), "conn_ok", "OK", !connected && valid_ip && valid_port, None);
            ok |= ok_button.on_hover_text("Apply settings and close window").clicked();
            cancel |= theme::bevel_button(ui, c.rect((225.0, 72.0, 75.0, 33.0)), "conn_cancel", "Cancel", true, None).on_hover_text("Close window").clicked();
            let status = if connected {
                "Disconnect to change the connection"
            } else if !valid_ip {
                "Each IP part must be 0-255"
            } else if !valid_port {
                "Port must be 1-65535"
            } else {
                ""
            };
            c.at(ui, (0.0, 140.0, 310.0, 19.0), |ui| {
                ui.label(status);
            });
          });
          if ctx.input(|i| i.viewport().close_requested()) {
              cancel = true;
          }
        });
        if ok && let Some(host) = join_octets(&self.conn.octets) {
            self.host = host;
            self.port = self.conn.port.trim().to_owned();
            self.keepalive = self.conn.keepalive;
            // OK connects, since Connect… only opens this dialog.
            self.connect();
        }
        self.conn.open = !ok && !cancel;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partner_text_shows_ip_and_port() {
        assert_eq!(partner_text(Some("192.168.1.7:1502".parse().unwrap())), "IP: 192.168.1.7  Port: 1502");
        assert_eq!(partner_text(None), "IP: --  Port: --");
    }

    #[test]
    fn octets_round_trip_and_validate() {
        let o = split_octets("192.168.1.7").unwrap();
        assert_eq!(join_octets(&o).as_deref(), Some("192.168.1.7"));
        assert!(split_octets("localhost").is_none());
        let bad = ["10".to_string(), "0".to_string(), "256".to_string(), "1".to_string()];
        assert!(join_octets(&bad).is_none());
        let empty = ["10".to_string(), String::new(), "0".to_string(), "1".to_string()];
        assert!(join_octets(&empty).is_none());
    }

    #[test]
    fn port_rejects_zero_and_overflow() {
        assert_eq!(parse_port(" 502 "), Some(502));
        assert_eq!(parse_port("0"), None);
        assert_eq!(parse_port("65536"), None);
    }
}
