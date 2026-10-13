#![no_main]
//! Fuzz the MBAP frame parser: arbitrary bytes must never panic.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = ananasar_core::protocol::frame(data);
});
