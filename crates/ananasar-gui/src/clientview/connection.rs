//! Connection bar and the Server connection dialog (IP octets, port, keepalives).

use super::widgets::{fixed_window, Canvas};
use super::worker::Cmd;
use super::ClientView;
use ananasar_core::sync::lock;
use eframe::egui;

/// The connection partner line: its IP and port, or dashes when not connected.
pub(super) fn partner_text(peer: Option<std::net::SocketAddr>) -> String {
    match peer {
        Some(a) => format!("Partner IP: {}  Port: {}", a.ip(), a.port()),
        None => "Partner IP: --  Port: --".into(),
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
    pub(super) fn connection_ui(&mut self, ui: &mut egui::Ui, connected: bool) {
        ui.horizontal(|ui| {
            ui.label(format!("Server: {}:{}", self.host, self.port));
            if ui.button("Server connection…").on_hover_text("Set the server's IP address, port and keepalives").clicked() {
                self.conn.octets = split_octets(&self.host).unwrap_or_else(|| ["127".into(), "0".into(), "0".into(), "1".into()]);
                self.conn.port = self.port.clone();
                self.conn.keepalive = self.keepalive;
                self.conn.open = true;
            }
            ui.label("UID:");
            ui.add(egui::TextEdit::singleline(&mut self.uid).desired_width(32.0));
            if !connected {
                if ui.button("Connect").clicked() {
                    let uid = self.uid.trim().parse().unwrap_or(1);
                    match ananasar_net::parse_endpoint(&self.host, &self.port) {
                        Ok((host, port)) => {
                            let _ = self.tx.send(Cmd::Connect { host, port, uid, keepalive: self.keepalive });
                            self.push_lists();
                            let _ = self.tx.send(Cmd::SetMode(self.mode));
                        }
                        Err(e) => self.shared.set_status(format!("Cannot connect: {e}")),
                    }
                }
            } else if ui.button("Disconnect").clicked() {
                let _ = self.tx.send(Cmd::Disconnect);
            }
        });
        ui.horizontal(|ui| {
            ui.label(partner_text(*lock(&self.shared.partner)));
            let status = lock(&self.shared.status).clone();
            if !status.is_empty() {
                ui.label(status);
            }
        });
        self.connection_dialog(ui.ctx(), connected);
    }

    fn connection_dialog(&mut self, ctx: &egui::Context, connected: bool) {
        if !self.conn.open {
            return;
        }
        let mut open = true;
        let (mut ok, mut cancel) = (false, false);
        fixed_window("Server connection").open(&mut open).show(ctx, |ui| {
            let c = Canvas::new(ui, 310.0, 159.0);
            let d = &mut self.conn;
            let valid_ip = join_octets(&d.octets).is_some();
            let valid_port = parse_port(&d.port).is_some();
            c.group(ui, (8.0, 8.0, 209.0, 49.0), "IP", |ui| {
                ui.add_enabled_ui(!connected, |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (i, octet) in d.octets.iter_mut().enumerate() {
                        if i > 0 {
                            ui.label(".");
                        }
                        let r = ui.add_sized([41.0, 20.0], egui::TextEdit::singleline(octet));
                        if r.changed() {
                            digits3(octet);
                        }
                        r.on_hover_text("Set communication partner's IP address");
                    }
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
            c.at(ui, (225.0, 20.0, 75.0, 34.0), |ui| {
                let ok_button = ui.add_sized([75.0, 34.0], egui::Button::new("OK"));
                if ok_button.on_hover_text("Apply settings and close window").clicked() && !connected && valid_ip && valid_port {
                    ok = true;
                }
            });
            c.at(ui, (225.0, 72.0, 75.0, 33.0), |ui| {
                if ui.add_sized([75.0, 33.0], egui::Button::new("Cancel")).on_hover_text("Close window").clicked() {
                    cancel = true;
                }
            });
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
        if ok && let Some(host) = join_octets(&self.conn.octets) {
            self.host = host;
            self.port = self.conn.port.trim().to_owned();
            self.keepalive = self.conn.keepalive;
        }
        self.conn.open = open && !ok && !cancel;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partner_text_shows_ip_and_port() {
        assert_eq!(partner_text(Some("192.168.1.7:1502".parse().unwrap())), "Partner IP: 192.168.1.7  Port: 1502");
        assert_eq!(partner_text(None), "Partner IP: --  Port: --");
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
