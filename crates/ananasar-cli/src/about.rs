//! `ananasar about`: the version, the dependencies and their licences, the
//! known advisories at build time and the full licence texts, all embedded when
//! the binary was built (see `build.rs` and `scripts/make-notices.sh`).

use clap::Args;
use ananasar_core::notices::{ADVISORIES, DEPENDENCIES, LICENSES};

/// Which part to print; with none given, the version, dependencies and advisories.
#[derive(Args)]
pub struct AboutArgs {
    /// Print the dependencies with their versions and licences
    #[arg(long)]
    deps: bool,
    /// Print the advisories known when this binary was built
    #[arg(long)]
    advisories: bool,
    /// Print the full licence texts of the dependencies
    #[arg(long)]
    licenses: bool,
}

pub fn run_about(args: &AboutArgs) -> i32 {
    let all = !(args.deps || args.advisories || args.licenses);
    println!("Ananasar {} (MIT licence)", env!("CARGO_PKG_VERSION"));
    if all || args.deps {
        println!("\n== Dependencies ==\n{DEPENDENCIES}");
    }
    if all || args.advisories {
        println!("\n== Advisories known at build time ==\n{ADVISORIES}");
    }
    if args.licenses {
        println!("\n== Licence texts ==\n{LICENSES}");
    }
    if all {
        println!("\nRun `ananasar about --licenses` for the licence texts. Check this binary against the current advisory database with `cargo audit bin <path to ananasar>`.");
    }
    0
}
