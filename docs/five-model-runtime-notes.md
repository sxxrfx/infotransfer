# Five-model local runtime notes

Checked **2026-10-30**, for an **Apple M4 / 16 GB Mac**. These are source findings and proposed operator commands, **not successful installation, inference, memory, or latency results**. Only documentation, source text, and package/model metadata were read; no weights were downloaded or run. Scope is exactly the five selected checkpoints.

## Capability flags

`true` = an author-supported control/path exists; `false` = no supported path established for this released checkpoint/API, not proof that an experiment can never work. `conditional` names a narrower, reference-dependent capability. **Reference-style imitation means conditioning delivery on a performance reference, beyond identifying a speaker. Voice design means creating a voice from descriptions without reference audio. Joint dialogue requires shared acoustic context across different speakers' turns.**

| Selected checkpoint | Emotion instructions | Reference-style imitation | Nonverbal sounds | Voice design | Joint dialogue generation |
| --- | --- | --- | --- | --- | --- |
| Qwen3-TTS **12Hz-1.7B-CustomVoice** | `true`: `instruct` with preset speaker | `false`: no reference input in this task | `false` / unverified: free-form instructions can be attempted, but no author-verified event contract found | `false`: separate VoiceDesign checkpoint | `false`: batches return independent WAVs |
| **VoxCPM2** | `true`: parenthesized description in target `text` | `true`: continuation mode, **audio + exact transcript** | `false` / unverified for the selected stock checkpoint | `true`: description without audio | `false`: single-utterance/reference API |
| **Fun-CosyVoice3-0.5B-2512** | `true`: `inference_instruct2` | `conditional`: zero-shot acoustic continuation; no independent style-reference control | `true`: registered events; v3 example demonstrates `[breath]` | `false`: instruction mode still needs reference audio | `false`: one reference identity per call |
| **Sopro v2 Turbo** | `false`: no semantic instruction argument | `conditional`: reference-derived style conditioning, coupled to its voice | `false` / unverified | `false`: reference required | `false`: long-text continuation is single-speaker |
| **Phonon 31M English** | `false` | `false`: public local voice files are not a raw-performance style interface | `false` / unverified | `false` **locally**; hosted Studio design is separate | `false`: utterance synthesis and sentence splitting |

These flags describe the original model/runtime paths, not features advertised by unrelated wrappers. They do not certify adherence or audio quality. Evidence and conditions follow.

### Qwen3 CustomVoice: instructions are not proof of nonverbal generation

