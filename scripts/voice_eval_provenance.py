"""Immutable inputs for resumable evaluation stages; reject mixed-run caches."""

from datetime import datetime, timezone
import ast
import hashlib
import importlib.metadata
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / ".cache/voice-eval"


def digest(path):
    checksum = hashlib.sha256()
    with Path(path).open("rb") as source:
        while block := source.read(1024 * 1024):
            checksum.update(block)
    return checksum.hexdigest()


def export_binary():
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=ROOT
        )
    )
    return Path(metadata["target_directory"]) / "release/voice-export"


def inputs(stage):
    code = [
        ROOT / "scripts/evaluate_voices.py",
        ROOT / "scripts/voice_eval_provenance.py",
    ]
    paths = [ROOT / "docs/performance/voice-corpus.json"]
    if stage == "generation":
        paths += [export_binary(), CACHE / "downloads.json"]
        for directory in ["kitten08", "paradee12", "kitten_code", "paradee_code"]:
            paths += [
                p
                for p in (CACHE / directory).rglob("*")
                if p.is_file() and "__pycache__" not in p.parts
            ]
        for directory in ["speech-kitten-cpu", "speech-heart"]:
            paths += [
                ROOT / "resources" / directory / name
                for name in ["voices.bin", "tokens.txt", "lexicon-us-en.txt"]
            ]
            paths += list((ROOT / "resources" / directory).glob("model.*.onnx"))
        paths += list((ROOT / "resources/runtime").rglob("*.dylib"))
        packages = [
            "numpy",
            "onnxruntime",
            "misaki",
            "spacy",
            "en-core-web-sm",
            "phonemizer-fork",
            "espeakng-loader",
        ]
    else:
        paths += [CACHE / "utmos22.pt"] + list((CACHE / "asr").glob("*"))
        paths += [p for p in (CACHE / "SpeechMOS/speechmos").rglob("*.py")]
        paths += [p for p in (CACHE / "whisper_normalizers").glob("*") if p.is_file()]
        packages = [
            "numpy",
            "torch",
            "torchaudio",
            "faster-whisper",
            "ctranslate2",
            "soundfile",
        ]
    return {
        "evaluation_ast_sha256": {
            str(p.relative_to(ROOT)): hashlib.sha256(
                ast.dump(ast.parse(p.read_text()), include_attributes=False).encode()
            ).hexdigest()
            for p in code
        },
        "files": {
            str(p.relative_to(ROOT)) if p.is_relative_to(ROOT) else str(p): digest(p)
            for p in sorted(set(paths))
            if p.is_file()
        },
        "packages": {name: importlib.metadata.version(name) for name in packages},
    }


