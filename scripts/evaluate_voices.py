#!/usr/bin/env python3
"""Local full-recording evaluation. Generated audio/text never leaves this machine.

Run prepare_voice_eval.py first, then generate, score, and summarize in order.
Experimental candidates use reviewed author frontends, NOT a shipping app adapter.
"""

import argparse
import json
import os
from pathlib import Path
import random
import re
import resource
import subprocess
import sys
import time
import unicodedata
from voice_eval_provenance import audit_existing, digest, guard

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / ".cache/voice-eval"
OUTPUT = ROOT / "docs/performance/voice-evaluation"
CORPUS = ROOT / "docs/performance/voice-corpus.json"
BASE_NAMES = [
    "Arthur",
    "Clara",
    "Elliot",
    "Maya",
    "Theo",
    "Nora",
    "Owen",
    "Ruby",
    "Heart",
]


def sha(path):
    return digest(path)


def chunks(text):
    """Mirror text::Chunks defaults, asserting against native exports after generation."""
    first = True
    while text.strip():
        text = text.lstrip()
        limit = 32 if first else 180
        first = False
        end, whitespace = len(text), None
        for index, ch in enumerate(text):
            if index >= limit:
                end = whitespace if whitespace else index
                break
            if ch.isspace():
                whitespace = index
            if index >= 18 and ch in "!?;\n":
                end = index + 1
                break
            if index >= 18 and ch == ".":
                word = text[:index].split()[-1] if text[:index].split() else ""
                abbreviation = len(word) == 1 or word.lower() in [
                    "mr",
                    "mrs",
                    "ms",
                    "dr",
                    "prof",
                    "st",
                    "vs",
                    "etc",
                    "e.g",
                    "i.e",
                ]
                if not abbreviation and (
                    index + 1 == len(text) or text[index + 1].isspace()
                ):
                    end = index + 1
                    break
        yield text[:end].strip()
        text = text[end:]


def candidate_engine(kind):
    import espeakng_loader
    from phonemizer.backend.espeak.wrapper import EspeakWrapper

    EspeakWrapper.set_library(espeakng_loader.get_library_path())
    os.environ["ESPEAK_DATA_PATH"] = espeakng_loader.get_data_path()
    sys.path.insert(0, str(CACHE))
    import onnxruntime as ort

    if kind == "kitten08":
        from kitten_code.onnx_model import KittenTTS_1_Onnx

        config = json.loads((CACHE / "kitten08/config.json").read_text())
        original = ort.InferenceSession

        def session(path, *args, **kwargs):
            options = ort.SessionOptions()
            options.intra_op_num_threads = 2
            options.inter_op_num_threads = 1
            return original(path, options, providers=["CPUExecutionProvider"])

        ort.InferenceSession = session
        try:
            engine = KittenTTS_1_Onnx(
                str(CACHE / "kitten08/kitten_tts_nano_v0_8.onnx"),
                str(CACHE / "kitten08/voices.npz"),
                config["speed_priors"],
                config["voice_aliases"],
            )
        finally:
            ort.InferenceSession = original

        def generate(text, voice):
            # Reference generate() performs preprocessing, sentence handling and tail trim.
            return engine.generate(text, voice=voice, speed=1.0)

        return engine, generate, list(config["voice_aliases"])
    from paradee_code.tts import Paradee

    engine = Paradee(
        version="1.2",
        model_path=str(CACHE / "paradee12/onnx/paradee_int8.onnx"),
        config_path=str(CACHE / "paradee12/config.json"),
        threads=2,
    )
    return (
        engine,
        lambda text, voice: engine(text, speed=1.0, voice=voice),
        engine.voices,
    )