The official [`generate_custom_voice` implementation](https://github.com/QwenLM/Qwen3-TTS/blob/022e286b98fbec7e1e916cb940cdf532cd9f488e/qwen_tts/inference/qwen3_tts_model.py#L730) validates the named speaker, tokenizes nonempty `instruct`, and forwards `instruct_ids` to generation. There is **no dedicated nonverbal parameter or event parser in that wrapper**. Its [author example](https://github.com/QwenLM/Qwen3-TTS/blob/022e286b98fbec7e1e916cb940cdf532cd9f488e/examples/test_model_12hz_custom_voice.py) demonstrates angry/happy delivery, not laughter/cough/sigh events. Thus `instruct="Laugh briefly, then speak happily."` is a permissible **experimental prompt**, not a verified nonverbal feature. Do not insert another engine's `[laugh]` syntax and label it supported. The [technical report](https://arxiv.org/html/2601.15621v1) also did not establish an event syntax in the inspected text.

The same wrapper rejects task/model-type mismatches: `generate_voice_design` requires `voice_design`; cloning belongs to Base. The [author release table](https://huggingface.co/Qwen/Qwen3-TTS-12Hz-1.7B-CustomVoice) distinguishes all three. **Do not silently download Base/VoiceDesign to make the selected CustomVoice row appear to support them.** Its list-valued speakers/texts are batching, not a conversational timeline. Also, `non_streaming_mode=False` is documented in the wrapper as simulated streaming text input, **not true streaming input or streaming generation in this API**.

### VoxCPM2: two different audio-conditioning modes

The [author examples and claims](https://github.com/OpenBMB/VoxCPM/blob/cce58a5b59303c9bd63d12f23afe6a49a4a80c59/README.md#L145) distinguish:

- **Controllable clone:** `reference_wav_path="ref.wav"`; use `text="(cheerful, slightly faster)Your sentence."` for instructions.
- **Performance continuation:** `prompt_wav_path="ref.wav", prompt_text="Exact transcript"`; optionally also supply `reference_wav_path` for identity similarity. The author explicitly claims preservation of rhythm, emotion, and style here. That claim, rather than cloning alone, supports the style flag.
- **Design:** the parenthesized description without either audio input.

No stock nonverbal-tag contract was established in the inspected author README/[inference wrapper](https://github.com/OpenBMB/VoxCPM/blob/cce58a5b59303c9bd63d12f23afe6a49a4a80c59/src/voxcpm/core.py). Do not transfer tags from a separate NVVSpeech fine-tune to `openbmb/VoxCPM2`.

For the parent experiment, the wrapper accepts `device="mps"` or `"cpu"`, `optimize=False`, and `load_denoiser=False`; generation defaults to `normalize=False`, avoiding a separate text-normalization model. [v2.0.3](https://github.com/OpenBMB/VoxCPM/releases/tag/2.0.3) specifically fixes MPS audio by promoting low-precision dtypes to FP32. This is a memory consideration on 16 GB, not evidence of M4 fit. Pin the actual runtime, device, dtype, and reference mode in benchmark records.

### CosyVoice3: instruction and event evidence

The v3 [official example](https://github.com/QwenAudio/CosyVoice/blob/074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc/example.py#L91) calls `inference_instruct2`, not the v1 `inference_instruct`. The [supported instruction list](https://github.com/QwenAudio/CosyVoice/blob/074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc/cosyvoice/utils/common.py#L28) includes happy, sad, angry, speed, volume, dialects, and role styles. Use the framing `You are a helpful assistant. …<|endofprompt|>`; do not include the reference transcript as the instruction text for `instruct2`.

The **v3** [tokenizer branch](https://github.com/QwenAudio/CosyVoice/blob/074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc/cosyvoice/tokenizer/tokenizer.py#L277) registers `[breath]`, `[laughter]`, `[cough]`, `[clucking]`, `[quick_breath]`, `[hissing]`, `[sigh]`, `[vocalized-noise]`, `[lipsmack]`, `[mn]`, `[noise]`, and `<laughter>…</laughter>`, plus emphasis/pronunciation tokens. Registration alone does not establish equal event quality; the v3 example actually uses `[breath]` in cross-lingual inference. Test other listed events individually.

For style, the [frontend](https://github.com/QwenAudio/CosyVoice/blob/074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc/cosyvoice/cli/frontend.py#L168) supplies reference speech tokens and mel features in zero-shot mode. `cross_lingual` and `instruct2` **remove the LM reference-speech tokens**, retaining acoustic conditioning for the flow model. The [author paper](https://funaudiollm.github.io/cosyvoice3/pdf/CosyVoice3_0.pdf) discusses zero-shot timbre/prosody/style, but this is not a separate same-speaker/different-style reference API. Treat performance imitation as conditional on the mode and recording, and measure it separately from identity similarity.

### Sopro: actual style conditioning, no instruction API

[`prepare_reference`](https://github.com/samuel-vitorino/sopro/blob/7bfcf9a0539d274f6593959a21da948f506ceee1/src/sopro/model.py#L107) builds conditioning from the speaker encoder's `id_emb`, `style_emb`, and `style_ctrl`; semantic generation also uses reference-derived style tokens. This supports **coupled reference-style conditioning**, not independent text emotion selection. The [author's architecture account](https://research.haloneuro.ai/posts/sopro-v2) describes this conditioning and identifies added emotion control as future work. No supported nonverbal vocabulary or joint-speaker generator was established.

The [current author README](https://github.com/samuel-vitorino/sopro/blob/7bfcf9a0539d274f6593959a21da948f506ceee1/README.md) says current artifacts require **`sopro>=2.2.0`**, default to CPU on macOS, and permit explicit MPS. Basic API: `SoproTTS.from_pretrained(local_model_dir, device="cpu")`, `tts.synthesize(text, ref_audio_path="ref.wav")`, `tts.save_wav(path, wav)`. References need no transcript. Cache `tts.prepare_reference(...)` when evaluating warm synthesis; keep reference preparation as a separate measured stage.

### Phonon: public runtime versus hosted voice creation

The [31M card](https://huggingface.co/gradium/phonon-31m-en/blob/92741c5b07d085ff840e26fea8ce43110b87852f/README.md) exposes precomputed voice latents and speech generation, not an emotion/style instruction field. The [runtime's voice workflow](https://github.com/gradium-ai/xn-ptts/blob/e557fc3966b432252f04173ac5e3817288f0dbb5/README.md#L148) explicitly requires **Gradium Studio/API and an API key** to design/clone a new voice, then export it for local use. `scripts/clone-voice.py` is that service workflow, not evidence of a released local audio-to-voice encoder. Downloaded voices and `add_voice` are selection/registration, not local cloning or local design.

For this offline experiment, use the bundled voice or separately licensed author voice states. `ptts.TTS(local_model_dir, lang="en", quant="q8")`, then `tts.save("out.wav", text)` is the [documented Python shape](https://github.com/gradium-ai/xn-ptts/blob/e557fc3966b432252f04173ac5e3817288f0dbb5/ptts-pyo3/README.md). `duration_delta` changes rate, not emotion. Do not treat Transformers' chat-shaped input as joint dialogue. The model card says long text is sentence-split without whole-text prosody continuity. Native Apple/Core ML support is an author-maintained route, distinct from MPS.

## CosyVoice3 on Mac: pinned CPU baseline

**Official code supports CPU; its inspected loader does not select MPS or accept a `device` argument.** Both [frontend](https://github.com/QwenAudio/CosyVoice/blob/074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc/cosyvoice/cli/frontend.py#L41) and [model](https://github.com/QwenAudio/CosyVoice/blob/074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc/cosyvoice/cli/model.py#L397) select only `cuda` or `cpu`. `PYTORCH_ENABLE_MPS_FALLBACK=1` does not change that. Moving just the LLM to MPS does not move the flow, frontend tensors, caches, or vocoder; that would be a port requiring separate validation.

Pins:

| Component | Exact revision |
| --- | --- |
| [Official code](https://github.com/QwenAudio/CosyVoice/tree/074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc) | `074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc` |
| [Matcha-TTS submodule](https://github.com/shivammehta25/Matcha-TTS/tree/dd9105b34bf2be2230f4aa1e4769fb586a3c824e) | `dd9105b34bf2be2230f4aa1e4769fb586a3c824e` |
| [FunAudioLLM/Fun-CosyVoice3-0.5B-2512](https://huggingface.co/FunAudioLLM/Fun-CosyVoice3-0.5B-2512/tree/29e01c4e8d000f4bcd70751be16fa94bf3d85a18) | `29e01c4e8d000f4bcd70751be16fa94bf3d85a18`, **base `llm.pt`** |

### Install guidance for the parent operator

Use an **arm64 CPython 3.10 environment** separate from the other models. This is a source-derived minimal inference subset of [upstream requirements](https://github.com/QwenAudio/CosyVoice/blob/074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc/requirements.txt), not an author-tested Mac lockfile. It omits the web/service/training executables and optional text-normalizer packages, but retains dependencies imported eagerly by inference. Additional Hub/TorchMetrics/setuptools pins below are compatibility constraints chosen for this experiment; transitive dependencies still need a captured lock after the parent resolves them.

```sh
git clone --filter=blob:none https://github.com/QwenAudio/CosyVoice.git cosyvoice
cd cosyvoice
git checkout 074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc
git submodule update --init --recursive
python3.10 -m venv .venv
. .venv/bin/activate
python -m pip install 'setuptools<81' wheel
python -m pip install \
  torch==2.3.1 torchaudio==2.3.1 numpy==1.26.4 onnxruntime==1.18.0 \
  transformers==4.51.3 huggingface-hub==0.30.2 \
  HyperPyYAML==1.2.3 modelscope==1.20.0 omegaconf==2.3.0 \
  openai-whisper==20231117 inflect==7.3.1 librosa==0.10.2 soundfile==0.12.1 \
  conformer==0.3.2 diffusers==0.29.0 x-transformers==2.11.24 \
  lightning==2.2.4 torchmetrics==1.3.2 hydra-core==1.3.2 rich==13.7.1 \
  matplotlib==3.7.5 gdown==5.1.0 wget==3.2 \
  pyarrow==18.1.0 pyworld==0.3.4
python -m pip check
unset onnx_path
PYTHONPATH="$PWD:$PWD/third_party/Matcha-TTS" python -c \
  'from cosyvoice.cli.cosyvoice import CosyVoice3; print("imports ok")'
```

**Install is not wholly wheel-only:** [PyWorld 0.3.4's published files](https://pypi.org/pypi/pyworld/0.3.4/json) provide no macOS wheel; expect a native extension build and compiler/build dependencies. [Torch 2.3.1](https://pypi.org/pypi/torch/2.3.1/json) and [ORT 1.18.0](https://pypi.org/pypi/onnxruntime/1.18.0/json) do publish compatible Mac/Python 3.10 wheels. No build was performed in this research.

Have the parent fetch **only** these model files at the pinned revision into `pretrained_models/Fun-CosyVoice3-0.5B-2512/`:

```text
cosyvoice3.yaml
llm.pt
flow.pt
hift.pt
campplus.onnx
speech_tokenizer_v3.onnx
CosyVoice-BlankEN/*
```

The [loader](https://github.com/QwenAudio/CosyVoice/blob/074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc/cosyvoice/cli/cosyvoice.py#L189) overrides `qwen_pretrain_path` to the local `CosyVoice-BlankEN` directory. **Its `model.safetensors` is required**, not just its tokenizer/config: [`Qwen2Encoder`](https://github.com/QwenAudio/CosyVoice/blob/074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc/cosyvoice/llm/llm.py#L249) calls `Qwen2ForCausalLM.from_pretrained`. The base loader reads `llm.pt`; `llm.rl.pt` is not selected automatically. `speech_tokenizer_v3.batch.onnx`, `flow.decoder.estimator.fp32.onnx`, and TensorRT plans are unnecessary for this CPU route. `spk2info.pt` is optional; absence starts with an empty speaker cache.

### Inference API and reference fixture

Author fixture: [`asset/zero_shot_prompt.wav`](https://github.com/QwenAudio/CosyVoice/blob/074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc/asset/zero_shot_prompt.wav), with exact spoken transcript **`希望你以后能够做的比我还好呦。`**, supplied by the pinned [v3 example](https://github.com/QwenAudio/CosyVoice/blob/074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc/example.py#L95). The repository declares [Apache-2.0](https://github.com/QwenAudio/CosyVoice/blob/074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc/LICENSE); no separate recording-specific provenance/consent statement was established. This is the official reproduction fixture with the repository's declared terms, **not separately verified speaker-rights clearance for redistribution**. No WAV was fetched here.

Run from the pinned code checkout after the parent supplies the model files. This example saves one complete output per mode and rejects silent/NaN output:

```python
import os
import sys
from pathlib import Path

os.environ.pop("onnx_path", None)  # Do not enable CUDA-only online feature extraction.
os.environ["HF_HUB_OFFLINE"] = "1"
sys.path.insert(0, str(Path("third_party/Matcha-TTS").resolve()))

import torch
import soundfile as sf
from cosyvoice.cli.cosyvoice import CosyVoice3

torch.set_num_threads(4)  # Conservative benchmark starting point; tune separately.
torch.set_num_interop_threads(1)
model_dir = Path("pretrained_models/Fun-CosyVoice3-0.5B-2512").resolve()
required = ["cosyvoice3.yaml", "llm.pt", "flow.pt", "hift.pt", "campplus.onnx",
            "speech_tokenizer_v3.onnx", "CosyVoice-BlankEN/model.safetensors"]
assert all((model_dir / name).is_file() for name in required), "Incomplete local model"
ref = str(Path("asset/zero_shot_prompt.wav").resolve())
assert Path(ref).is_file()
cv = CosyVoice3(str(model_dir), load_trt=False, load_vllm=False, fp16=False)
assert cv.model.device.type == "cpu"
prefix = "You are a helpful assistant.<|endofprompt|>"
transcript = "希望你以后能够做的比我还好呦。"
text = "今天有一个好消息，我们终于可以出发了。"

def save(name, outputs):
    chunks = [item["tts_speech"].detach().cpu() for item in outputs]
    assert chunks, "No audio chunks"
    wav = torch.cat(chunks, dim=-1)
    assert torch.isfinite(wav).all() and wav.abs().max() > 1e-5, "Invalid/silent audio"
    sf.write(name, wav.squeeze(0).numpy(), cv.sample_rate)

save("cosy-neutral.wav", cv.inference_zero_shot(
    text, prefix + transcript, ref, stream=False, speed=1.0, text_frontend=False))
save("cosy-happy.wav", cv.inference_instruct2(
    text, "You are a helpful assistant. 请非常开心地说一句话。<|endofprompt|>",
    ref, stream=False, speed=1.0, text_frontend=False))
save("cosy-breath.wav", cv.inference_cross_lingual(
    prefix + "[breath]今天终于可以出发了。", ref,
    stream=False, speed=1.0, text_frontend=False))
```

The [frontend](https://github.com/QwenAudio/CosyVoice/blob/074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc/cosyvoice/cli/frontend.py#L95) resamples internally and **rejects references over 30 seconds**. Keep the full transcript aligned to the actual clip; if the parent trims audio, update the transcript. `text_frontend=False` avoids optional normalization and preserves control syntax; it does not avoid module imports. Spell out numbers for comparable tests.

### Concrete blockers and CUDA branches

| Failure/risk | Source and action |
| --- | --- |
| `No module named matcha` | Initialize the pinned submodule and add its root to `sys.path`; no Matcha package build is required for this import path. |
| Missing `hydra`, `lightning`, `rich`, `gdown`, `wget`, or plotting dependencies | [`matcha.utils.__init__`](https://github.com/shivammehta25/Matcha-TTS/blob/dd9105b34bf2be2230f4aa1e4769fb586a3c824e/matcha/utils/__init__.py) eagerly imports utility modules. Retain these even for inference. |
| Missing `pyarrow` / `pyworld` while loading YAML | The [inference YAML](https://huggingface.co/FunAudioLLM/Fun-CosyVoice3-0.5B-2512/blob/29e01c4e8d000f4bcd70751be16fa94bf3d85a18/cosyvoice3.yaml) resolves dataset processors; [`processor.py`](https://github.com/QwenAudio/CosyVoice/blob/074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc/cosyvoice/dataset/processor.py) imports both at module load. |
| Missing `conformer` / `diffusers` | [`BASECFM`](https://github.com/shivammehta25/Matcha-TTS/blob/dd9105b34bf2be2230f4aa1e4769fb586a3c824e/matcha/models/components/flow_matching.py) imports Matcha's decoder even though the selected estimator is DiT. |
| ORT `_ARRAY_API not found` / NumPy ABI failure | Keep `numpy==1.26.4`; the [ORT maintainer issue](https://github.com/microsoft/onnxruntime/issues/21063) reproduces ORT 1.18.0 + NumPy 2 failure on Mac ARM64. |
| No `ttsfrd` or `wetext` | [Frontend initialization](https://github.com/QwenAudio/CosyVoice/blob/074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc/cosyvoice/cli/frontend.py#L55) catches absence and continues without normalization. For these fixed test sentences, both can be omitted. |
| Unexpected CUDA ONNX session | Unset lowercase **`onnx_path`**: [`utils/onnx.py`](https://github.com/QwenAudio/CosyVoice/blob/074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc/cosyvoice/utils/onnx.py) enables online features when it is set, and `SpeechTokenExtractor` hardcodes CUDA EP. Ordinary frontend inference chooses CPU EP on Mac. |
| TensorRT / vLLM / mixed precision | Keep `load_trt=False`, `load_vllm=False`, `fp16=False`. [v3 initialization](https://github.com/QwenAudio/CosyVoice/blob/074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc/cosyvoice/cli/cosyvoice.py#L209) disables TRT/FP16 without CUDA, **but does not guard `load_vllm` the same way**. No `load_jit` argument exists on `CosyVoice3`. |
| CUDA references in normal PyTorch inference | [`model.py`](https://github.com/QwenAudio/CosyVoice/blob/074ca6dc9e80a2f424f1f74b48bdd7d3fea531cc/cosyvoice/cli/model.py) uses a null context without CUDA; FP16=False disables its CUDA autocast, and cache synchronization is CUDA-guarded. TensorRT's stream/pointer operations are a different branch. |

An import pass is only a prerequisite. The parent must still run the pinned model, check complete intelligible output, and measure process memory/first audio. Model loading instantiates the YAML's extra modules and loads a blank Qwen backbone before loading the real checkpoint; weight-file totals cannot predict peak RAM. Do not run the five large models concurrently on 16 GB.

## CosyVoice3 fallback: a real maintainer MLX port, with limits

[Anthony DePasquale's maintained `mlx-audio-plus`](https://github.com/DePasqualeOrg/mlx-audio-plus) implements CosyVoice3; this is **a third-party port maintained by its author**, not Alibaba's official Mac runtime. Its [pinned source](https://github.com/DePasqualeOrg/mlx-audio-plus/tree/4c9ec6a8489e790b5ba8964ab1f1d63150476f9f) and [`mlx-community/Fun-CosyVoice3-0.5B-2512-4bit`](https://huggingface.co/mlx-community/Fun-CosyVoice3-0.5B-2512-4bit/tree/55a6713d54751f1ec2645aa11294676f2067202a) are a concrete fallback lead. The [conversion card](https://huggingface.co/mlx-community/Fun-CosyVoice3-0.5B-2512-4bit/blob/55a6713d54751f1ec2645aa11294676f2067202a/README.md) names the selected original checkpoint and conversion tool 0.1.4; it does **not** pin the original checkpoint revision. [Its config](https://huggingface.co/mlx-community/Fun-CosyVoice3-0.5B-2512-4bit/blob/55a6713d54751f1ec2645aa11294676f2067202a/config.json) quantizes the Qwen layers, not every component. This is not evidence of numerical/quality equivalence to the FP32 CPU baseline.

Avoid an unqualified `pip install mlx-audio-plus`: published [0.1.8 metadata](https://pypi.org/pypi/mlx-audio-plus/0.1.8/json) requires both `transformers<5` and unpinned `mlx-audio[all]`; current [mlx-audio 0.6.2](https://pypi.org/pypi/mlx-audio/0.6.2/json) requires `transformers>=5.14.0` and newer `mlx-lm`. A resolver may backtrack to old upstream releases; that is not a reproducible current pairing. The pinned **source** [pyproject](https://github.com/DePasqualeOrg/mlx-audio-plus/blob/4c9ec6a8489e790b5ba8964ab1f1d63150476f9f/pyproject.toml) removes that upstream-package dependency. A separate environment can try:

```sh
python3.10 -m venv .venv-cosy-mlx
. .venv-cosy-mlx/bin/activate
python -m pip install 'mlx-audio-plus[tts] @ git+https://github.com/DePasqualeOrg/mlx-audio-plus.git@4c9ec6a8489e790b5ba8964ab1f1d63150476f9f'
python -m pip check
# After the parent stages the pinned conversion locally:
mlx_audio.tts.generate --model /absolute/path/to/cosyvoice3-mlx-4bit \
  --text 'Hello, we can leave now.' \
  --ref_audio /absolute/path/to/reference.wav \
  --ref_text 'The exact spoken transcript of reference.wav.'
```

The [port implementation](https://raw.githubusercontent.com/DePasqualeOrg/mlx-audio-plus/4c9ec6a8489e790b5ba8964ab1f1d63150476f9f/mlx_audio/tts/models/cosyvoice3/cosyvoice3.py) also loads **`mlx-community/S3TokenizerV3`** separately through `from_pretrained()`. Capture/pin that asset before calling this fully offline; a local main model path alone is insufficient. It looks for embedded `campplus.*` weights or separate speaker-encoder files. Instruct mode uses `instruct_text` plus reference audio; no reference transcript means cross-lingual mode, not automatic transcription. Some common wrapper arguments (including `speed` and `temperature`) are ignored. No installation or compatibility claim for M4 is made here; treat the port as a separate experiment after its imports/assets are resolved.
