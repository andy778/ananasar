#![no_main]
//! Fuzz the server request handler end to end: a malicious/malformed request
//! must never panic the server (no crash / DoS).
use libfuzzer_sys::fuzz_target;
use ananasar_core::registers::RegisterFile;
use ananasar_core::server::{process, ServerConfig};
use std::cell::RefCell;

thread_local! {
    // Reuse the 256 KiB register file across iterations instead of reallocating.
    static CTX: RefCell<(ServerConfig, RegisterFile)> =
        RefCell::new((ServerConfig::default(), RegisterFile::new()));
}

fuzz_target!(|data: &[u8]| {
    CTX.with(|c| {
        let (cfg, regs) = &mut *c.borrow_mut();
        let _ = process(cfg, regs, data);
    });
});
