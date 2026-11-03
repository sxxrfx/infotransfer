#!/usr/bin/env python3
"""Offline, isolated model-component measurements; never controls system audio."""
import argparse
import contextlib
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import resource
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / '.cache/advanced-tts'
os.environ.update(HF_HUB_OFFLINE='1', TRANSFORMERS_OFFLINE='1', TOKENIZERS_PARALLELISM='false', HF_HUB_DISABLE_PROGRESS_BARS='1')


def load(kind):
    folder = CACHE / kind
    reference = str(CACHE / 'reference.wav')
    transcript = json.loads((CACHE / 'reference.json').read_text())['text']
    if kind == 'phonon':
        import ptts
        ptts.set_num_threads(4)
        model = ptts.TTS(model=str(folder), device='cpu', quant='q8', lang='en', seed=42)
        return model.sample_rate, lambda text: model.stream(text), {'runtime': 'ptts', 'version': ptts.__version__, 'device': model.device, 'quant': model.quant, 'threads': 4, 'seed': 42}
    if kind == 'sopro':
        import torch
        from sopro import SoproTTS
        torch.set_num_threads(4)
        torch.set_num_interop_threads(1)
        torch.manual_seed(42)
        device = os.getenv('SOPRO_DEVICE', 'cpu')
        model = SoproTTS.from_pretrained(str(folder), device=device, quantization='int8' if device == 'cpu' else None)
        ref = model.prepare_reference(ref_audio_path=reference, stream=True)
        return model.sample_rate, lambda text: (a.detach().cpu().numpy() for a in model.stream(text, ref=ref, lang='en', steps=2)), {'runtime': 'sopro', 'version': importlib.metadata.version('sopro'), 'device': device, 'quant': 'int8' if device == 'cpu' else 'float32', 'threads': 4, 'seed': 42, 'steps': 2, 'reference_sha256': hashlib.sha256(Path(reference).read_bytes()).hexdigest()}
    if kind == 'cosyvoice3':
        sys.path.insert(0, str(CACHE / 'code/mlx-audio-plus'))
    import mlx.core as mx
    from mlx_audio.tts.utils import load as load_mlx
    mx.random.seed(42)
    model = load_mlx(str(folder))
    params = {'runtime': 'mlx-audio', 'version': importlib.metadata.version('mlx-audio'), 'device': 'metal', 'quant': '4bit LM, unquantized decoder', 'seed': 42}
    if kind == 'qwen3':
        generate = lambda text: model.generate(text=text, voice='Ryan', lang_code='English', stream=True, streaming_interval=0.5, max_tokens=800, verbose=False)
        params.update(voice='Ryan', streaming_interval=0.5, max_tokens=800)
    elif kind == 'voxcpm2':
        generate = lambda text: model.generate(text=text, ref_audio=reference, ref_text=transcript, inference_timesteps=10, max_tokens=800, verbose=False)
        params.update(reference_sha256=hashlib.sha256(Path(reference).read_bytes()).hexdigest(), inference_timesteps=10, max_tokens=800)
    else:
        from mlx_audio.codec.models.s3tokenizer.model_v3 import S3TokenizerV3
        import soundfile as sf
        import numpy as np
        audio, sr = sf.read(reference, dtype='float32')
        assert sr == 24000
        model._s3_tokenizer = S3TokenizerV3('speech_tokenizer_v3')
        model._s3_tokenizer.load_weights(list(mx.load(str(CACHE / 's3tokenizer/model.safetensors')).items()))
        mx.eval(model._s3_tokenizer.parameters())
        model._ensure_model_loaded()
        model._ensure_tokenizers_loaded()
        mx.eval(model._model.parameters())
        generate = lambda text: model.generate(text=text, ref_audio=mx.array(audio), ref_text=transcript, stream=True, max_tokens=800, verbose=False, seed=42)
        params.update(runtime='mlx-audio-plus source', commit='4c9ec6a8489e790b5ba8964ab1f1d63150476f9f', reference_sha256=hashlib.sha256(Path(reference).read_bytes()).hexdigest(), max_tokens=800)
    mx.eval(model.parameters())
    def pcm(text):
        for result in generate(text):
            mx.eval(result.audio)
            yield __import__('numpy').asarray(result.audio, dtype='float32')
    return model.sample_rate, pcm, params


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--model', required=True)
    parser.add_argument('--round', type=int, default=0)
    parser.add_argument('--output', type=Path, default=ROOT / 'docs/performance/five-models')
    parser.add_argument('--warm', type=int, default=1)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    # Record the executing code and staged assets before generation, never relabel old outputs.
    fingerprint = hashlib.sha256(Path(__file__).read_bytes() + (CACHE / 'downloads.json').read_bytes()).hexdigest()
    started = time.perf_counter()
    with contextlib.redirect_stdout(sys.stderr):
        rate, generate, config = load(args.model)
    load_ms = (time.perf_counter() - started) * 1000
    import numpy as np
    import soundfile as sf
    import psutil
    process = psutil.Process()
    ready_rss = process.memory_info().rss
    text = 'Reading at your own pace can make a difficult paragraph easier to follow. Take a breath, then continue when you are ready.'
    print(json.dumps({'event': 'loaded', 'model': args.model, 'round': args.round, 'load_ms': load_ms, 'ready_rss_bytes': ready_rss, 'sample_rate': rate, 'config': config, 'fingerprint': fingerprint, 'mlx_active_bytes': sys.modules['mlx.core'].get_active_memory() if 'mlx.core' in sys.modules else None}), flush=True)
    for trial in range(args.warm + 1):
        first = signal = None
        arrays = []
        chunks = []
        start = time.perf_counter()
        with contextlib.redirect_stdout(sys.stderr):
            for array in generate(text):
                array = np.asarray(array, dtype=np.float32).reshape(-1).copy()
                elapsed = (time.perf_counter() - start) * 1000
                assert array.size and np.isfinite(array).all(), 'Empty or nonfinite PCM'
                if first is None: first = elapsed
                if signal is None and np.max(np.abs(array)) >= 0.0031623: signal = elapsed
                chunks.append({'ms': elapsed, 'samples': int(array.size)})
                arrays.append(array)
        elapsed = (time.perf_counter() - start) * 1000
        pcm = np.concatenate(arrays)
        assert signal is not None, 'Silent output'
        path = args.output / f'{args.model}-r{args.round}-t{trial}.wav'
        sf.write(path, pcm, rate, subtype='PCM_16')
        print(json.dumps({'event': 'recording', 'model': args.model, 'round': args.round, 'trial': trial, 'state': 'first inference' if trial == 0 else 'warm', 'text': text, 'success': True, 'first_chunk_ms': first, 'first_signal_chunk_ms': signal, 'generation_ms': elapsed, 'audio_seconds': len(pcm)/rate, 'rtf': elapsed/1000/(len(pcm)/rate), 'peak': float(np.max(np.abs(pcm))), 'clipped_samples': int(np.sum(np.abs(pcm) >= 1)), 'peak_rss_bytes': resource.getrusage(resource.RUSAGE_SELF).ru_maxrss, 'mlx_peak_bytes': sys.modules['mlx.core'].get_peak_memory() if 'mlx.core' in sys.modules else None, 'wav': str(path.relative_to(ROOT)), 'wav_sha256': hashlib.sha256(path.read_bytes()).hexdigest(), 'chunks': chunks, 'fingerprint': fingerprint}), flush=True)

if __name__ == '__main__':
    try:
        main()
    except Exception as exc:
        print(json.dumps({'event': 'failure', 'success': False, 'error': str(exc)}), flush=True)
        raise
