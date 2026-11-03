#!/usr/bin/env python3
"""Interleaved first-chunk synthesis experiment; not audible/app latency."""
import argparse
import json
import math
import platform
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def percentile(values, fraction):
    values = sorted(values)
    return values[min(len(values) - 1, math.ceil(len(values) * fraction) - 1)]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rounds", type=int, default=5)
    parser.add_argument("--runs", type=int, default=6)
    parser.add_argument("--words", type=int, default=6)
    parser.add_argument("--threads", type=int, nargs="+", default=[2, 4])
    parser.add_argument("--engine", default="kitten")
    parser.add_argument("--output", type=Path, default=ROOT / ".cache/bench/thread-experiment.json")
    args = parser.parse_args()
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=ROOT))
    binary = Path(metadata["target_directory"]) / "release/speech-bench"
    result = {"method": "first_chunk_model_callback_not_audible_output", "platform": platform.platform(),
              "hardware": subprocess.check_output(["sysctl", "-n", "machdep.cpu.brand_string"], text=True).strip(),
              "started": time.time(), "words": args.words, "engine": args.engine, "runs": [], "summary": {}}
    for round_index in range(args.rounds):
        # Reverse each alternate round so each side sees both earlier/later load.
        order = args.threads if round_index % 2 == 0 else list(reversed(args.threads))
        for threads in order:
            command = [str(binary), "--engine", args.engine, "--runs", str(args.runs),
                       "--threads", str(threads), "--words", str(args.words)]
            process = subprocess.run(command, capture_output=True, text=True, timeout=120)
            rows = []
            for line in process.stdout.splitlines():
                rows.append(json.loads(line))
            generated = [row for row in rows if row.get("event") == "generation"]
            failures = sum(not row.get("success", False) for row in generated)
            if process.returncode or len(generated) != args.runs:
                failures += 1
            result["runs"].append({"round": round_index, "threads": threads, "exit_code": process.returncode,
                                   "failures": failures, "rows": rows, "stderr": process.stderr})
            print(f"Round {round_index + 1}: {threads} threads, {len(generated)} generations, {failures} failures", flush=True)
    for threads in args.threads:
        trials = [trial for trial in result["runs"] if trial["threads"] == threads]
        warm = [row["first_pcm_ms"] for trial in trials for row in trial["rows"]
                if row.get("event") == "generation" and row.get("phase") == "warm" and row.get("success")]
        if warm:
            result["summary"][str(threads)] = {"samples": len(warm), "p50_ms": percentile(warm, .5),
                                             "p95_ms": percentile(warm, .95), "min_ms": min(warm), "max_ms": max(warm),
                                             "failures": sum(trial["failures"] for trial in trials)}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result["summary"], indent=2))
    if any(trial["failures"] for trial in result["runs"]):
        raise SystemExit("Experiment contains failures; no speed verdict is valid.")


if __name__ == "__main__":
    main()