def generate_one(kind, voice):
    guard("generation")
    import numpy as np
    import soundfile as sf

    audio_dir = CACHE / "audio"
    audio_dir.mkdir(parents=True, exist_ok=True)
    start = time.perf_counter()
    engine, generate, _ = candidate_engine(kind)
    loaded_ms = (time.perf_counter() - start) * 1000
    rows = []
    for passage in json.loads(CORPUS.read_text()):
        for control in [False, True]:
            if control and passage["id"] not in ["prose", "paragraph"]:
                continue
            id_ = f"{kind}-{voice}-{passage['id']}" + ("-whole" if control else "")
            pieces = [passage["text"]] if control else list(chunks(passage["text"]))
            row = {
                "event": "recording",
                "id": id_,
                "passage": passage["id"],
                "whole_control": control,
                "candidate": kind,
                "name": voice,
                "chunks": pieces,
                "speed": 1,
                "load_ms": loaded_ms,
            }
            try:
                audio, times = [], []
                for piece in pieces:
                    start = time.perf_counter()
                    pcm = np.asarray(generate(piece, voice), dtype=np.float32).reshape(
                        -1
                    )
                    times.append((time.perf_counter() - start) * 1000)
                    if not len(pcm) or not np.isfinite(pcm).all():
                        raise ValueError("Empty/nonfinite audio")
                    audio.append(pcm)
                audio = np.concatenate(audio)
                path = audio_dir / f"{id_}.wav"
                sf.write(path, audio, 24000, subtype="PCM_16")
                row["wav_sha256"] = sha(path)
                row.update(
                    success=True,
                    wav=str(path),
                    samples=len(audio),
                    duration_s=len(audio) / 24000,
                    peak=float(np.abs(audio).max()),
                    clipped_samples=int((np.abs(audio) >= 1).sum()),
                    chunk_ms=times,
                    total_ms=sum(times),
                )
            except Exception as error:
                row.update(success=False, error=f"{type(error).__name__}: {error}")
            rows.append(row)
            print(
                json.dumps(
                    {k: row.get(k) for k in ["id", "success", "error", "duration_s"]}
                ),
                flush=True,
            )
    # A small matched CPU microbenchmark, warm text frontend, no playback/device.
    timing = []
    for trial in range(11):
        start = time.perf_counter()
        pcm = generate("Reading at your own pace", voice)
        ms = (time.perf_counter() - start) * 1000
        timing.append(
            {
                "trial": trial,
                "phase": "priming" if trial == 0 else "warm",
                "ms": ms,
                "samples": int(np.asarray(pcm).size),
            }
        )
    result = {
        "kind": kind,
        "voice": voice,
        "load_ms": loaded_ms,
        "peak_rss_bytes": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
        "timing": timing,
        "rows": rows,
    }
    (CACHE / f"generated-{kind}-{voice}.json").write_text(
        json.dumps(result, indent=2) + "\n"
    )
    del engine


