#!/usr/bin/env python3
"""Build, zip and publish a GitHub release with a checksum-pinned install script and a
signed latest.json for the in-app updater.

The update key lives in the login keychain, apart from the code-signing keychain and its
password file. Both are needed to ship a different program, so they must not share a
folder or a backup. Move an existing key file there once with --import-key."""
import argparse
import hashlib
import os
from pathlib import Path
import shlex
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
SIGNING = Path.home() / 'Library/Application Support/InfoTransfer/signing'
KEYCHAIN_SERVICE = 'infotransfer-update-key'


def import_key(path):
    key = Path(path).read_text().strip()
    if len(key) != 64 or any(c not in '0123456789abcdef' for c in key.lower()):
        raise SystemExit('The key file must hold 32 hex bytes.')
    command = (f'add-generic-password -U -s {KEYCHAIN_SERVICE} -a infotransfer '
               f'-l "InfoTransfer update key" -w {shlex.quote(key)}\n')
    # Over stdin, so the key is not visible in the process list.
    subprocess.run(['security', '-i'], input=command, text=True, check=True, capture_output=True)
    print(f'Stored in the login keychain as "{KEYCHAIN_SERVICE}". Keep an offline backup of '
          f'{path}, then delete it from this Mac.')


def update_key():
    """The update key as hex, from INFOTRANSFER_UPDATE_KEY (a file) or the login keychain."""
    configured = os.environ.get('INFOTRANSFER_UPDATE_KEY')
    if configured:
        path = Path(configured).resolve()
        if path.is_relative_to(SIGNING.resolve()):
            raise SystemExit('Keep the update key out of the code-signing folder. '
                             f'Run: python3 scripts/release.py --import-key {path}')
        return path.read_text()
    legacy = SIGNING / 'update-ed25519.key'
    found = subprocess.run(['security', 'find-generic-password', '-s', KEYCHAIN_SERVICE, '-w'],
                           capture_output=True, text=True)
    if found.returncode:
        hint = f' --import-key "{legacy}"' if legacy.exists() else ' --import-key <file>'
        raise SystemExit(f'No update key in the login keychain. Run: python3 scripts/release.py{hint}')
    if legacy.exists():
        print(f'Warning: {legacy} still sits next to the code-signing keychain. Back it up offline, then delete it.')
    return found.stdout


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
    parser.add_argument('--replace', action='store_true', help='Replace the assets of an existing release of this version')
    parser.add_argument('--import-key', metavar='FILE', help='Store an update key file in the login keychain, then exit')
    args = parser.parse_args()
    if args.import_key:
        return import_key(args.import_key)
    key = update_key()
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    if not args.dry_run:
        dirty = subprocess.run(['git', 'status', '--porcelain'], cwd=ROOT, check=True,
                               capture_output=True, text=True).stdout.strip()
        if dirty:
            raise SystemExit('Commit or stash your changes first: a release must match a commit.')
    package = ['python3', 'scripts/package_app.py', '--build-number', args.build_number]
    subprocess.run(package + (['--skip-build'] if args.skip_build else []), cwd=ROOT, check=True)
    dist = ROOT / 'dist'
    archive = dist / f'InfoTransfer-{version}-arm64.zip'
    archive.unlink(missing_ok=True)
    subprocess.run(['ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', dist / 'InfoTransfer.app', archive], check=True)
    sha = hashlib.sha256(archive.read_bytes()).hexdigest()
    installer = dist / 'install.sh'
    template = (ROOT / 'scripts/install.sh').read_text()
    details = subprocess.run(['codesign', '-d', '-r-', dist / 'InfoTransfer.app'], check=True,
                             capture_output=True, text=True)
    requirement = next(line.split('designated => ', 1)[1].strip()
                       for line in (details.stdout + details.stderr).splitlines() if 'designated => ' in line)
    if "'" in requirement:
        raise SystemExit('The signing requirement cannot be quoted for install.sh')
    installer.write_text(template.replace('@VERSION@', version).replace('@SHA256@', sha)
                         .replace('@REQUIREMENT@', requirement))
    installer.chmod(0o755)
    (dist / 'SHA256SUMS').write_text(f'{sha}  {archive.name}\n')
    notes = args.notes or subprocess.run(['git', 'log', '-1', '--format=%s'], cwd=ROOT, check=True,
                                         capture_output=True, text=True).stdout.strip()
    notes_file = dist / 'update-notes.txt'
    notes_file.write_text(notes + '\n')
    manifest = dist / 'latest.json'
    subprocess.run(['cargo', 'run', '--quiet', '--release', '--no-default-features', '--bin', 'update-sign', '--',
                    'manifest', archive, version, notes_file, '-', manifest], cwd=ROOT, check=True,
                   input=key, text=True)
    print(f'{archive.name} {sha}')
    if args.dry_run:
        return
    tag = f'v{version}'
    commit = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=ROOT, check=True, capture_output=True, text=True).stdout.strip()
    assets = [str(archive), str(installer), str(dist / 'SHA256SUMS'), str(manifest)]
    exists = subprocess.run(['gh', 'release', 'view', tag], cwd=ROOT, capture_output=True).returncode == 0
    if exists and not args.replace:
        raise SystemExit(f'{tag} is already published. Bump the version in Cargo.toml, or pass --replace.')
    if exists:
        subprocess.run(['gh', 'release', 'upload', tag, *assets, '--clobber'], cwd=ROOT, check=True)
        subprocess.run(['gh', 'release', 'edit', tag, '--notes', NOTES.format(sha=sha)], cwd=ROOT, check=True)
    else:
        subprocess.run(['gh', 'release', 'create', tag, *assets, '--title', f'InfoTransfer {version}',
                        '--notes', NOTES.format(sha=sha), '--target', commit], cwd=ROOT, check=True)


if __name__ == '__main__':
    main()
