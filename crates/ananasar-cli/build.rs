//! Embeds the third-party notices into the binary. A release build points
//! `ANANASAR_NOTICES` at the directory `scripts/make-notices.sh` filled; any
//! other build gets a short placeholder for each part.

use std::path::{Path, PathBuf};

const PARTS: [(&str, &str); 3] = [
    ("dependencies.txt", "The dependency list is only embedded in release builds."),
    ("advisories.txt", "The advisory report is only embedded in release builds."),
    ("licenses.txt", "The licence texts are only embedded in release builds."),
];

fn main() {
    println!("cargo:rerun-if-env-changed=ANANASAR_NOTICES");
    let Some(out) = std::env::var_os("OUT_DIR").map(PathBuf::from) else { return };
    let from = std::env::var_os("ANANASAR_NOTICES").map(PathBuf::from);
    for (name, placeholder) in PARTS {
        let source: Option<&Path> = from.as_deref();
        let text = source.and_then(|dir| std::fs::read_to_string(dir.join(name)).ok());
        if let Some(dir) = source {
            println!("cargo:rerun-if-changed={}", dir.join(name).display());
        }
        let text = text.unwrap_or_else(|| format!("{placeholder}\n"));
        if let Err(e) = std::fs::write(out.join(name), text) {
            println!("cargo:warning=cannot write {name}: {e}");
        }
    }
}