def generate_all():
    CACHE.mkdir(parents=True, exist_ok=True)
    guard("generation")
    binary = (
        Path(
            json.loads(
                subprocess.check_output(
                    ["cargo", "metadata", "--no-deps", "--format-version", "1"],
                    cwd=ROOT,
                )
            )["target_directory"]
        )
        / "release/voice-export"
    )
    indices = list(range(9))
    random.Random(42).shuffle(indices)
    for index in indices:
        log = CACHE / f"generated-native-{index}.jsonl"
        if log.exists():
            for row in map(json.loads, log.read_text().splitlines()):
                if row.get("event") == "recording":
                    from voice_eval_provenance import verify_audio

                    verify_audio(row)
            continue
        command = [
            str(binary),
            "--engine",
            "kokoro-heart" if index == 8 else "kitten-cpu",
            "--voice",
            str(3 if index == 8 else index),
            "--prefix",
            f"native-{index}",
            "--output",
            str(CACHE / "audio"),
            "--corpus",
            str(CORPUS),
        ]
        process = subprocess.run(
            command, cwd=ROOT, capture_output=True, text=True, timeout=300
        )
        (CACHE / f"native-{index}.stderr").write_text(process.stderr)
        if process.returncode:
            raise RuntimeError(f"Native voice {index} failed: {process.stderr[-1000:]}")
        rows = [
            json.loads(line)
            for line in process.stdout.splitlines()
            if line.startswith("{")
        ]
        for row in rows:
            if row.get("event") == "recording" and row["success"]:
                row["wav_sha256"] = sha(row["wav"])
        log.write_text("".join(json.dumps(row) + "\n" for row in rows))
        print(f"Exported {BASE_NAMES[index]}", flush=True)
    voices = [
        ("kitten08", voice)
        for voice in [
            "Bella",
            "Jasper",
            "Luna",
            "Bruno",
            "Rosie",
            "Hugo",
            "Kiki",
            "Leo",
        ]
    ] + [
        ("paradee12", voice)
        for voice in ["af_heart", "bf_isabella", "am_puck", "bm_fable"]
    ]
    random.Random(42).shuffle(voices)
    for kind, voice in voices:
        if (CACHE / f"generated-{kind}-{voice}.json").exists():
            from voice_eval_provenance import verify_audio

            for row in json.loads(
                (CACHE / f"generated-{kind}-{voice}.json").read_text()
            )["rows"]:
                verify_audio(row)
            continue
        subprocess.run(
            [sys.executable, __file__, "one", "--kind", kind, "--voice", voice],
            cwd=ROOT,
            check=True,
            timeout=600,
        )
    native = [
        json.loads(line)
        for line in (CACHE / "generated-native-0.jsonl").read_text().splitlines()
        if line.startswith("{")
    ]
    passages = {p["id"]: p for p in json.loads(CORPUS.read_text())}
    for row in native:
        if row.get("event") == "recording" and not row["whole_control"]:
            assert row["chunks"] == list(chunks(passages[row["passage"]]["text"])), row[
                "id"
            ]


def all_rows():
    rows = []
    for index, name in enumerate(BASE_NAMES):
        for line in (
            (CACHE / f"generated-native-{index}.jsonl").read_text().splitlines()
        ):
            row = json.loads(line)
            if row["event"] != "recording":
                continue
            row.update(
                candidate="native-kokoro" if index == 8 else "native-kitten01",
                name=name,
                catalog_id=index,
            )
            rows.append(row)
    for path in sorted(CACHE.glob("generated-*.json")):
        rows.extend(json.loads(path.read_text())["rows"])
    return rows


def normalized(text):
    # Official Whisper normalizer, downloaded at a pinned tag by evaluation setup.
    sys.path.insert(0, str(CACHE))
    from whisper_normalizers.english import EnglishTextNormalizer

    # Predeclared equivalent spellings for this corpus, applied to both sides.
    for acronym in ["pdf", "bbc", "url", "cpu"]:
        text = re.sub(
            r"\b" + r"[.\s-]*".join(acronym) + r"\b", acronym, text, flags=re.I
        )
    text = re.sub(r"\b(\d{1,2}):(\d{2})\b", r"\1\2", text)
    text = re.sub(
        r"(\bat\s+)(\d{1,2})\s*[.,:]\s*(\d{2})\b", r"\1\2\3", text, flags=re.I
    )
    value = EnglishTextNormalizer()(text)
    value = re.sub(r"\b(\d+)(?:st|nd|rd|th)\b", r"\1", value)
    value = unicodedata.normalize("NFKD", value)
    return "".join(c for c in value if not unicodedata.combining(c)).split()


def edit_counts(reference, hypothesis):
    # Counts are (substitutions, deletions, insertions); deterministic tie policy.
    previous = [(i, (0, 0, i)) for i in range(len(hypothesis) + 1)]
    for r, word in enumerate(reference, 1):
        current = [(r, (0, r, 0))]
        for h, other in enumerate(hypothesis, 1):
            if word == other:
                current.append(previous[h - 1])
                continue
            choices = []
            for cost, counts, category in [
                (previous[h - 1][0], previous[h - 1][1], 0),
                (previous[h][0], previous[h][1], 1),
                (current[h - 1][0], current[h - 1][1], 2),
            ]:
                updated = list(counts)
                updated[category] += 1
                choices.append((cost + 1, tuple(updated)))
            current.append(min(choices, key=lambda x: x[0]))
        previous = current
    return previous[-1][1]


