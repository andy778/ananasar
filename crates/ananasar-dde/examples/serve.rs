//! Serves the holding registers 0..=9 over DDE until Enter is pressed, for
//! trying the server with a DDE client. Windows only.

#[cfg(windows)]
fn main() {
    use ananasar_core::registers::RegisterFile;
    use ananasar_dde::item::{Activation, Scope};
    use std::sync::{Arc, Mutex};

    let regs = Arc::new(Mutex::new(RegisterFile::new()));
    let Ok(activation) = Activation::new(Scope::BothRange, "0", "9") else {
        return;
    };
    match ananasar_dde::Server::start(regs, activation) {
        Ok(_server) => {
            println!(
                "serving {}|holding!r0..r9 and input; press Enter to stop",
                ananasar_dde::SERVICE
            );
            let _ = std::io::stdin().read_line(&mut String::new());
        }
        Err(e) => eprintln!("{e}"),
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("DDE is only available on Windows");
}
