//! The third-party notices of a release build: the dependency list, the
//! advisories known when it was built and the licence texts. Release builds
//! embed the files `scripts/make-notices.sh` writes (see `build.rs`); other
//! builds get a one-line placeholder for each part.

/// Every dependency with its version and licence, one per line.
pub const DEPENDENCIES: &str = include_str!(concat!(env!("OUT_DIR"), "/dependencies.txt"));
/// The RustSec advisories known when the binary was built.
pub const ADVISORIES: &str = include_str!(concat!(env!("OUT_DIR"), "/advisories.txt"));
/// The licence texts of all dependencies.
pub const LICENSES: &str = include_str!(concat!(env!("OUT_DIR"), "/licenses.txt"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_part_has_text() {
        for part in [DEPENDENCIES, ADVISORIES, LICENSES] {
            assert!(!part.trim().is_empty());
        }
    }
}