def score():
    guard("generation")
    scoring = guard("scoring")
    from voice_eval_provenance import verify_audio
    import numpy as np
    import soundfile as sf
    import torch
    from faster_whisper import WhisperModel

    torch.set_num_threads(2)
    torch.manual_seed(42)
    random.seed(42)
    np.random.seed(42)
    sys.path.insert(0, str(CACHE / "SpeechMOS"))
    from speechmos.utmos22.strong.model import UTMOS22Strong

    predictor = UTMOS22Strong().eval()
    predictor.load_state_dict(
        torch.load(CACHE / "utmos22.pt", map_location="cpu", weights_only=True)
    )
    recognizer = WhisperModel(
        str(CACHE / "asr"),
        device="cpu",
        compute_type="int8",
        cpu_threads=2,
        num_workers=1,
    )
    passages = {p["id"]: p for p in json.loads(CORPUS.read_text())}
    existing = {}
    score_path = CACHE / "scored.jsonl"
    if score_path.exists():
        existing = {
            r["id"]: r for r in map(json.loads, score_path.read_text().splitlines())
        }
    rows = all_rows()
    random.Random(42).shuffle(rows)
    with score_path.open("a") as output:
        for row in rows:
            verify_audio(row)
            if row["id"] in existing:
                saved = existing[row["id"]]
                verify_audio(saved)
                if saved.get("scoring_fingerprint") != scoring[
                    "fingerprint"
                ] or saved.get("wav_sha256") != row.get("wav_sha256"):
                    raise RuntimeError(f"Scoring provenance changed: {row['id']}")
                continue
            row["scoring_fingerprint"] = scoring["fingerprint"]
            if row["success"]:
                try:
                    audio, rate = sf.read(row["wav"], dtype="float32")
                    active = np.flatnonzero(np.abs(audio) >= 0.00316227766)
                    row.update(
                        wav_sha256=sha(row["wav"]),
                        leading_silence_ms=float(active[0] / rate * 1000)
                        if len(active)
                        else None,
                        trailing_silence_ms=float(
                            (len(audio) - 1 - active[-1]) / rate * 1000
                        )
                        if len(active)
                        else None,
                    )
                    if not len(active):
                        raise ValueError("Silent recording")
                    with torch.inference_mode():
                        row["utmos22"] = float(
                            predictor(torch.from_numpy(audio).unsqueeze(0), rate)[0]
                        )
                    # Feed PCM directly; faster-whisper requires 16 kHz arrays.
                    # This also avoids a PyAV 19 path-decoder API incompatibility.
                    import torchaudio

                    recognition_audio = torchaudio.functional.resample(
                        torch.from_numpy(audio), rate, 16000
                    ).numpy()
                    segments, _ = recognizer.transcribe(
                        recognition_audio,
                        language="en",
                        beam_size=5,
                        temperature=0,
                        condition_on_previous_text=False,
                        vad_filter=False,
                    )
                    row["transcript"] = " ".join(s.text.strip() for s in segments)
                    reference = normalized(passages[row["passage"]]["expected"])
                    hypothesis = normalized(row["transcript"])
                    row.update(
                        reference_normalized=reference, hypothesis_normalized=hypothesis
                    )
                    sub, delete, insert = edit_counts(reference, hypothesis)
                    row.update(
                        substitutions=sub,
                        deletions=delete,
                        insertions=insert,
                        reference_words=len(reference),
                        wer=(sub + delete + insert) / max(1, len(reference)),
                    )
                except Exception as error:
                    row.update(
                        success=False, scoring_error=f"{type(error).__name__}: {error}"
                    )
            output.write(json.dumps(row) + "\n")
            output.flush()
            print(
                json.dumps(
                    {
                        k: row.get(k)
                        for k in ["id", "success", "utmos22", "wer", "scoring_error"]
                    }
                ),
                flush=True,
            )


