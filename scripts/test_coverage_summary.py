"""Unit tests for coverage-summary.py. Run: python -m unittest discover -s scripts"""
import importlib.util
import pathlib
import unittest

spec = importlib.util.spec_from_file_location(
    "coverage_summary", pathlib.Path(__file__).with_name("coverage-summary.py")
)
cs = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cs)


def export(*files):
    """files are (filename, covered, count)."""
    entries = [{"filename": n, "summary": {"lines": {"covered": c, "count": t}}} for n, c, t in files]
    covered = sum(c for _, c, _ in files)
    count = sum(t for _, _, t in files)
    return {"data": [{"files": entries, "totals": {"lines": {"covered": covered, "count": count}}}]}


class CoverageSummaryTest(unittest.TestCase):
    def test_groups_files_by_crate(self):
        e = export(
            ("/w/ananasar/crates/ananasar-core/src/frame.rs", 8, 10),
            ("/w/ananasar/crates/ananasar-core/src/lib.rs", 2, 10),
            ("/w/ananasar/crates/ananasar-net/src/lib.rs", 5, 5),
            ("/w/ananasar/build.rs", 0, 4),
        )
        self.assertEqual(
            cs.per_crate(e),
            {"ananasar-core": (10, 20), "ananasar-net": (5, 5), "other": (0, 4)},
        )

    def test_markdown_has_a_row_per_crate_and_the_total(self):
        e = export(("/w/crates/b/src/lib.rs", 1, 2), ("/w/crates/a/src/lib.rs", 3, 4))
        rows = cs.markdown(e).splitlines()
        self.assertEqual(rows[2], "| `a` | 4 | 3 | 75.0% |")
        self.assertEqual(rows[3], "| `b` | 2 | 1 | 50.0% |")
        self.assertEqual(rows[4], "| **total** | 6 | 4 | **66.7%** |")

    def test_badge_is_a_shields_endpoint(self):
        b = cs.badge(export(("/w/crates/a/src/lib.rs", 823, 1000)))
        self.assertEqual(b, {"schemaVersion": 1, "label": "coverage", "message": "82.3%", "color": "green"})

    def test_badge_with_no_lines_does_not_divide_by_zero(self):
        self.assertEqual(cs.badge(export())["message"], "0.0%")
        self.assertEqual(cs.badge(export())["color"], "red")


if __name__ == "__main__":
    unittest.main()
