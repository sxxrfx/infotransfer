#!/usr/bin/env python3
"""Interleave real playback experiments. Counts process failures as failures.

Output timestamps are software proxies. CPAL's macOS playback timestamp adds
an assumed buffer duration; it does not measure physical speaker latency.
"""
import argparse
import hashlib
import json
import math
import os
import platform
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def percentile(values, fraction):
    values = sorted(values)
    return values[min(len(values)-1, math.ceil(len(values)*fraction)-1)]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rounds", type=int, default=5)
    parser.add_argument("--runs", type=int, default=6)
    parser.add_argument("--float-model", type=Path, default=ROOT/"resources/speech-kitten-cpu")
    parser.add_argument("--device")
    parser.add_argument("--bucket", default="typical")
    parser.add_argument("--replace", action="store_true")
    parser.add_argument("--cold", action="store_true")
    parser.add_argument("--float-only", action="store_true")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    metadata = json.loads(subprocess.check_output(["cargo","metadata","--no-deps","--format-version","1"],cwd=ROOT))
    binary = Path(metadata["target_directory"])/"release/reading-bench"
    configurations = {"original":ROOT/"resources/speech-kitten", "float_conv":args.float_model.resolve()}
    if args.float_only:
        del configurations["original"]
    result = {"scope":"real_audio_callback; excludes_capture_GPUI_physical_device_latency", "started":time.time(),
              "platform":platform.platform(),"hardware":subprocess.check_output(["sysctl","-n","machdep.cpu.brand_string"],text=True).strip(),
              "load":subprocess.check_output(["uptime"],text=True).strip(), "binary_sha256":hashlib.sha256(binary.read_bytes()).hexdigest(),
              "model_sha256":{label:hashlib.sha256((path/("model.cpu.onnx" if label=="float_conv" else "model.fp16.onnx")).read_bytes()).hexdigest() for label,path in configurations.items()},
              "bucket":args.bucket,"replacement":args.replace,"cold":args.cold,"runs":[],"summary":{}}
    for index in range(args.rounds):
        order = list(configurations)
        if index % 2:
            order.reverse()
        for label in order:
            command = [str(binary),"--engine","kitten-cpu" if label=="float_conv" else "kitten","--runs",str(args.runs),"--bucket",args.bucket]
            if args.replace: command.append("--replace")
            if args.cold: command.append("--cold")
            if args.device: command += ["--device",args.device]
            environment = dict(os.environ, INFOTRANSFER_SPEECH_DIR=str(configurations[label]))
            try:
                process = subprocess.run(command,capture_output=True,text=True,timeout=180,env=environment,cwd=ROOT)
                rows = [json.loads(line) for line in process.stdout.splitlines() if line.startswith("{")]
                trials = [row for row in rows if row.get("event")=="trial"]
                failures = sum(not row.get("success",False) for row in trials)
                if process.returncode or len(trials)!=args.runs:
                    failures += 1
                run = {"round":index,"configuration":label,"command":command,"exit":process.returncode,
                       "failures":failures,"rows":rows,"stderr":process.stderr}
            except subprocess.TimeoutExpired:
                run = {"round":index,"configuration":label,"command":command,"exit":None,"failures":1,"rows":[],"error":"process timeout"}
            result["runs"].append(run)
            print(f"Round {index+1}: {label}; {run['failures']} failures",flush=True)
            args.output.parent.mkdir(parents=True,exist_ok=True)
            args.output.write_text(json.dumps(result,indent=2)+"\n")
    for label in configurations:
        runs = [run for run in result["runs"] if run["configuration"]==label]
        rows = [row for run in runs for row in run["rows"] if row.get("event")=="trial" and row.get("success")
                and (args.cold or row.get("phase")=="warm")]
        summary = {"samples":len(rows),"failures":sum(run["failures"] for run in runs)}
        for field in ["control_feedback_ms","first_pcm_ms","first_render_ms","first_signal_render_ms","first_signal_playback_estimate_ms","first_playback_estimate_ms","stop_silence_estimate_ms","resident_bytes","peak_resident_bytes"]:
            values = [row[field] for row in rows if row.get(field) is not None]
            if values:
                summary[field] = {"p50":percentile(values,.5),"p95":percentile(values,.95),"min":min(values),"max":max(values)}
        result["summary"][label] = summary
    args.output.write_text(json.dumps(result,indent=2)+"\n")
    print(json.dumps(result["summary"],indent=2))
    if any(run["failures"] for run in result["runs"]):
        raise SystemExit("Experiment failed; do not report successful timing rows as a pass")


if __name__=="__main__":
    main()
