#!/usr/bin/env python3
"""Local development bridge. Framed little-endian float PCM; selected text stays offline."""
import contextlib
import json
import sys

from advanced_tts_worker import load


def main():
    kind = sys.argv[1]
    output = sys.stdout.buffer
    def header(value):
        output.write((json.dumps(value) + '\n').encode())
        output.flush()
    with contextlib.redirect_stdout(sys.stderr):
        rate, generate, config = load(kind)
    header({'event': 'ready', 'rate': rate, 'config': config})
    import numpy as np
    for line in sys.stdin:
        request = json.loads(line)
        text = request['text']
        if not isinstance(text, str) or not text.strip() or len(text) > 100000:
            raise ValueError('Invalid reading text')
        try:
            with contextlib.redirect_stdout(sys.stderr):
                for audio in generate(text):
                    pcm = np.asarray(audio, dtype='<f4').reshape(-1)
                    if not pcm.size or not np.isfinite(pcm).all():
                        raise ValueError('Invalid generated audio')
                    header({'event': 'pcm', 'samples': int(pcm.size)})
                    output.write(pcm.tobytes())
                    output.flush()
            header({'event': 'done'})
        except Exception:
            header({'event': 'error', 'message': 'Local model generation failed. Retry or select another voice.'})
            raise

if __name__ == '__main__':
    main()
