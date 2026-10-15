//! Renders the main window headlessly to `assets/screenshot.png` (the README
//! image). Ignored by default; see docs/screenshots.md for how to run it.

use super::{App, theme};
use ananasar_core::registers::Bank;
use ananasar_core::sync::lock;
use eframe::egui;
use egui_kittest::Harness;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// A register value that looks like live process data, for a few screens of rows.
fn sample_value(addr: u16) -> u16 {
    let n = addr.wrapping_mul(7);
    match addr % 8 {
        0 => 230u16.wrapping_add(addr.wrapping_mul(3)),
        1 => 1500u16.wrapping_add(addr.wrapping_mul(17)),
        2 => addr.wrapping_mul(100).wrapping_add(7),
        3 => 0xA5A5 ^ addr.wrapping_mul(257),
        4 => 4096u16.wrapping_sub(addr.wrapping_mul(31)),
        5 => 21u16.wrapping_add(addr % 13),
        6 => 65535u16.wrapping_sub(addr.wrapping_mul(1021)),
        _ => n.wrapping_add(addr),
    }
}

/// Polls the server like a Modbus master: function 3, ten registers from 0,
/// every 250 ms, until `stop` is set.
fn poll(port: u16, stop: &AtomicBool) -> std::io::Result<()> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut tid: u16 = 0x0100;
    while !stop.load(Ordering::SeqCst) {
        tid = tid.wrapping_add(1);
        let [t_hi, t_lo] = tid.to_be_bytes();
        stream.write_all(&[t_hi, t_lo, 0, 0, 0, 6, 1, 3, 0, 0, 0, 10])?;
        let mut reply = [0u8; 9 + 20];
        stream.read_exact(&mut reply)?;
        std::thread::sleep(Duration::from_millis(250));
    }
    Ok(())
}

fn output_path() -> PathBuf {
    std::env::var_os("ANANASAR_SCREENSHOT_OUT")
        .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/screenshot.png"), PathBuf::from)
}

#[test]
#[ignore = "writes assets/screenshot.png; needs a (software) wgpu adapter, see docs/screenshots.md"]
fn render_readme_screenshot() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(super::CLIENT_W, super::CLIENT_H + 48.0))
        .with_pixels_per_point(1.5)
        .with_theme(egui::Theme::Light)
        .wgpu()
        .build_eframe(|cc| {
            cc.egui_ctx.set_theme(egui::ThemePreference::Light);
            theme::apply(&cc.egui_ctx, true);
            // The Courier face is read from the Windows fonts folder, so it is
            // left out: the image must look the same on every machine.
            let mut app = App::new(&cc.egui_ctx);
            app.port_notice = None;
            app.message = None;
            app.bank = Bank::Holding;
            {
                let mut regs = lock(&app.regs);
                for addr in 0..=u16::MAX {
                    regs.set(Bank::Holding, addr, sample_value(addr));
                    regs.set(Bank::Input, addr, sample_value(addr).rotate_left(3));
                }
            }
            app
        });

    let port = harness.state().server_port;
    let stop = Arc::new(AtomicBool::new(false));
    let poller = {
        let stop = stop.clone();
        std::thread::spawn(move || poll(port, &stop))
    };

    // Let the client connect and poll a few times so the Server panel has live values.
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        harness.step();
        let ready = lock(&harness.state().server.stats).slots.first().and_then(Option::as_ref).is_some_and(|c| c.poll_interval_ms.is_some() && c.bytes_sent > 100);
        if ready {
            break;
        }
        assert!(Instant::now() < deadline, "the client never polled the server");
        std::thread::sleep(Duration::from_millis(50));
    }
    harness.run();

    let image = harness.render().expect("render the window");
    let path = output_path();
    image.save(&path).expect("write the screenshot");
    eprintln!("wrote {}", path.display());

    stop.store(true, Ordering::SeqCst);
    let _ = poller.join();
}
