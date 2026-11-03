#!/usr/bin/env python3
"""Interleaved CPU generation screening; not shortcut-to-audible app latency."""

import argparse
import json
from pathlib import Path
import random
import resource
import subprocess
import sys
import time

from evaluate_voices import CACHE, ROOT, candidate_engine, sha

TEXT = "Reading at your own pace"
CONFIGS = ["native-kitten", "native-heart", "kitten08", "paradee12"]


def one(kind):
    import numpy as np

    start = time.perf_counter()
    engine, generate, _ = candidate_engine(kind)
    load = (time.perf_counter() - start) * 1000
    voice = "Hugo" if kind == "kitten08" else "af_heart"
    rows = []
    for index in range(11):
        start = time.perf_counter()
        audio = np.asarray(generate(TEXT, voice)).reshape(-1)
        elapsed = (time.perf_counter() - start) * 1000
        valid = bool(
            len(audio)
            and np.isfinite(audio).all()
            and np.abs(audio).max() >= 0.00316227766
        )
        rows.append(
            {
                "trial": index,
                "phase": "priming" if index == 0 else "warm",
                "ms": elapsed,
                "samples": len(audio),
                "peak": float(np.abs(audio).max()) if len(audio) else 0,
                "success": valid,
            }
        )
    print(
        json.dumps(
            {
                "kind": kind,
                "voice": voice,
                "load_ms": load,
                "peak_rss_bytes": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
                "rows": rows,
            }
        )
    )


def main():
    import numpy as np

    binary = (
        Path(
            json.loads(
                subprocess.check_output(
                    ["cargo", "metadata", "--no-deps", "--format-version", "1"],
                    cwd=ROOT,
                )
            )["target_directory"]
        )
        / "release/speech-bench"
    )
    result = {
        "method": "CPU text-to-complete-PCM generation; no selection capture, UI, playback, output device or physical audio",
        "hardware": subprocess.check_output(
            ["sysctl", "-n", "machdep.cpu.brand_string"], text=True
        ).strip(),
        "os": subprocess.check_output(
            ["sw_vers", "-productVersion"], text=True
        ).strip(),
        "text": TEXT,
        "threads": 2,
        "speed": 1,
        "rounds": 5,
        "priming_per_process": 1,
        "warm_per_process": 10,
        "load_before": subprocess.check_output(["uptime"], text=True).strip(),
        "native_binary_sha256": sha(binary),
        "runs": [],
    }
    for round_ in range(5):
        order = CONFIGS.copy()
        random.Random(42 + round_).shuffle(order)
        for kind in order:
            if kind.startswith("native"):
                engine, speaker = (
                    ("kitten-cpu", "0")
                    if kind == "native-kitten"
                    else ("kokoro-heart", "3")
                )
                command = [
                    "/usr/bin/time",
                    "-l",
                    str(binary),
                    "--engine",
                    engine,
                    "--voice",
                    speaker,
                    "--threads",
                    "2",
                    "--runs",
                    "11",
                    "--words",
                    "5",
                ]
            else:
                command = [sys.executable, __file__, "--one", kind]
            process = subprocess.run(
                command, cwd=ROOT, capture_output=True, text=True, timeout=180
            )
            if process.returncode:
                raise RuntimeError(f"{kind}: {process.stderr[-2000:]}")
            payloads = [
                json.loads(line)
                for line in process.stdout.splitlines()
                if line.startswith("{")
            ]
            if kind.startswith("native"):
                generated = [r for r in payloads if r.get("event") == "generation"]
                rows = [
                    {
                        "trial": r["run"],
                        "phase": "priming" if r["run"] == 0 else "warm",
                        "ms": r["total_ms"],
                        "samples": r["samples"],
                        "peak": r["peak"],
                        "success": r["success"]
                        and r["samples"] > 0
                        and r["peak"] >= 0.00316227766,
                    }
                    for r in generated
                ]
                peak = next(
                    int(line.split()[0])
                    for line in process.stderr.splitlines()
                    if "maximum resident set size" in line
                )
                run = {
                    "kind": kind,
                    "voice": "Arthur" if kind == "native-kitten" else "Heart",
                    "load_ms": next(
                        r["load_ms"] for r in payloads if r.get("event") == "loaded"
                    ),
                    "peak_rss_bytes": peak,
                    "rows": rows,
                }
            else:
                run = payloads[-1]
            assert len(run["rows"]) == 11
            run.update(round=round_, order=order.index(kind))
            result["runs"].append(run)
            print(
                json.dumps(
                    {
                        "round": round_,
                        "kind": kind,
                        "median_ms": float(
                            np.median(
                                [r["ms"] for r in run["rows"] if r["phase"] == "warm"]
                            )
                        ),
                        "failures": sum(not r["success"] for r in run["rows"]),
                    }
                ),
                flush=True,
            )
    result["load_after"] = subprocess.check_output(["uptime"], text=True).strip()
    result["summary"] = []
    for kind in CONFIGS:
        runs = [r for r in result["runs"] if r["kind"] == kind]
        trials = [r for run in runs for r in run["rows"] if r["phase"] == "warm"]
        times = [r["ms"] for r in trials]
        result["summary"].append(
            {
                "kind": kind,
                "voice": runs[0]["voice"],
                "warm_trials": len(trials),
                "failures": sum(not r["success"] for r in trials),
                "median_ms": float(np.median(times)),
                "p95_ms": float(np.percentile(times, 95)),
                "range_ms": [min(times), max(times)],
                "round_median_ms": [
                    float(
                        np.median(
                            [r["ms"] for r in run["rows"] if r["phase"] == "warm"]
                        )
                    )
                    for run in runs
                ],
                "peak_process_rss_bytes": max(run["peak_rss_bytes"] for run in runs),
            }
        )
    (ROOT / "docs/performance/voice-evaluation/cpu-screening.json").write_text(
        json.dumps(result, indent=2) + "\n"
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--one", choices=["kitten08", "paradee12"])
    args = parser.parse_args()
    one(args.one) if args.one else main()
