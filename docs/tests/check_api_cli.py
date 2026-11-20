"""Exercise API exporter rejection paths without compiling Rust."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class ExportCli(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="openrust-api-cli-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.script = self.root / "docs/scripts/build_api.py"
        self.script.parent.mkdir(parents=True)
        shutil.copy2(Path(__file__).resolve().parents[1] / "scripts/build_api.py", self.script)

    def reject(self, *args, reason):
        result = subprocess.run([
            "python3", "-B", str(self.script), "--output", str(self.root / "output"),
            "--target-dir", str(self.root / "target"), *args,
        ], capture_output=True, text=True)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn(reason, result.stderr)

    def checkout(self):
        def git(*args):
            return subprocess.check_output(["git", "-c", "user.name=API test", "-c",
                "user.email=api@example.invalid", "-c", "commit.gpgsign=false", *args],
                cwd=self.root, text=True, stderr=subprocess.PIPE).strip()
        git("init", "-q")
        git("add", ".")
        git("commit", "-qm", "fixture")
        return git("rev-parse", "HEAD")

    def test_archive_requires_revision(self):
        self.reject(reason="--revision is required")

    def test_full_revision_required(self):
        self.reject("--revision", "invalid", reason="full Git commit SHA")

    def test_existing_output_preserved(self):
        output = self.root / "output"
        output.mkdir()
        marker = output / "marker"
        marker.write_text("keep")
        self.reject("--revision", "a" * 40, reason="already exists")
        self.assertEqual(marker.read_text(), "keep")

    def test_nested_directories_rejected(self):
        self.reject("--output", str(self.root / "target/inside"), "--revision", "a" * 40,
                    reason="separate directories")

    def test_revision_must_match_head(self):
        self.checkout()
        self.reject("--revision", "a" * 40, reason="checkout's HEAD")

    def test_explicit_revision_cannot_bypass_dirty_checkout(self):
        revision = self.checkout()
        self.script.write_text(self.script.read_text() + "\n# dirty\n")
        self.reject("--revision", revision, reason="uncommitted changes")

    def test_untracked_source_rejected(self):
        revision = self.checkout()
        (self.root / "untracked.rs").write_text("// new source\n")
        self.reject("--revision", revision, reason="uncommitted changes")


if __name__ == "__main__":
    unittest.main()
