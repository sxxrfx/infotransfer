#!/usr/bin/env python3
"""A reusable local code-signing identity; no system/TLS trust changes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import shlex
import subprocess
import tempfile

DIRECTORY = Path.home() / 'Library/Application Support/InfoTransfer/signing'
CONFIG = DIRECTORY / 'identity.json'
KEYCHAIN = DIRECTORY / 'signing.keychain-db'
PASSWORD = DIRECTORY / 'keychain-password'
NAME = 'InfoTransfer Local Development'


def run(args, **kwargs):
    return subprocess.run(args, check=True, capture_output=True, text=True, **kwargs)


def security(command):
    """Run one security command over stdin, keeping the password out of argv."""
    result = subprocess.run(['security', '-i'], input=command + '\n', capture_output=True, text=True, timeout=30)
    if result.returncode or 'SecKeychain' in result.stderr or 'security: ' in result.stderr:
        raise RuntimeError('Keychain command failed: ' + command.split()[0])


def identity():
    if not CONFIG.exists():
        raise RuntimeError('Local signing is not set up. Run python3 scripts/local_signing.py --setup once, or set INFOTRANSFER_SIGNING_IDENTITY.')
    value = json.loads(CONFIG.read_text())
    if value.get('keychain') != str(KEYCHAIN):
        raise RuntimeError('This identity needs Keychain approval. Run python3 scripts/local_signing.py --setup.')
    security('unlock-keychain -p ' + shlex.quote(PASSWORD.read_text()) + ' ' + shlex.quote(str(KEYCHAIN)))
    result = run(['security', 'find-certificate', '-a', '-c', NAME, '-Z', value['keychain']], timeout=30).stdout
    if value['sha1'] not in result:
        raise RuntimeError('The saved signing certificate is missing. Restore it; a new identity would invalidate permissions.')
    return value


def setup():
    """Create a self-signed identity in its own keychain, so builds never wait on a Keychain prompt."""
    if CONFIG.exists() and json.loads(CONFIG.read_text()).get('keychain') == str(KEYCHAIN):
        return identity()
    DIRECTORY.mkdir(parents=True, exist_ok=True)
    DIRECTORY.chmod(0o700)
    if CONFIG.exists():
        CONFIG.rename(DIRECTORY / 'identity-login-keychain.json')
    with tempfile.TemporaryDirectory(prefix='create-', dir=DIRECTORY) as temporary:
        folder = Path(temporary)
        transfer = secrets.token_urlsafe(32)
        secret = folder / 'password'; secret.write_text(transfer); secret.chmod(0o600)
        config = folder / 'openssl.cnf'
        config.write_text('[req]\nprompt=no\ndistinguished_name=dn\nx509_extensions=code_signing\n[dn]\nCN='+NAME+'\nO=InfoTransfer Local\n[code_signing]\nbasicConstraints=critical,CA:false\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=critical,codeSigning\nsubjectKeyIdentifier=hash\n')
        run(['/usr/bin/openssl', 'req', '-new', '-x509', '-newkey', 'rsa:3072', '-nodes', '-sha256', '-days', '3650', '-config', str(config), '-keyout', str(folder/'key.pem'), '-out', str(folder/'certificate.pem')])
        run(['/usr/bin/openssl', 'x509', '-in', str(folder/'certificate.pem'), '-outform', 'der', '-out', str(folder/'certificate.der')])
        certificate = (folder/'certificate.der').read_bytes()
        fingerprint = hashlib.sha1(certificate).hexdigest().upper()
        run(['/usr/bin/openssl', 'pkcs12', '-export', '-name', NAME, '-inkey', str(folder/'key.pem'), '-in', str(folder/'certificate.pem'), '-out', str(folder/'identity.p12'), '-passout', 'file:'+str(secret)])
        password = secrets.token_urlsafe(32)
        PASSWORD.write_text(password); PASSWORD.chmod(0o600)
        chain = shlex.quote(str(KEYCHAIN)); quoted = shlex.quote(password)
        security('create-keychain -p ' + quoted + ' ' + chain)
        security('set-keychain-settings ' + chain)
        security('unlock-keychain -p ' + quoted + ' ' + chain)
        security('import ' + shlex.quote(str(folder/'identity.p12')) + ' -k ' + chain + ' -P ' + shlex.quote(transfer) + ' -T /usr/bin/codesign')
        security('set-key-partition-list -S apple-tool:,apple:,codesign: -s -k ' + quoted + ' ' + chain)
        (DIRECTORY/'certificate.der').write_bytes(certificate)
        value = {'name': NAME, 'sha1': fingerprint, 'keychain': str(KEYCHAIN), 'certificate': str(DIRECTORY/'certificate.der'), 'scope': 'Local code identity only; no system or TLS trust added'}
        CONFIG.write_text(json.dumps(value, indent=2)+'\n'); CONFIG.chmod(0o600)
    return identity()


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--setup',action='store_true');args=parser.parse_args()
    print(json.dumps(setup() if args.setup else identity()))

if __name__=='__main__':main()
