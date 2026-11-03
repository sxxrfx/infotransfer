"""Exercise cache rejection without modifying the retained evaluation run."""

import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import voice_eval_provenance as provenance


class CacheProvenanceTests(unittest.TestCase):
    def test_unchanged_resume_preserves_execution_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            with (
                patch.object(provenance, "CACHE", Path(directory)),
                patch.object(provenance, "inputs", return_value={"model": "a"}),
                patch.object(provenance, "metadata", return_value={"corpus": ["old"]}),
            ):
                original = provenance.guard("generation")
                with patch.object(
                    provenance, "metadata", return_value={"corpus": ["new"]}
                ):
                    self.assertEqual(provenance.guard("generation"), original)

    def test_changed_inputs_reject_reuse_without_rewriting_manifest(self):
        with tempfile.TemporaryDirectory() as directory:
            with (
                patch.object(provenance, "CACHE", Path(directory)),
                patch.object(provenance, "inputs", return_value={"corpus": "a"}),
                patch.object(provenance, "metadata", return_value={}),
            ):
                original = provenance.guard("scoring")
                with patch.object(provenance, "inputs", return_value={"corpus": "b"}):
                    with self.assertRaisesRegex(RuntimeError, "inputs changed"):
                        provenance.guard("scoring")
                stored = json.loads(
                    (Path(directory) / "scoring-provenance.json").read_text()
                )
                self.assertEqual(stored, original)

    def test_changed_audio_rejects_reuse(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "recording.wav"
            path.write_bytes(b"original recording")
            row = {
                "id": "voice-prose",
                "success": True,
                "wav": str(path),
                "wav_sha256": provenance.digest(path),
            }
            provenance.verify_audio(row)
            path.write_bytes(b"replaced recording")
            with self.assertRaisesRegex(RuntimeError, "Recording changed"):
                provenance.verify_audio(row)

    def test_legacy_cache_needs_explicit_audit(self):
        with tempfile.TemporaryDirectory() as directory:
            with (
                patch.object(provenance, "CACHE", Path(directory)),
                patch.object(provenance, "inputs", return_value={}),
            ):
                (Path(directory) / "scored.jsonl").write_text("{}\n")
                with self.assertRaisesRegex(RuntimeError, "Unfingerprinted"):
                    provenance.guard("scoring")


if __name__ == "__main__":
    unittest.main()
