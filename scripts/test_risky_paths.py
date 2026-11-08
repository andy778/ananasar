"""Unit tests for risky_paths.py. Run: python -m unittest discover -s scripts"""
import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("risky_paths", HERE / "risky_paths.py")
rp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rp)

CODEOWNERS_PATH = HERE.parent / ".github" / "CODEOWNERS"
CODEOWNERS = CODEOWNERS_PATH.read_text(encoding="utf-8")


def f(name, patch="@@ -1 +1 @@\n+x", **kw):
    d = {"filename": name, "status": "modified"}
    if patch is not None:
        d["patch"] = patch
    d.update(kw)
    return d


def check(*files):
    return rp.reasons(CODEOWNERS, list(files))


class RiskyPathsTest(unittest.TestCase):
    def test_workflow_is_flagged(self):
        self.assertTrue(check(f(".github/workflows/ci.yml")))

    def test_docs_and_tests_are_not_flagged(self):
        self.assertEqual(check(f("docs/x.md"), f("crates/ananasar-storm/tests/x.rs")), [])

    def test_crate_manifest_is_flagged_by_glob(self):
        self.assertTrue(check(f("crates/ananasar-gui/Cargo.toml")))
        self.assertTrue(check(f("Cargo.toml")))

    def test_exact_files(self):
        for name in ("deny.toml", "Cargo.lock", ".cargo/config.toml", "AGENTS.md",
                     "crates/ananasar-net/src/lib.rs", "scripts/next-release.py"):
            self.assertTrue(check(f(name, patch=None if name.endswith(".rs") else "x")), name)
        self.assertEqual(check(f("crates/ananasar-core/src/lib.rs")), [])

    def test_rename_out_of_a_risky_path_is_flagged(self):
        r = check(f("docs/x", status="renamed", previous_filename=".github/x"))
        self.assertTrue(any(".github/x" in line for line in r))

    def test_added_unsafe_and_allow_are_flagged_with_file(self):
        for added in ("unsafe {", "unsafe fn f() {}", "unsafe impl Send for T {}",
                      "#[allow(clippy::x)]", "#![allow(dead_code)]", "#[expect(unused)]"):
            r = check(f("crates/ananasar-cli/src/a.rs", patch=f"@@ -1 +1 @@\n context\n+{added}"))
            self.assertTrue(r and "crates/ananasar-cli/src/a.rs" in r[0], added)

    def test_removed_and_context_lines_are_not_flagged(self):
        patch = "@@ -1,2 +1,2 @@\n-unsafe {\n #[allow(clippy::x)]\n+safe();"
        self.assertEqual(check(f("crates/ananasar-cli/src/a.rs", patch=patch)), [])

    def test_rs_without_patch_needs_hand_check(self):
        r = check(f("crates/ananasar-cli/src/a.rs", patch=None))
        self.assertEqual(r, ["crates/ananasar-cli/src/a.rs: patch not available, check by hand"])

    def test_unknown_pattern_fails_loudly(self):
        with self.assertRaises(ValueError):
            rp.parse_patterns("/crates/*/src/ @a")

    def test_load_files_accepts_paginated_output(self):
        self.assertEqual(len(rp.load_files('[{"filename":"a"}][{"filename":"b"}]')), 2)

    def test_real_codeowners_parses_and_lists_every_risky_path(self):
        patterns = rp.parse_patterns(CODEOWNERS)
        shown = {rp._show(p) for p in patterns}
        for want in ("/.github/", "/deny.toml", "**/Cargo.toml", "/Cargo.lock", "/.cargo/",
                     "/crates/ananasar-core/src/protocol.rs", "/crates/ananasar-core/src/server.rs",
                     "/crates/ananasar-net/src/lib.rs", "/scripts/next-release.py",
                     "/scripts/stamp-version.sh", "/SECURITY.md", "/AGENTS.md", "/CLAUDE.md"):
            self.assertIn(want, shown)


if __name__ == "__main__":
    unittest.main()
