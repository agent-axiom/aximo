"""Exercise model setup without network access or large model downloads."""
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "fetch-models.sh"
FILES = ("encoder-model.int8.onnx", "decoder_joint-model.int8.onnx", "nemo128.onnx", "vocab.txt")


class FetchModelsTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.models = self.root / "models"
        self.target = self.models / "parakeet-tdt-0.6b-v3-int8"
        self.bin = self.root / "bin"
        self.bin.mkdir()
        curl = self.bin / "curl"
        curl.write_text('#!/usr/bin/env bash\nset -eu\nwhile [[ "$1" != "-o" ]]; do shift; done\ncp "$TEST_ARCHIVE" "$2"\n')
        curl.chmod(0o755)
        self.env = {**os.environ, "PATH": str(self.bin) + os.pathsep + os.environ["PATH"]}
        self.env.pop("AXIMO_FORCE", None)
        self.env.pop("AXIMO_MODELS_DIR", None)

    def archive(self, missing=None, dirname="parakeet-tdt-0.6b-v3-int8"):
        bundle = self.root / "bundle" / dirname
        bundle.mkdir(parents=True)
        for name in FILES:
            if name != missing:
                (bundle / name).write_bytes(b"fixture")
        archive = self.root / "fixture.tar.gz"
        with tarfile.open(archive, "w:gz") as tar:
            tar.add(bundle, arcname=dirname)
        self.env["TEST_ARCHIVE"] = str(archive)

    def run_script(self):
        return subprocess.run(["bash", str(SCRIPT), str(self.models)], env=self.env, text=True, capture_output=True)

    def test_valid_bundle_and_repeat(self):
        self.archive()
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(all((self.target / name).is_file() for name in FILES))
        del self.env["TEST_ARCHIVE"]  # Repeat must not download again.
        self.assertEqual(self.run_script().returncode, 0)

    def test_alternative_archive_directory(self):
        self.archive(dirname="parakeet-v3")
        self.assertEqual(self.run_script().returncode, 0)

    def test_incomplete_existing_directory_is_not_success(self):
        self.target.mkdir(parents=True)
        result = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("AXIMO_FORCE=1", result.stderr)

    def test_empty_existing_file_is_not_success(self):
        self.target.mkdir(parents=True)
        for name in FILES:
            (self.target / name).write_bytes(b"")
        self.assertNotEqual(self.run_script().returncode, 0)

    def test_invalid_download_preserves_previous_model(self):
        self.target.mkdir(parents=True)
        for name in FILES:
            (self.target / name).write_bytes(b"previous")
        self.archive(missing="nemo128.onnx")
        self.env["AXIMO_FORCE"] = "1"
        self.assertNotEqual(self.run_script().returncode, 0)
        self.assertEqual((self.target / "vocab.txt").read_bytes(), b"previous")

    def test_directory_instead_of_model_file_preserves_previous_model(self):
        self.target.mkdir(parents=True)
        for name in FILES:
            (self.target / name).write_bytes(b"previous")
        self.archive(missing="encoder-model.int8.onnx")
        bundle = self.root / "bundle" / "parakeet-tdt-0.6b-v3-int8"
        (bundle / "encoder-model.int8.onnx").mkdir()
        with tarfile.open(self.env["TEST_ARCHIVE"], "w:gz") as tar:
            tar.add(bundle, arcname=bundle.name)
        self.env["AXIMO_FORCE"] = "1"
        self.assertNotEqual(self.run_script().returncode, 0)
        self.assertEqual((self.target / "vocab.txt").read_bytes(), b"previous")

    def test_forced_valid_download_replaces_previous_model(self):
        self.target.mkdir(parents=True)
        (self.target / "vocab.txt").write_bytes(b"previous")
        self.archive()
        self.env["AXIMO_FORCE"] = "1"
        self.assertEqual(self.run_script().returncode, 0)
        self.assertEqual((self.target / "vocab.txt").read_bytes(), b"fixture")


if __name__ == "__main__":
    unittest.main()
