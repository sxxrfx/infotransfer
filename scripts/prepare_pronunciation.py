#!/usr/bin/env python3
"""Build an isolated, offline eSpeak pronunciation utility and retain corresponding source."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
COMMIT = '4870adfa25b1a32b4361592f1be8a40337c58d6c'
WORK = ROOT / '.cache/pronunciation'
SOURCE = WORK / 'espeak-ng'
TOOLS = WORK / 'build-env/bin'
BUILD = WORK / 'build'
OUT = ROOT / 'resources/phonemizer'


def run(command, **kwargs):
    subprocess.run(list(map(str, command)), check=True, **kwargs)


def main():
    WORK.mkdir(parents=True, exist_ok=True)
    if not (TOOLS / 'cmake').is_file():
        run([sys.executable, '-m', 'venv', WORK / 'build-env'])
        run([TOOLS / 'python', '-m', 'pip', 'install', 'cmake==3.31.6', 'ninja==1.11.1.3'])
    if not SOURCE.exists():
        run(['git', 'clone', '--depth', '1', '--branch', '1.52.0', 'https://github.com/espeak-ng/espeak-ng.git', SOURCE])
    actual = subprocess.check_output(['git', '-C', str(SOURCE), 'rev-parse', 'HEAD'], text=True).strip()
    if actual != COMMIT:
        raise RuntimeError('Unexpected eSpeak source revision')
    # Upstream probes/fetches Sonic even when explicitly disabled. Honor OFF
    # without downloading or linking an unrelated audio processor.
    deps = SOURCE / 'cmake/deps.cmake'
    original = subprocess.check_output(['git', '-C', str(SOURCE), 'show', 'HEAD:cmake/deps.cmake'], text=True)
    start = original.index('if (SONIC_LIB AND SONIC_INC)')
    end = original.index('if (PCAUDIO_LIB AND PCAUDIO_INC)', start)
    deps.write_text(original[:start] + 'if (USE_LIBSONIC)\n' + original[start:end] + 'endif()\n' + original[end:])
    # English-only dictionaries; retain all phoneme tables and language metadata.
    data = SOURCE / 'cmake/data.cmake'
    original_data = subprocess.check_output(['git', '-C', str(SOURCE), 'show', 'HEAD:cmake/data.cmake'], text=True)
    stop = original_data.index('list(APPEND _mbrola_lang_list')
    data.write_text('set(_dict_compile_list en)\n\n' + original_data[stop:])
    env = {**os.environ, 'PATH': str(TOOLS) + os.pathsep + os.environ['PATH']}
    run([TOOLS / 'cmake', '-S', SOURCE, '-B', BUILD, '-G', 'Ninja', '-DCMAKE_BUILD_TYPE=Release',
         '-DCMAKE_OSX_DEPLOYMENT_TARGET=14.0', '-DBUILD_SHARED_LIBS=OFF', '-DBUILD_TESTING=OFF',
         '-DUSE_MBROLA=OFF', '-DUSE_LIBSONIC=OFF', '-DUSE_LIBPCAUDIO=OFF', '-DUSE_ASYNC=OFF',
         '-DUSE_SPEECHPLAYER=OFF', '-DUSE_KLATT=OFF',
         '-DESPEAK_BUILD_MANPAGES=OFF'], env=env)
    run([TOOLS / 'cmake', '--build', BUILD, '-j', '4'], env=env)
    OUT.mkdir(parents=True, exist_ok=True)
    library = next(BUILD.rglob('libespeak-ng.a'))
    unicode_library = next(BUILD.rglob('libucd.a'))
    run(['clang', '-O2', '-mmacosx-version-min=14.0', '-I', SOURCE / 'src/include',
         ROOT / 'native/phonemize.c', library, unicode_library, '-lm', '-o', OUT / 'phonemize'])
    target = OUT / 'espeak-ng-data'
    if target.exists():
        shutil.rmtree(target)
    shutil.copytree(BUILD / 'espeak-ng-data', target)
    shutil.copy2(SOURCE / 'COPYING', OUT / 'COPYING-ESPEAK-NG')
    # Exact patched tree + original utility + reproducible build instructions.
    source_tree = WORK / 'corresponding-source'
    if source_tree.exists():
        shutil.rmtree(source_tree)
    shutil.copytree(SOURCE, source_tree / 'espeak-ng', ignore=shutil.ignore_patterns('.git'))
    shutil.copy2(ROOT / 'native/phonemize.c', source_tree / 'phonemize.c')
    shutil.copy2(ROOT / 'LICENSE', source_tree / 'LICENSE-INFOTRANSFER')
    shutil.copy2(Path(__file__), source_tree / 'prepare_pronunciation.py')
    (source_tree / 'BUILD.txt').write_text('eSpeak NG 1.52.0, original revision ' + COMMIT + '\n'
        'The tree includes the English-only data and disabled-Sonic CMake patches.\n'
        'Configure/build with the options in prepare_pronunciation.py.\n'
        'Then: clang -O2 -I espeak-ng/src/include phonemize.c build/src/libespeak-ng/libespeak-ng.a build/src/ucd-tools/libucd.a -lm -o phonemize\n'
        'The standalone utility is distributed with eSpeak under GPL-3.0-or-later;\n'
        'original InfoTransfer application/utility source remains Apache-2.0.\n')
    run(['tar', '-czf', OUT / 'corresponding-source.tar.gz', '-C', source_tree, '.'])
    files = {str(path.relative_to(OUT)): hashlib.sha256(path.read_bytes()).hexdigest()
             for path in sorted(OUT.rglob('*')) if path.is_file() and path.name != 'PROVENANCE.json'}
    (OUT / 'PROVENANCE.json').write_text(json.dumps({'source': 'https://github.com/espeak-ng/espeak-ng',
        'revision': COMMIT, 'license': 'GPL-3.0-or-later (separate executable)', 'english_only': True,
        'macos_deployment_target': '14.0', 'files': files,
        'bytes': sum((OUT / name).stat().st_size for name in files)}, indent=2) + '\n')
    print(json.dumps({'phonemizer': str(OUT), 'files': len(files)}))


if __name__ == '__main__':
    main()
