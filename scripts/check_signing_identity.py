#!/usr/bin/env python3
"""Prove content changes retain the signer requirement and a different signer fails."""
import hashlib
import json
from pathlib import Path
import subprocess
from local_signing import identity

ROOT=Path(__file__).resolve().parents[1]
folder=ROOT/'.cache/signing-check';folder.mkdir(parents=True,exist_ok=True)
config=identity();updates=[];requirement=None
for number in [1,2,3]:
 source=folder/f'v{number}.c';source.write_text(f'int main(void){{return {number-1};}}\n');binary=folder/f'v{number}'
 subprocess.run(['cc',str(source),'-o',str(binary)],check=True)
 signer=['--sign','-'] if number==3 else ['--sign',config['sha1'],'--keychain',config['keychain'],'--timestamp=none']
 subprocess.run(['codesign','--force',*signer,'--identifier','dev.infotransfer.reader',str(binary)],check=True,capture_output=True)
 result=subprocess.run(['codesign','-d','-r-',str(binary)],check=True,capture_output=True,text=True)
 own=(result.stdout+result.stderr).split('designated => ',1)[1].splitlines()[0].strip()
 if number==1:requirement=own
 result=subprocess.run(['codesign','--verify','--strict','-R='+requirement,str(binary)],capture_output=True,text=True)
 details=subprocess.run(['codesign','-d','--verbose=4',str(binary)],check=True,capture_output=True,text=True).stderr
 accepted=result.returncode==0
 assert accepted==(number!=3),result.stderr
 updates.append({'fixture':f'v{number}','file_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'code_directory_hash':next(x.split('=',1)[1] for x in details.splitlines() if x.startswith('CDHash=')),'same_requirement':own==requirement,'accepted_by_original_requirement':accepted})
assert updates[0]['code_directory_hash']!=updates[1]['code_directory_hash']
proof={'requirement':requirement,'fixtures':updates,'same_signer_updates_pass':True,'ad_hoc_impostor_rejected':True,'trust_scope':'Only the local signing certificate identifies this app. No system or TLS trust changed.','os_permission_preservation':'Not yet verified; requires granting the final identity once and checking a real bundle replacement.'}
(ROOT/'docs/permission-signing-proof.json').write_text(json.dumps(proof,indent=2)+'\n')
print(json.dumps({'same_signer_updates_pass':True,'ad_hoc_impostor_rejected':True}))
