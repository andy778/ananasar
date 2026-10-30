"""Unit tests for next-release.py. Run: python -m unittest discover -s scripts"""
import importlib.util
import pathlib
import unittest

spec = importlib.util.spec_from_file_location(
    "next_release", pathlib.Path(__file__).with_name("next-release.py")
)
nr = importlib.util.module_from_spec(spec)
spec.loader.exec_module(nr)

REPO = "o/r"


def commits(*items):
    """items are (subject, body) or just subject."""
    out = []
    for i, item in enumerate(items):
        subject, body = item if isinstance(item, tuple) else (item, "")
        c = nr.parse_commit(f"{i:040x}", subject, body)
        if c:
            out.append(c)
    return out


def version(tag, *items):
    v = nr.next_version(tag, commits(*items))
    return None if v is None else "v%d.%d.%d" % v


class Bump(unittest.TestCase):
    def test_major_via_bang(self):
        self.assertEqual(version("v1.2.3", "feat!: drop the old flag"), "v2.0.0")

    def test_major_via_footer(self):
        self.assertEqual(
            version("v1.2.3", ("fix: change output", "text\n\nBREAKING CHANGE: output differs")),
            "v2.0.0",
        )

    def test_scoped_types(self):
        self.assertEqual(version("v1.2.3", "feat(gui): new window"), "v1.3.0")
        self.assertEqual(version("v1.2.3", "fix(net): bug"), "v1.2.4")
        self.assertEqual(version("v1.2.3", "refactor(gui)!: rename"), "v2.0.0")
        self.assertEqual(version("v1.2.3", "feat(gui)!: rework"), "v2.0.0")

    def test_perf_is_patch(self):
        self.assertEqual(version("v1.2.3", "perf: faster"), "v1.2.4")

    def test_highest_bump_wins(self):
        self.assertEqual(version("v1.2.3", "fix: a", "feat: b", "fix: c"), "v1.3.0")
        self.assertEqual(version("v1.2.3", "fix: a", "feat: b", "chore!: c"), "v2.0.0")

    def test_no_release_types(self):
        for kind in ("chore", "docs", "ci", "refactor", "test", "style", "build"):
            self.assertIsNone(version("v1.2.3", f"{kind}: x", f"{kind}(s): y"), kind)

    def test_non_conventional_and_merge_style_ignored(self):
        self.assertIsNone(version("v1.2.3", "Merge pull request #1 from a/b", "WIP stuff"))

    def test_first_release(self):
        self.assertEqual(version(None, "fix: a"), "v1.0.0")
        self.assertEqual(version(None, "feat: a", "feat!: b"), "v1.0.0")
        self.assertIsNone(version(None, "docs: a"))

    def test_footer_hyphen_form(self):
        self.assertEqual(version("v1.0.0", ("fix: x", "BREAKING-CHANGE: y")), "v2.0.0")


class Notes(unittest.TestCase):
    def test_grouping_and_links(self):
        cs = commits("fix(gui): a", "feat: b", "chore: skip", "feat!: c")
        text = nr.notes((2, 0, 0), "v1.9.0", cs, REPO)
        self.assertIn("## [2.0.0](https://github.com/o/r/compare/v1.9.0...v2.0.0)", text)
        self.assertIn("### Breaking changes", text)
        self.assertIn("### Features", text)
        self.assertIn("### Fixes", text)
        self.assertIn("- **gui:** a ([0000000](https://github.com/o/r/commit/" + "0" * 40 + "))", text)
        self.assertNotIn("skip", text)
        self.assertLess(text.index("Breaking"), text.index("Features"))
        self.assertLess(text.index("Features"), text.index("Fixes"))

    def test_first_release_has_no_compare_link(self):
        text = nr.notes((1, 0, 0), None, commits("fix: a"), REPO)
        self.assertNotIn("compare", text)


if __name__ == "__main__":
    unittest.main()
