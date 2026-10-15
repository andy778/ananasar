# README screenshot

`assets/screenshot.png` is rendered by the real main window, headlessly, so it can be refreshed whenever the look changes instead of being retaken by hand.

The generator is the ignored test `render_readme_screenshot` in `crates/ananasar-gui/src/screenshot.rs`. It uses [egui_kittest](https://docs.rs/egui_kittest) (a dev-dependency of `ananasar-gui` only) with its wgpu renderer, which prefers a software adapter, so no GPU or display is needed. The test:

- starts the app and its embedded server on a loopback port,
- fills the register file with varied non-zero values,
- connects a client thread that polls function 3 every 250 ms, and waits until the Server panel shows live values,
- renders the window at 1.5x and writes the PNG.

It leaves out the Courier face that the app reads from the Windows fonts folder, so the image looks the same on every machine. The window has no OS title bar. The clock in the status bar shows the time of the run.

## Regenerate

In CI (recommended, as it is the reference environment): run the **screenshot** workflow from the Actions tab (*Run workflow*), download the `screenshot` artifact, replace `assets/screenshot.png` with it and commit it with a `docs:` message.

Locally (Rust 1.95 or newer):

```bash
cargo test -p ananasar-gui render_readme_screenshot -- --ignored --nocapture
```

This writes `assets/screenshot.png`; set `ANANASAR_SCREENSHOT_OUT` to write somewhere else. On Linux without a GPU, install a software Vulkan driver first (`sudo apt install mesa-vulkan-drivers`). If the image changes the content shown, update the alt text in `README.md` too.