def fingerprint(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()


def metadata(stage):
    corpus = json.loads((ROOT / "docs/performance/voice-corpus.json").read_text())
    shared = {
        "hardware": subprocess.check_output(
            ["sysctl", "-n", "machdep.cpu.brand_string"], text=True
        ).strip(),
        "corpus": corpus,
        "corpus_sha256": digest(ROOT / "docs/performance/voice-corpus.json"),
    }
    if stage == "generation":
        return dict(
            shared,
            export_binary_sha256=digest(export_binary()),
            downloads=json.loads((CACHE / "downloads.json").read_text()),
        )
    return dict(
        shared,
        utmos22_checkpoint_sha256=digest(CACHE / "utmos22.pt"),
        utmos22_code_commit=subprocess.check_output(
            ["git", "-C", str(CACHE / "SpeechMOS"), "rev-parse", "HEAD"], text=True
        ).strip(),
    )


def guard(stage):
    current = inputs(stage)
    path = CACHE / f"{stage}-provenance.json"
    if path.exists():
        saved = json.loads(path.read_text())
        if saved["cache_guard_inputs"] != current:
            raise RuntimeError(
                f"{stage} inputs changed. Keep this run intact and start a fresh evaluation cache; refusing to attach new provenance to old recordings/scores."
            )
        return saved
    cached = (
        list(CACHE.glob("generated-*.json*"))
        if stage == "generation"
        else [CACHE / "scored.jsonl"]
    )
    if any(p.exists() for p in cached):
        raise RuntimeError(
            f"Unfingerprinted {stage} cache. Audit the existing report explicitly or start a fresh cache."
        )
    saved = {
        "origin": "recorded_before_execution",
        "created_utc": datetime.now(timezone.utc).isoformat(),
        "cache_guard_inputs": current,
        "fingerprint": fingerprint(current),
        "metadata": metadata(stage),
    }
    path.write_text(json.dumps(saved, indent=2) + "\n")
    return saved


def verify_audio(row):
    if (row["success"] or row.get("wav_sha256")) and (
        not row.get("wav_sha256") or digest(row["wav"]) != row["wav_sha256"]
    ):
        raise RuntimeError(
            f"Recording changed or lacks a generation digest: {row['id']}"
        )


def audit_existing():
    """Explicit, labeled import of the critic-verified legacy run, never a fresh run."""
    data = json.loads(
        (ROOT / "docs/performance/voice-evaluation/results.json").read_text()
    )
    assert data["corpus_sha256"] == digest(ROOT / "docs/performance/voice-corpus.json")
    assert data["export_binary_sha256"] == digest(export_binary())
    assert data["utmos22_checkpoint_sha256"] == digest(CACHE / "utmos22.pt")
    assert data["downloads"] == json.loads((CACHE / "downloads.json").read_text())
    for artifact in data["downloads"]["artifacts"]:
        assert digest(ROOT / artifact["path"]) == artifact["sha256"], artifact["path"]
    # The unchanged native exporter verifies active assets against its compiled
    # digests before loading. Empty input checks both models without new audio.
    with tempfile.TemporaryDirectory(prefix="infotransfer-voice-audit-") as directory:
        corpus = Path(directory) / "empty.json"
        corpus.write_text("[]")
        for engine in ["kitten-cpu", "kokoro-heart"]:
            subprocess.run(
                [
                    str(export_binary()),
                    "--engine",
                    engine,
                    "--corpus",
                    str(corpus),
                    "--output",
                    directory,
                ],
                cwd=ROOT,
                check=True,
                capture_output=True,
                timeout=30,
            )
    expected = {r["id"]: r for r in data["rows"]}
    assert len(expected) == 294
    for row in expected.values():
        verify_audio(row)
    manifests = {}
    for stage in ["generation", "scoring"]:
        path = CACHE / f"{stage}-provenance.json"
        assert not path.exists(), (
            "An execution manifest already exists; never replace it with an audit"
        )
        current = inputs(stage)
        frozen = {
            key: data[key]
            for key in (
                ["hardware", "corpus_sha256", "export_binary_sha256", "downloads"]
                if stage == "generation"
                else ["utmos22_checkpoint_sha256", "utmos22_code_commit"]
            )
        }
        frozen["corpus"] = json.loads(
            (ROOT / "docs/performance/voice-corpus.json").read_text()
        )
        manifests[stage] = {
            "origin": "post_run_verified_import",
            "created_utc": datetime.now(timezone.utc).isoformat(),
            "limitation": "Original execution-time code fingerprints were not captured. This explicit audit freezes the previously reported artifact metadata and verifies retained waveforms; it does not reconstruct historical code fingerprints.",
            "cache_guard_inputs": current,
            "fingerprint": fingerprint(current),
            "metadata": frozen,
        }
    for path in CACHE.glob("generated-native-*.jsonl"):
        rows = [json.loads(line) for line in path.read_text().splitlines()]
        for row in rows:
            if row.get("event") == "recording":
                row["wav_sha256"] = expected[row["id"]]["wav_sha256"]
        path.write_text("".join(json.dumps(row) + "\n" for row in rows))
    for path in CACHE.glob("generated-*.json"):
        payload = json.loads(path.read_text())
        for row in payload["rows"]:
            row["wav_sha256"] = expected[row["id"]]["wav_sha256"]
        path.write_text(json.dumps(payload, indent=2) + "\n")
    score_path = CACHE / "scored.jsonl"
    rows = [json.loads(line) for line in score_path.read_text().splitlines()]
    for row in rows:
        prior = expected[row["id"]]
        assert (
            row["transcript"] == prior["transcript"]
            and row["utmos22"] == prior["utmos22"]
        )
        row["scoring_fingerprint"] = manifests["scoring"]["fingerprint"]
    score_path.write_text("".join(json.dumps(row) + "\n" for row in rows))
    for stage, saved in manifests.items():
        (CACHE / f"{stage}-provenance.json").write_text(
            json.dumps(saved, indent=2) + "\n"
        )
    print(
        "Imported the existing verified run with explicit post-run provenance limitations"
    )
