#!/usr/bin/env python3
"""Download the pinned sherpa-onnx 2.0.1 arm64 runtime and verify it before use."""
import hashlib
from pathlib import Path
import shutil
import tarfile
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
NAME = 'sherpa-onnx-v2.0.1-osx-arm64-shared-lib'
URL = f'https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/{NAME}.tar.bz2'
ARCHIVE = 'e2d57ad9c83803d6c48f259caa66e44879bd4c4724c61f0d51a277b411bffb1d'
LIBRARIES = {
    'libonnxruntime.dylib': '3567d114f7299d559993e536d605a6f46d7bc9d2542004accc80ee9bf5457f0b',
    'libsherpa-onnx-c-api.dylib': 'c22f53f3c16c077e0ce0f31eca4a6637c86bf842bed67769d6d5a99ebafab340',
    'libsherpa-onnx-cxx-api.dylib': '7f1f463a6c28ccd067f33f14defb690df78e49beccfcbb9ffd417c15f9c426ac',
}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    destination = ROOT / 'resources/runtime' / NAME / 'lib'
    if all((destination / name).is_file() and digest(destination / name) == value for name, value in LIBRARIES.items()):
        print(f'Runtime ready: {destination}')
        return
    with tempfile.TemporaryDirectory() as temporary:
        folder = Path(temporary)
        archive = folder / 'runtime.tar.bz2'
        with urllib.request.urlopen(URL, timeout=600) as response, archive.open('wb') as file:
            shutil.copyfileobj(response, file)
        if digest(archive) != ARCHIVE:
            raise RuntimeError('Runtime archive checksum mismatch')
        with tarfile.open(archive) as tar:
            tar.extractall(folder, filter='data')
        destination.mkdir(parents=True, exist_ok=True)
        for name, value in LIBRARIES.items():
            source = folder / NAME / 'lib' / name
            if digest(source) != value:
                raise RuntimeError(f'{name} checksum mismatch')
            shutil.copy2(source, destination / name)
    print(f'Runtime ready: {destination}')


if __name__ == '__main__':
    main()