def summarize():
    import numpy as np

    generation = guard("generation")
    scoring = guard("scoring")
    from voice_eval_provenance import verify_audio

    rows = [
        json.loads(line) for line in (CACHE / "scored.jsonl").read_text().splitlines()
    ]
    passages = {p["id"]: p for p in generation["metadata"]["corpus"]}
    # Recompute from retained transcripts so normalization changes never require
    # rerunning recognition or selectively dropping inconvenient recordings.
    for row in rows:
        verify_audio(row)
        if row.get("scoring_fingerprint") != scoring["fingerprint"]:
            raise RuntimeError(f"Scoring provenance changed: {row['id']}")
        if row["success"]:
            reference = normalized(passages[row["passage"]]["expected"])
            hypothesis = normalized(row["transcript"])
            sub, delete, insert = edit_counts(reference, hypothesis)
            row.update(
                reference_normalized=reference,
                hypothesis_normalized=hypothesis,
                substitutions=sub,
                deletions=delete,
                insertions=insert,
                reference_words=len(reference),
                wer=(sub + delete + insert) / max(1, len(reference)),
            )
    groups = {}
    for row in rows:
        key = f"{row['candidate']}:{row['name']}"
        groups.setdefault(key, []).append(row)
    summaries = []
    for key, records in sorted(groups.items()):
        primary = [r for r in records if not r["whole_control"]]
        valid = [r for r in primary if r["success"]]
        scores = [r["utmos22"] for r in valid]
        words = sum(r["reference_words"] for r in valid)
        counts = {
            k: sum(r[k] for r in valid)
            for k in ["substitutions", "deletions", "insertions"]
        }
        item = {
            "key": key,
            "name": records[0]["name"],
            "candidate": records[0]["candidate"],
            "catalog_id": records[0].get("catalog_id"),
            "recordings": len(primary),
            "failures": len(primary) - len(valid),
            "naturalness_median": float(np.median(scores)) if scores else None,
            "naturalness_mean": float(np.mean(scores)) if scores else None,
            "naturalness_iqr": [float(x) for x in np.percentile(scores, [25, 75])]
            if scores
            else None,
            "naturalness_range": [min(scores), max(scores)] if scores else None,
            "reference_words": words,
            **counts,
            "wer": sum(counts.values()) / words if words else None,
            "clipped_samples": sum(r.get("clipped_samples", 0) for r in primary),
        }
        summaries.append(item)
    OUTPUT.mkdir(parents=True, exist_ok=True)
    report = {
        "date": generation["created_utc"][:10],
        "hardware": generation["metadata"]["hardware"],
        "scope": "Local automated screening at 1x. Existing voices use native production frontend/chunks; candidates use author Python frontends with matching outer chunks. No human listener study, app playback or M1 certification.",
        "normalization": "Whisper v20250625 English normalizer; diacritics removed; PDF/BBC/URL/CPU joined; digit clock times joined; numeric ordinal suffixes removed, with identical rules on reference and hypothesis. Proper-name spelling remains ASR-sensitive.",
        "corpus": generation["metadata"]["corpus"],
        **{
            key: generation["metadata"][key]
            for key in ["corpus_sha256", "export_binary_sha256", "downloads"]
        },
        **{
            key: scoring["metadata"][key]
            for key in ["utmos22_checkpoint_sha256", "utmos22_code_commit"]
        },
        "provenance": {"generation": generation, "scoring": scoring},
        "voices": summaries,
        "rows": rows,
    }
    (OUTPUT / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    for item in sorted(
        summaries, key=lambda r: r["naturalness_median"] or 0, reverse=True
    ):
        print(json.dumps(item), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "stage", choices=["generate", "one", "score", "summarize", "audit-existing"]
    )
    parser.add_argument("--kind", choices=["kitten08", "paradee12"])
    parser.add_argument("--voice")
    args = parser.parse_args()
    if args.stage == "one":
        generate_one(args.kind, args.voice)
    else:
        {
            "generate": generate_all,
            "score": score,
            "summarize": summarize,
            "audit-existing": audit_existing,
        }[args.stage]()
