#!/usr/bin/env python3
"""Comparable per-voice startup sampling through the application's catalog.

Requires release reading-bench and prepared Kitten CPU / authentic Heart assets.
Times the real audio callback, not physical speech onset. Does not rate quality.
"""
import argparse
import hashlib
import json
import math
import platform
import random
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def percentile(values, p):
    values = sorted(values)
    return values[math.ceil(len(values) * p) - 1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rounds", type=int, default=5)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--device", default="MacBook Air Speakers")
    parser.add_argument("--output", type=Path, default=ROOT / "docs/performance/voice-startup.json")
    args = parser.parse_args()
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=ROOT))
    binary = Path(metadata["target_directory"]) / "release/reading-bench"
    models = {"kitten_cpu": ROOT / "resources/speech-kitten-cpu/model.cpu.onnx",
              "kokoro_heart": ROOT / "resources/speech-heart/model.int8.onnx"}
    result = {
        "method": "catalog_request_to_signal_render_minus50dBFS_not_acoustic",
        "hardware": subprocess.check_output(["sysctl", "-n", "machdep.cpu.brand_string"], text=True).strip(),
        "logical_cpus": subprocess.check_output(["sysctl", "-n", "hw.logicalcpu"], text=True).strip(),
        "platform": platform.platform(),
        "load": subprocess.check_output(["uptime"], text=True).strip(),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "model_sha256": {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in models.items()},
        "device": args.device, "threads": 2, "speed": 1,
        "first_chunk_characters": 32, "following_chunk_characters": 180,
        "scope": "Typical synthetic reading; excludes capture, GPUI, physical audio latency. First inference excluded from warm summary. Quality unmeasured.",
        "runs": [], "summary": {},
    }
    rng = random.Random(90210)
    for round_index in range(args.rounds):
        order = list(range(9))
        rng.shuffle(order)
        for voice in order:
            command = [str(binary), "--catalog", "--engine", "kitten-cpu", "--voice", str(voice),
                       "--runs", str(args.runs), "--device", args.device]
            try:
                process = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, timeout=180)
                rows = [json.loads(line) for line in process.stdout.splitlines() if line.startswith("{")]
                trials = [row for row in rows if row.get("event") == "trial"]
                failures = sum(not row.get("success", False) for row in trials)
                if process.returncode or len(trials) != args.runs:
                    failures += 1
                run = {"round": round_index, "voice": voice, "command": command,
                       "exit": process.returncode, "failures": failures, "rows": rows, "stderr": process.stderr}
            except subprocess.TimeoutExpired:
                run = {"round": round_index, "voice": voice, "command": command,
                       "exit": None, "failures": 1, "rows": [], "error": "process timeout"}
            result["runs"].append(run)
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(result, indent=2) + "\n")
            print(f"Round {round_index + 1}, voice {voice}: {run['failures']} failures", flush=True)
    for voice in range(9):
        runs = [run for run in result["runs"] if run["voice"] == voice]
        rows = [row for run in runs for row in run["rows"] if row.get("event") == "trial"
                and row.get("success") and row.get("phase") == "warm"]
        summary = {"samples": len(rows), "failures": sum(run["failures"] for run in runs)}
        for field in ["first_signal_render_ms", "stop_silence_estimate_ms", "peak_resident_bytes"]:
            values = [row[field] for row in rows if row.get(field) is not None]
            if values:
                summary[field] = {"p50": percentile(values, .5), "p95": percentile(values, .95),
                                  "min": min(values), "max": max(values)}
        result["summary"][str(voice)] = summary
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result["summary"], indent=2))
    if any(run["failures"] for run in result["runs"]):
        raise SystemExit("Voice sampling failed; inspect the raw report before publishing ratings")


if __name__ == "__main__":
    main()
