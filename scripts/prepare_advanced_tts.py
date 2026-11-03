#!/usr/bin/env python3
"""Fetch pinned local evaluation assets and retain streaming SHA-256 evidence."""

import concurrent.futures
import hashlib
import json
from pathlib import Path
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / ".cache/advanced-tts"
SPECS = {
    "s3tokenizer": ("mlx-community/S3TokenizerV3", "b143914b3e912278104824da706edc9c2d317c4e"),
    "qwen3": ("mlx-community/Qwen3-TTS-12Hz-1.7B-CustomVoice-4bit", "f35faf19b0cc2160865af64ecf0f22f83d335135"),
    "voxcpm2": ("mlx-community/VoxCPM2-4bit", "dc9e5c187858da5f4a13dc4c247e297339216381"),
    "cosyvoice3": ("mlx-community/Fun-CosyVoice3-0.5B-2512-4bit", "55a6713d54751f1ec2645aa11294676f2067202a"),
    "sopro": ("samuel-vitorino/sopro-v2-turbo", "c12d0dc7c4a33b3cb6ea3d11ceb70bc60c7c6312"),
    "phonon": ("gradium/phonon-31m-en", "92741c5b07d085ff840e26fea8ce43110b87852f"),
}


def digest(path):
    checksum = hashlib.sha256()
    with path.open("rb") as stream:
        while block := stream.read(1024 * 1024):
            checksum.update(block)
    return checksum.hexdigest()


def fetch(url, path, size=None, expected=None):
    path.parent.mkdir(parents=True, exist_ok=True)
    if not path.exists() or (size is not None and path.stat().st_size != size):
        temporary = path.with_suffix(path.suffix + ".part")
        with urllib.request.urlopen(url, timeout=180) as source, temporary.open("wb") as target:
            while block := source.read(1024 * 1024):
                target.write(block)
        if size is not None and temporary.stat().st_size != size:
            raise ValueError(f"Incomplete download: {path.name}")
        if expected and digest(temporary) != expected:
            raise ValueError(f"SHA-256 mismatch: {path.name}")
        temporary.replace(path)
    checksum = digest(path)
    if expected and checksum != expected:
        raise ValueError(f"SHA-256 mismatch: {path.name}")
    return {"path": str(path.relative_to(ROOT)), "url": url, "bytes": path.stat().st_size, "sha256": checksum}


def main():
    CACHE.mkdir(parents=True, exist_ok=True)
    manifest = {"models": {}, "artifacts": []}
    jobs = []
    for key, (repo, revision) in SPECS.items():
        metadata = json.load(urllib.request.urlopen(f"https://huggingface.co/api/models/{repo}/revision/{revision}?blobs=true", timeout=60))
        assert metadata["sha"] == revision
        manifest["models"][key] = {"repository": repo, "revision": revision, "declared_license": metadata.get("cardData", {}).get("license")}
        (CACHE / f"{key}-metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
        for entry in metadata["siblings"]:
            name = entry["rfilename"]
            if name == ".gitattributes":
                continue
            if key == "sopro" and name.endswith(".gguf"):
                continue
            if key == "phonon" and (name.endswith(".safetensors") and name != "default-voice.safetensors" or name.endswith(".gguf") and name != "model.q8.gguf"):
                continue
            url = f"https://huggingface.co/{repo}/resolve/{revision}/{name}"
            jobs.append((url, CACHE / key / name, entry.get("size"), entry.get("lfs", {}).get("sha256")))
    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
        futures = [pool.submit(fetch, *job) for job in jobs]
        for future in concurrent.futures.as_completed(futures):
            artifact = future.result()
            manifest["artifacts"].append(artifact)
            print(json.dumps({"downloaded": artifact["path"], "bytes": artifact["bytes"]}), flush=True)
            (CACHE / "downloads.json").write_text(json.dumps(manifest, indent=2) + "\n")
    manifest["artifacts"].sort(key=lambda row: row["path"])
    (CACHE / "downloads.json").write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    main()
