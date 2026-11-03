#!/usr/bin/env python3
"""Download pinned experimental models; never add them to the shipping catalog."""

import concurrent.futures
import hashlib
import json
from pathlib import Path
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / ".cache/voice-eval"


def fetch(url, path, size=None, sha=None):
    path.parent.mkdir(parents=True, exist_ok=True)
    if not path.exists():
        temporary = path.with_suffix(path.suffix + ".part")
        with (
            urllib.request.urlopen(url, timeout=90) as source,
            temporary.open("wb") as target,
        ):
            while block := source.read(1024 * 1024):
                target.write(block)
        temporary.replace(path)
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    if size is not None and path.stat().st_size != size:
        raise ValueError(f"Wrong length: {path}")
    if sha is not None and digest != sha:
        raise ValueError(f"Wrong SHA-256: {path}")
    return {
        "path": str(path.relative_to(ROOT)),
        "url": url,
        "bytes": path.stat().st_size,
        "sha256": digest,
    }


def main():
    specs = [
        (
            "KittenML/kitten-tts-nano-0.8-int8",
            "84781d74e29ee25217551556398b42f80593a813",
            "kitten08",
            ["kitten_tts_nano_v0_8.onnx", "voices.npz", "config.json", "README.md"],
        ),
        (
            "sahilmahendrakar/Paradee-8M-v1.2",
            "ed2934d4df8a4fd891e29584c5e1104649409765",
            "paradee12",
            ["onnx/paradee_int8.onnx", "config.json", "LICENSE", "README.md"],
        ),
        (
            "Systran/faster-whisper-small.en",
            None,
            "asr",
            [
                "model.bin",
                "config.json",
                "tokenizer.json",
                "vocabulary.txt",
                "README.md",
            ],
        ),
    ]
    jobs = []
    revisions = {}
    for repo, revision, directory, names in specs:
        endpoint = (
            f"https://huggingface.co/api/models/{repo}"
            + (f"/revision/{revision}" if revision else "")
            + "?blobs=true"
        )
        metadata = json.load(urllib.request.urlopen(endpoint, timeout=60))
        revision = metadata["sha"]
        revisions[repo] = revision
        CACHE.mkdir(parents=True, exist_ok=True)
        (CACHE / f"{directory}-metadata.json").write_text(
            json.dumps(metadata, indent=2) + "\n"
        )
        files = {entry["rfilename"]: entry for entry in metadata["siblings"]}
        for name in names:
            entry = files[name]
            jobs.append(
                (
                    f"https://huggingface.co/{repo}/resolve/{revision}/{name}",
                    CACHE / directory / name,
                    entry.get("size"),
                    entry.get("lfs", {}).get("sha256"),
                )
            )
    for filename in ["onnx_model.py", "preprocess.py"]:
        jobs.append(
            (
                f"https://raw.githubusercontent.com/KittenML/KittenTTS/0.8.1/kittentts/{filename}",
                CACHE / "kitten_code" / filename,
            )
        )
    for filename in ["basic.py", "english.py", "english.json"]:
        jobs.append(
            (
                f"https://raw.githubusercontent.com/openai/whisper/v20250625/whisper/normalizers/{filename}",
                CACHE / "whisper_normalizers" / filename,
            )
        )
    jobs += [
        (
            "https://raw.githubusercontent.com/KittenML/KittenTTS/0.8.1/LICENSE",
            CACHE / "kitten_code/LICENSE",
        ),
        (
            "https://raw.githubusercontent.com/sahilmahendrakar/paradee/v1.2/paradee/tts.py",
            CACHE / "paradee_code/tts.py",
        ),
        (
            "https://github.com/tarepan/SpeechMOS/releases/download/v1.0.0/utmos22_strong_step7459_v1.pt",
            CACHE / "utmos22.pt",
        ),
    ]
    with concurrent.futures.ThreadPoolExecutor(max_workers=5) as pool:
        artifacts = list(pool.map(lambda args: fetch(*args), jobs))
    for directory in ["kitten_code", "paradee_code", "whisper_normalizers"]:
        (CACHE / directory / "__init__.py").touch()
    (CACHE / "downloads.json").write_text(
        json.dumps({"revisions": revisions, "artifacts": artifacts}, indent=2) + "\n"
    )
    for artifact in artifacts:
        print(
            f"{artifact['bytes']:>10} {artifact['sha256']} {artifact['path']}",
            flush=True,
        )


if __name__ == "__main__":
    main()
