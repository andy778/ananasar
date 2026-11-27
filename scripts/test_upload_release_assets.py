"""Unit tests for upload-release-assets.py. Run: python -m unittest discover -s scripts"""
import contextlib
import importlib.util
import io
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("upload_release_assets", HERE / "upload-release-assets.py")
ur = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ur)


class UploadTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.file = str(pathlib.Path(tmp.name) / "asset")
        pathlib.Path(self.file).write_text("x")
        self.commands = []
        self.pauses = []

    def upload(self, codes, files=None):
        """Runs an upload in which `gh` exits with `codes`, one per call."""
        codes = iter(codes)

        def run(command):
            self.commands.append(command)
            return subprocess.CompletedProcess(command, next(codes))

        with contextlib.redirect_stdout(io.StringIO()):
            return ur.upload("v1.2.3", files or [self.file], run=run, sleep=self.pauses.append)

    def test_success_on_first_try(self):
        self.assertEqual(self.upload([0]), 0)
        self.assertEqual(self.commands, [["gh", "release", "upload", "v1.2.3", "--clobber", "--", self.file]])
        self.assertEqual(self.pauses, [])

    def test_retries_after_a_failure(self):
        self.assertEqual(self.upload([1, 1, 0]), 0)
        self.assertEqual(len(self.commands), 3)
        self.assertEqual(self.pauses, list(ur.PAUSES[:2]))

    def test_gives_up_after_the_last_try(self):
        self.assertEqual(self.upload([1] * (len(ur.PAUSES) + 1)), 1)
        self.assertEqual(len(self.commands), len(ur.PAUSES) + 1)
        self.assertEqual(self.pauses, list(ur.PAUSES))

    def test_missing_file_uploads_nothing(self):
        self.assertEqual(self.upload([], files=[self.file, self.file + ".gone"]), 1)
        self.assertEqual(self.commands, [])


if __name__ == "__main__":
    unittest.main()
