//! Connection bar: endpoint, unit id, connect/disconnect, keepalive and the partner line.

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

impl ClientView {
    pub(super) fn connection_ui(&mut self, ui: &mut egui::Ui, connected: bool) {
        ui.horizontal(|ui| {
            ui.label("IP:");
            ui.add_enabled(!connected, egui::TextEdit::singleline(&mut self.host).desired_width(130.0));
            ui.label("Port:");
            ui.add_enabled(!connected, egui::TextEdit::singleline(&mut self.port).desired_width(45.0));
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
            ui.add_enabled(!connected, egui::Checkbox::new(&mut self.keepalive, "Send keepalives"));
            ui.separator();
            ui.label(partner_text(*lock(&self.shared.partner)));
            let status = lock(&self.shared.status).clone();
            if !status.is_empty() {
                ui.label(status);
            }
        });
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
}
