#!/usr/bin/env python3
"""Publish complete, matched screening results and the app's predicted ratings."""

import json
from pathlib import Path
import shutil

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "docs/performance/voice-evaluation"


def main():
    data = json.loads((OUT / "results.json").read_text())
    voices = data["voices"]
    assert len(voices) == 21 and len(data["rows"]) == 294, "Incomplete evaluation"
    assert len({row["id"] for row in data["rows"]}) == 294, "Duplicate recordings"
    assert all(v["recordings"] == 12 and v["failures"] == 0 for v in voices), (
        "Do not publish incomplete scores"
    )
    names = {
        "native-kitten01": "Kitten 0.1 · app",
        "native-kokoro": "Kokoro Heart · app",
        "kitten08": "Kitten 0.8 · experimental",
        "paradee12": "Paradee 1.2 · experimental",
    }
    rows = sorted(voices, key=lambda v: v["naturalness_median"], reverse=True)
    md = [
        "# Local voice screening — 2026-10-30",
        "",
        "All **21 voices** completed the same **12 passages at 1×**, plus two whole-passage controls: **294 recordings**, no generation/scoring failures. Runs used this M4 Mac. Ratings are **UTMOS22 predicted naturalness**, not a human listening score. ASR word error is a separate intelligibility proxy; lower is better. Neither measures dyslexia-specific reading comprehension.",
        "",
        "Current app voices were exported through the production native frontend and 32/180-character chunk policy. The two newer models used their author Python frontends with the same outer chunks. This compares complete experimental pipelines, not identical frontends or native app adapters. The first 32-character boundary can cut prosodic context. Source-specific cleanup, punctuation handling, style selection and tail trimming remain part of each result.",
        "",
        "| Voice | Pipeline | Predicted naturalness, median (IQR) | ASR word error |",
        "| --- | --- | ---: | ---: |",
    ]
    md[6:6] = [
        "The current app frontend has a concrete pronunciation defect: its lexicon path omits unsupported tokens. Names, accented words, acronyms and numbers dominate the app's error rate. For example, the native Heart transcript loses PDF/BBC/URL/CPU and the amounts/time from the number passage. Backend logs corroborate unknown-token handling. These scores rate the current app pipeline, not the best possible quality of the underlying Kokoro/Kitten voices. Fix the frontend and rerun before judging those model families or promoting a replacement.",
        "",
    ]
    for v in rows:
        lo, hi = v["naturalness_iqr"]
        md.append(
            f"| {v['name']} | {names[v['candidate']]} | {v['naturalness_median']:.2f} ({lo:.2f}–{hi:.2f}) | {v['wer']:.1%} |"
        )
    md += [
        "",
        "The predictor's nominal scale is 1–5; values are not percentages. IQR describes variation across passages, not confidence from human listeners. Small differences and overlapping ranges do not establish a preference. No subjective listening or Heart speaker-similarity verdict is claimed. Paradee's `af_heart` is a distilled Heart-derived preset, distinct from authentic Kokoro Heart.",
        "",
        "## What the error rate means",
        "",
        "Whisper small.en (faster-whisper, local CPU int8, beam 5, temperature zero, no VAD, no previous-text conditioning) transcribed the saved 24 kHz recordings after conversion to 16 kHz PCM. A pinned official Whisper English normalizer is applied to both the declared spoken reference and transcript. PDF/BBC/URL/CPU spellings are joined, digit clock times are joined, numeric ordinal suffixes are removed, and diacritics are ignored. The literal text, spoken reference, transcripts, substitutions, deletions and insertions are retained in `results.json`. Recognition and proper-name spelling errors can inflate WER; it is not a direct count of mispronunciations. Homograph pronunciation is not established by a correct transcript.",
        "",
        "Missing, empty, nonfinite and silent recordings are failures rather than omitted observations. Clipped samples, duration and leading/trailing signal threshold crossings are retained per passage. No loudness normalization, silence trimming or selective passage removal was applied during scoring. Waveforms were saved as 24 kHz PCM16 for every pipeline.",
        "",
        "## Downloads, runtime and adoption",
        "",
        "The new models are downloaded under `.cache/voice-eval`: Kitten nano 0.8 int8 (27,649,561 bytes for source graph/voices/config) and Paradee 1.2 int8 (8,463,946 bytes for graph/config). Those sums exclude frontend, licenses, Python environment and evaluation tools. The full experimental environment is not shipped in the app. Hashes, pinned revisions and exact downloaded byte counts are in `results.json`.",
        "",
        "Experimental generation used ONNX Runtime 1.24.4 CPU, two intra-op threads and one inter-op thread. Existing voices used the app's sherpa 2.0.1 / ORT 1.28.2 runtime. Candidate `generated-*.json` files retain diagnostic CPU generation times and Python-process high-water RSS, but those runs overlapped scoring/build activity. They are not uncontended app-startup benchmarks, ready-state memory measurements or M1 certification. Do not compare them directly to the app's request-to-render latency bars.",
        "",
        "The shipping catalog and default engine have not been replaced. A candidate still needs a native frontend/runtime adapter, license/notices review of the complete distribution, app onset/cancellation tests, whole-app resource checks, and M1/8 GB/macOS 14 validation. Authentic Heart's existing M4 startup p95 remains about 1.07 seconds, exceeding the 500 ms warm target.",
        "",
        "## Reproduce",
        "",
        "Use the isolated environment pinned in `requirements.txt`. It is a development evaluation dependency set, not an app dependency. Install the spaCy English model at the URL in that file. Fetch SpeechMOS v1.2.0 at commit `ed25eacbfa42b99156c36ebec67a733b5dbb9b79` into `.cache/voice-eval/SpeechMOS`, and retain its MIT license.",
        "",
        "```sh",
        "python3 scripts/prepare_voice_eval.py",
        "cargo build --release --no-default-features --bin voice-export",
        ".cache/voice-eval-env/bin/python scripts/evaluate_voices.py generate",
        ".cache/voice-eval-env/bin/python scripts/evaluate_voices.py score",
        ".cache/voice-eval-env/bin/python scripts/evaluate_voices.py summarize",
        "python3 scripts/report_voice_evaluation.py",
        "```",
        "",
        "The generation and scoring stages resume completed recordings. To obtain a new stochastic run, move the evaluation cache to a separate directory first. ONNX model randomness was left as published; scoring seeds are fixed. A normalizer API mismatch was caught in an initial harness attempt; those invalid rows are archived separately and absent from the complete results. Final WER is recomputed uniformly from all retained transcripts.",
        "",
        "Fresh runs capture stage inputs before execution. Resume checks the corpus, evaluator code, package versions, model/scorer artifacts and retained WAV hashes; changed inputs are rejected rather than attached to old scores. This original run predates those guards. Its provenance is explicitly marked `post_run_verified_import`: an audit checked the published corpus/binary/checkpoint hashes, downloaded artifacts, all retained waveforms, transcripts and scores. It freezes that evidence without claiming to reconstruct missing execution-time code fingerprints. The frozen corpus, including literal inputs and spoken references, is embedded in `results.json`.",
        "",
        "[UTMOS wrapper and metric description](https://github.com/tarepan/SpeechMOS/tree/v1.2.0), [Kitten reference frontend](https://github.com/KittenML/KittenTTS/blob/0.8.1/kittentts/onnx_model.py), [Paradee reference frontend](https://github.com/sahilmahendrakar/paradee/blob/v1.2/paradee/tts.py). Candidate licenses and integration details are in [the model research](../../current-tts-models.md).",
        "",
        "## Auditions",
        "",
        "The same opening prose is preserved below for every voice. These files are generated evaluation samples, not evidence of a listening study.",
        "",
    ]
    audio = OUT / "audio"
    audio.mkdir(exist_ok=True)
    for v in rows:
        row = next(
            r
            for r in data["rows"]
            if r["candidate"] == v["candidate"]
            and r["name"] == v["name"]
            and r["passage"] == "prose"
            and not r["whole_control"]
        )
        dest = audio / (row["id"] + ".wav")
        shutil.copyfile(row["wav"], dest)
        md.append(f"- [{v['name']} — {names[v['candidate']]}](audio/{dest.name})")
    (OUT / "REPORT.md").write_text("\n".join(md) + "\n")
    shutil.copyfile(
        ROOT / ".cache/voice-eval/requirements.txt", OUT / "requirements.txt"
    )
    ratings_path = ROOT / "resources/voice-ratings.json"
    ratings = json.loads(ratings_path.read_text())
    for item in ratings["voices"]:
        v = next(v for v in voices if v["catalog_id"] == item["voice"])
        item.update(
            naturalness_median=v["naturalness_median"],
            naturalness_iqr=v["naturalness_iqr"],
            asr_wer=v["wer"],
            quality_samples=v["recordings"],
        )
    ratings["quality_method"] = {
        "kind": "predicted_naturalness",
        "predictor": "UTMOS22 strong / SpeechMOS v1.2.0",
        "checkpoint_sha256": data["utmos22_checkpoint_sha256"],
        "corpus_sha256": data["corpus_sha256"],
        "source": "docs/performance/voice-evaluation/results.json",
        "samples_per_voice": 12,
        "speed": 1,
        "human_listener_study": False,
    }
    ratings["quality_detail"] = (
        "Median prediction across 12 passages at 1×. This build skipped some names, numbers and accented words. No human listening study."
    )
    ratings_path.write_text(json.dumps(ratings, indent=2) + "\n")
    chart_rows = [dict(v, pipeline=names[v["candidate"]]) for v in rows]
    template = """<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><style>
body{margin:0;color:var(--foreground);font-family:var(--font-sans);font-size:13px}h2{font-size:18px;margin:0 0 8px;font-weight:600}p{color:var(--muted-foreground);line-height:1.5;margin:8px 0 14px}select{font:inherit;color:var(--foreground);background:var(--secondary);border:1px solid var(--border);border-radius:var(--radius);padding:6px 9px}.tools{display:flex;gap:8px;margin-bottom:14px}.axis,.row{display:grid;grid-template-columns:minmax(125px,1.1fr) minmax(140px,2fr) 48px 56px;gap:10px;align-items:center}.axis{color:var(--muted-foreground);font-size:11px;margin-bottom:8px}.ticks{display:flex;justify-content:space-between}.row{min-height:39px;padding:3px 0;border-bottom:1px solid var(--border)}.name{font-weight:500}.model{display:block;color:var(--muted-foreground);font-size:10px;margin-top:2px}.track{height:7px;background:var(--secondary);position:relative;border-radius:4px}.bar{position:absolute;height:7px;border-radius:4px;opacity:.55}.iqr{position:absolute;height:3px;top:2px;background:var(--foreground);opacity:.8}.dot{position:absolute;height:11px;width:3px;top:-2px;background:var(--foreground);border-radius:2px}.value,.wer{font-variant-numeric:tabular-nums;text-align:right}.note{font-size:11px}.legend{display:flex;gap:14px;flex-wrap:wrap;margin:12px 0;color:var(--muted-foreground);font-size:11px}.key:before{content:"";display:inline-block;width:8px;height:8px;border-radius:50%;background:var(--c);margin-right:5px}@media(max-width:480px){.axis,.row{grid-template-columns:105px minmax(85px,1fr) 34px 42px;gap:7px}.row{min-height:46px}.model{font-size:9px}}
</style></head><body><h2>Voice quality · local automated screening</h2><p>21 voices, 12 passages each, 1×. Naturalness is a model prediction on a 1–5 scale. Word error comes from local speech recognition; lower is better.</p><div class="tools"><select id="filter" aria-label="Show pipelines"><option value="all">All voices</option><option value="app">App voices</option><option value="new">New models</option></select><select id="sort" aria-label="Sort voices"><option value="quality">Predicted naturalness</option><option value="wer">Word error</option><option value="name">Name</option></select></div><div class="axis"><span>Voice / pipeline</span><span class="ticks"><span>1</span><span>2</span><span>3</span><span>4</span><span>5</span></span><span>Score</span><span style="text-align:right">WER</span></div><div id="rows"></div><div class="legend"><span class="key" style="--c:var(--chart-1)">Current app</span><span class="key" style="--c:var(--chart-2)">Kitten 0.8</span><span class="key" style="--c:var(--chart-3)">Paradee 1.2</span></div><p class="note">The thin interval shows the middle half of passage scores; the tick is the median. Small differences are exploratory. App recordings contain a skipped-word defect. No human listening study or M1 certification. New candidates use experimental Python frontends; app voices use the production native frontend.</p><script>const data=__DATA__;const filter=document.querySelector('#filter'),sort=document.querySelector('#sort');function render(){let items=data.filter(d=>filter.value==='all'||(filter.value==='app')===d.candidate.startsWith('native'));items.sort((a,b)=>sort.value==='name'?a.name.localeCompare(b.name):sort.value==='wer'?a.wer-b.wer:b.naturalness_median-a.naturalness_median);document.querySelector('#rows').replaceChildren(...items.map(d=>{const row=document.createElement('div');row.className='row';const identity=document.createElement('div');identity.className='name';identity.textContent=d.name;const model=document.createElement('span');model.className='model';model.textContent=d.pipeline;identity.append(model);const track=document.createElement('div');track.className='track';const color=d.candidate.startsWith('native')?'var(--chart-1)':d.candidate==='kitten08'?'var(--chart-2)':'var(--chart-3)';const bar=document.createElement('div');bar.className='bar';bar.style.width=((d.naturalness_median-1)/4*100)+'%';bar.style.background=color;const iqr=document.createElement('div');iqr.className='iqr';iqr.style.left=((d.naturalness_iqr[0]-1)/4*100)+'%';iqr.style.width=((d.naturalness_iqr[1]-d.naturalness_iqr[0])/4*100)+'%';const dot=document.createElement('div');dot.className='dot';dot.style.left=((d.naturalness_median-1)/4*100)+'%';track.append(bar,iqr,dot);track.title='Median '+d.naturalness_median.toFixed(2)+'; IQR '+d.naturalness_iqr.map(n=>n.toFixed(2)).join('–');const value=document.createElement('div');value.className='value';value.textContent=d.naturalness_median.toFixed(2);const wer=document.createElement('div');wer.className='wer';wer.textContent=(d.wer*100).toFixed(1)+'%';row.append(identity,track,value,wer);return row}))}filter.onchange=render;sort.onchange=render;render();</script></body></html>"""
    (OUT / "chart.html").write_text(
        template.replace("__DATA__", json.dumps(chart_rows))
    )
    print(f"Published {len(voices)} voice results and {len(data['rows'])} recordings")


if __name__ == "__main__":
    main()
