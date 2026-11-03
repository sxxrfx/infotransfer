#!/usr/bin/env python3
"""Build, zip and publish a GitHub release with a checksum-pinned install script and a
signed latest.json for the in-app updater."""
import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
UPDATE_KEY = Path(os.environ.get('INFOTRANSFER_UPDATE_KEY', Path.home() / 'Library/Application Support/InfoTransfer/signing/update-ed25519.key'))
NOTES = '''**Unsigned app.** InfoTransfer is not signed with an Apple Developer ID or notarized by Apple.
Building from source is more secure: see [Build from source](https://github.com/sxxrfx/infotransfer#build-from-source).

Install (Apple Silicon, macOS 14+):

```sh
curl -fsSL https://github.com/sxxrfx/infotransfer/releases/latest/download/install.sh | sh
```

The script shows this warning, asks before installing, and checks the zip's SHA256:
`{sha}`

If you download the zip in a browser instead, macOS will refuse to open it. After moving the app to Applications, run
`xattr -dr com.apple.quarantine /Applications/InfoTransfer.app`, or use System Settings → Privacy & Security → Open Anyway.
'''


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--build-number', default='1')
    parser.add_argument('--skip-build', action='store_true')
    parser.add_argument('--dry-run', action='store_true', help='Build the assets but do not publish')
    parser.add_argument('--notes', help='What changed, shown in the app before installing (default: the HEAD commit subject)')
    args = parser.parse_args()
    if not UPDATE_KEY.is_file():
        raise SystemExit(f'Update signing key not found at {UPDATE_KEY}. Set INFOTRANSFER_UPDATE_KEY or restore it from backup.')
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    package = ['python3', 'scripts/package_app.py', '--build-number', args.build_number]
    subprocess.run(package + (['--skip-build'] if args.skip_build else []), cwd=ROOT, check=True)
    dist = ROOT / 'dist'
    archive = dist / f'InfoTransfer-{version}-arm64.zip'
    archive.unlink(missing_ok=True)
    subprocess.run(['ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', dist / 'InfoTransfer.app', archive], check=True)
    sha = hashlib.sha256(archive.read_bytes()).hexdigest()
    installer = dist / 'install.sh'
    template = (ROOT / 'scripts/install.sh').read_text()
    installer.write_text(template.replace('@VERSION@', version).replace('@SHA256@', sha))
    installer.chmod(0o755)
    (dist / 'SHA256SUMS').write_text(f'{sha}  {archive.name}\n')
    notes = args.notes or subprocess.run(['git', 'log', '-1', '--format=%s'], cwd=ROOT, check=True,
                                         capture_output=True, text=True).stdout.strip()
    notes_file = dist / 'update-notes.txt'
    notes_file.write_text(notes + '\n')
    manifest = dist / 'latest.json'
    subprocess.run(['cargo', 'run', '--quiet', '--release', '--no-default-features', '--bin', 'update-sign', '--',
                    'manifest', archive, version, notes_file, UPDATE_KEY, manifest], cwd=ROOT, check=True)
    print(f'{archive.name} {sha}')
    if args.dry_run:
        return
    tag = f'v{version}'
    commit = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=ROOT, check=True, capture_output=True, text=True).stdout.strip()
    assets = [str(archive), str(installer), str(dist / 'SHA256SUMS'), str(manifest)]
    exists = subprocess.run(['gh', 'release', 'view', tag], cwd=ROOT, capture_output=True).returncode == 0
    if exists:
        subprocess.run(['gh', 'release', 'upload', tag, *assets, '--clobber'], cwd=ROOT, check=True)
        subprocess.run(['gh', 'release', 'edit', tag, '--notes', NOTES.format(sha=sha)], cwd=ROOT, check=True)
    else:
        subprocess.run(['gh', 'release', 'create', tag, *assets, '--title', f'InfoTransfer {version}',
                        '--notes', NOTES.format(sha=sha), '--target', commit], cwd=ROOT, check=True)


if __name__ == '__main__':
    main()
