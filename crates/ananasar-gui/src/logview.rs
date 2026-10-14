//! The Data logging window: start and stop recording the server's Modbus
//! exchanges (raw or interpretive) to a log file, with the recording count and
//! file size shown while it runs.

use ananasar_core::log::LogMode;
use ananasar_net::LogSink;
use eframe::egui;
use std::path::Path;

/// Window state: where to write and how to decode.
pub struct LogView {
    dir: String,
    interpretive: bool,
    message: String,
}

impl Default for LogView {
    fn default() -> Self {
        Self { dir: ".".into(), interpretive: false, message: String::new() }
    }
}

impl LogView {
    pub fn ui(&mut self, ui: &mut egui::Ui, log: &LogSink, port: usize, listening: bool) {
        let recording = log.is_recording();
        ui.add_enabled_ui(!recording, |ui| {
            ui.horizontal(|ui| {
                ui.label("Folder:");
                ui.add(egui::TextEdit::singleline(&mut self.dir).desired_width(260.0));
            });
            ui.checkbox(&mut self.interpretive, "Interpretive log");
        });
        ui.horizontal(|ui| {
            if recording {
                if ui.button("Stop logging").clicked() {
                    log.stop();
                }
            } else if ui.add_enabled(listening, egui::Button::new("Start logging")).clicked() {
                let mode = if self.interpretive { LogMode::Interpretive } else { LogMode::Raw };
                let made = std::fs::create_dir_all(Path::new(&self.dir))
                    .and_then(|()| log.start(Path::new(&self.dir), "0.0.0.0", &port.to_string(), mode));
                self.message = match made {
                    Ok(p) => format!("Logging to {}", p.display()),
                    Err(e) => format!("Cannot start log: {e}"),
                };
            }
            if !listening && !recording {
                ui.label("The server is not listening.");
            }
        });
        if recording || log.recordings() > 0 {
            ui.label(format!("{} recordings, {} bytes", log.recordings(), log.bytes()));
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
    }
}
