#!/usr/bin/env python3
"""Stage, sign and verify an arm64 bundle before publishing it at a stable path."""
import argparse
import ctypes
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile
import tomllib

from local_signing import identity as local_identity

ROOT = Path(__file__).resolve().parents[1]


def checked(command):
    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=180)
    except subprocess.TimeoutExpired:
        raise RuntimeError(f'{command[0]} timed out; it may be waiting on a hidden Keychain prompt')
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or f'{command[0]} failed')
    return result.stdout


def publish(staged, destination):
    if not destination.exists():
        staged.rename(destination)
        return
    # macOS renamex_np/RENAME_SWAP exchanges both directories atomically.
    # The old bundle moves under the temporary directory and is cleaned up afterward.
    library = ctypes.CDLL(None, use_errno=True)
    exchange = library.renamex_np
    exchange.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint]
    exchange.restype = ctypes.c_int
    if exchange(os.fsencode(staged), os.fsencode(destination), 0x00000002):
        raise OSError(ctypes.get_errno(), 'Could not atomically replace InfoTransfer')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--skip-build', action='store_true')
    parser.add_argument('--build-number', default='1')
    parser.add_argument('--ui-preview', action='store_true', help='Isolated ad-hoc UI build; does not replace the permission-approved app')
    parser.add_argument('--install', action='store_true', help='Also install to /Applications')
    args = parser.parse_args()
    destination = ROOT / ('.cache/ui-preview/InfoTransfer.app' if args.ui_preview else 'dist/InfoTransfer.app')
    # Do not change the bundle beneath a live process and confuse macOS identity checks.
    running = subprocess.run(['pgrep', '-f', r'/InfoTransfer\.app/Contents/MacOS/infotransfer($| )'], capture_output=True)
    if running.returncode == 0:
        raise RuntimeError('Quit InfoTransfer before packaging. The installed bundle has not been changed.')
    if not args.skip_build:
        subprocess.run(['cargo', 'build', '--release', '--bin', 'infotransfer'], cwd=ROOT, check=True)
    configured = os.environ.get('INFOTRANSFER_SIGNING_IDENTITY')
    local = None if configured or args.ui_preview else local_identity()
    signer = '-' if args.ui_preview else configured or local['sha1']
    signing = ['--sign', signer, '--timestamp=none']
    if local:
        signing += ['--keychain', local['keychain']]
    metadata = json.loads(checked(['cargo', 'metadata', '--no-deps', '--format-version', '1']))
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.package-', dir=destination.parent) as temporary:
        bundle = Path(temporary) / 'InfoTransfer.app'
        macos = bundle / 'Contents/MacOS'
        resources = bundle / 'Contents/Resources'
        macos.mkdir(parents=True)
        resources.mkdir()
        shutil.copy2(Path(metadata['target_directory']) / 'release/infotransfer', macos / 'infotransfer')
        runtime = resources / 'runtime'
        runtime.mkdir()
        # Paradee runs on ONNX Runtime directly; the sherpa-onnx libraries are not shipped.
        libraries = list((ROOT / 'resources/runtime').rglob('libonnxruntime.dylib'))
        if len(libraries) != 1:
            raise RuntimeError('Prepare the verified sherpa-onnx 2.0.1 arm64 runtime first')
        for library in libraries:
            shutil.copy2(library, runtime / library.name)
        pronunciation = ROOT / 'resources/phonemizer'
        if not (pronunciation / 'PROVENANCE.json').is_file():
            raise RuntimeError('Run python3 scripts/prepare_pronunciation.py before packaging')
        import hashlib
        manifest = json.loads((pronunciation / 'PROVENANCE.json').read_text())
        for name, digest in manifest['files'].items():
            path = pronunciation / name
            if not path.is_relative_to(pronunciation) or hashlib.sha256(path.read_bytes()).hexdigest() != digest:
                raise RuntimeError('Pronunciation artifact checksum mismatch')
        shutil.copytree(pronunciation, resources / 'phonemizer')
        for source, name in [
            ('vendor/sherpa-onnx/LICENSE', 'LICENSE-APACHE-2.0'),
            ('vendor/onnxruntime/LICENSE', 'ONNXRUNTIME-LICENSE'),
            ('resources/SPEECH-NOTICES.txt', 'SPEECH-NOTICES.txt'),
            ('assets/icons/LICENSE', 'LUCIDE-LICENSE'),
            ('vendor/sonic/LICENSE', 'SONIC-LICENSE'),
            ('assets/AppIcon.icns', 'AppIcon.icns'),
        ]:
            shutil.copy2(ROOT / source, resources / name)
        # The updater compares this with each release's signed version.
        version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
        preview_id = 'dev.infotransfer.reader.preview'
        info = {'CFBundleIdentifier': preview_id if args.ui_preview else 'dev.infotransfer.reader', 'CFBundleName': 'InfoTransfer Preview' if args.ui_preview else 'InfoTransfer',
                'CFBundleDisplayName': 'InfoTransfer', 'CFBundleExecutable': 'infotransfer',
                'CFBundlePackageType': 'APPL', 'CFBundleShortVersionString': version,
                'CFBundleVersion': args.build_number, 'LSMinimumSystemVersion': '14.0',
                'LSUIElement': True, 'NSHighResolutionCapable': True, 'CFBundleIconFile': 'AppIcon',
                'NSAccessibilityUsageDescription': 'Read selected text when you tap Fn or right Option.'}
        (bundle / 'Contents/Info.plist').write_bytes(plistlib.dumps(info))
        for library in runtime.iterdir():
            checked(['codesign', '--force', *signing, str(library)])
        checked(['codesign', '--force', *signing, str(resources / 'phonemizer/phonemize')])
        packed_manifest = resources / 'phonemizer/PROVENANCE.json'
        packed = json.loads(packed_manifest.read_text())
        packed['source_build_executable_sha256'] = packed['files']['phonemize']
        packed['files']['phonemize'] = hashlib.sha256((resources / 'phonemizer/phonemize').read_bytes()).hexdigest()
        packed_manifest.write_text(json.dumps(packed, indent=2) + '\n')
        checked(['codesign', '--force', *signing, str(bundle)])
        checked(['codesign', '--verify', '--deep', '--strict', str(bundle)])
        details = subprocess.run(['codesign', '-d', '-r-', str(bundle)], check=True, capture_output=True, text=True)
        requirement = next(line.split('designated => ', 1)[1] for line in (details.stdout+details.stderr).splitlines() if 'designated => ' in line)
        if not args.ui_preview and 'cdhash H' in requirement:

            raise RuntimeError('Refusing an unstable ad-hoc identity that would lose Accessibility permission')
        checked(['codesign', '--verify', '--strict', '-R='+requirement, str(bundle)])
        size = sum(path.stat().st_size for path in bundle.rglob('*') if path.is_file())
        publish(bundle, destination)
        if args.install:
            if args.ui_preview:
                raise RuntimeError('Install a signed build, not an ad-hoc preview')
            installed = Path('/Applications/InfoTransfer.app')
            staged = Path('/Applications/.InfoTransfer.installing.app')
            shutil.rmtree(staged, ignore_errors=True)
            subprocess.run(['ditto', str(destination), str(staged)], check=True)
            publish(staged, installed)
            shutil.rmtree(staged, ignore_errors=True)
    record = {'bundle': str(destination), 'bytes': size, 'signer': signer, 'requirement': requirement,
              'signing': 'ad-hoc UI preview only' if args.ui_preview else 'persistent local development identity' if local else 'configured signing identity',
              'build_number': args.build_number, 'notarized': False}
    (destination.parent / 'signing.json').write_text(json.dumps(record, indent=2)+'\n')
    print(json.dumps(record))


if __name__ == '__main__':
    main()
