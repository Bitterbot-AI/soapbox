#!/usr/bin/env python3
"""Package an already-built Linux runtime from an explicit, secret-free allowlist."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tomllib

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
version = tomllib.loads((root/'crates/soapbox/Cargo.toml').read_text())['package']['version']
assert subprocess.check_output([str(args.binary.resolve()), '--version'], text=True).strip() == 'soapbox '+version
revision = subprocess.check_output(['git','-C',str(root),'rev-parse','HEAD'], text=True).strip()
name = f'soapbox-runtime-{version}-linux-x64'
directory = args.output/name
directory.mkdir(parents=True, exist_ok=False)
def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

assets = ['LICENSE','containers/Dockerfile','scripts/start-container-engine.sh',
          'scripts/soapbox-docker-service','scripts/install-wsl-docker-service.py',
          'docs/OPERATOR-HEALTH.md','docs/OPERATOR-HEALTH-VALIDATION.md','docs/RUNTIME-RC9.md',
          'docs/RUNTIME-RC10.md']
for name_in in assets:
    target = directory/name_in
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(root/name_in, target)
    target.chmod(0o755 if name_in.startswith('scripts/') else 0o644)
(directory/'bin').mkdir()
shutil.copyfile(args.binary, directory/'bin/soapbox')
(directory/'bin/soapbox').chmod(0o755)
files = sorted([root/'Cargo.lock',root/'Cargo.toml',root/'rust-toolchain.toml',
                root/'crates/soapbox/Cargo.toml',*(root/'crates/soapbox').rglob('*.rs'),
                *(root/p for p in assets)])
digests = {str(p.relative_to(root)):sha(p) for p in files}
tree = hashlib.sha256(json.dumps(digests,sort_keys=True).encode()).hexdigest()
payload = {str(p.relative_to(directory)):sha(p) for p in sorted(directory.rglob('*')) if p.is_file()}
(directory/'provenance.json').write_text(json.dumps({'version':version,'source_revision':revision,
    'source_tree_sha256':tree,'files':digests,'payload_sha256':payload,
    'binary_sha256':sha(directory/'bin/soapbox'),'chain_id':84532},indent=2)+'\n')
archive = args.output/(directory.name+'.tar.gz')
with archive.open('wb') as output, gzip.GzipFile(filename='', mode='wb', fileobj=output, mtime=0) as zipped:
    with tarfile.open(fileobj=zipped,mode='w') as tar:
        for path in [directory,*sorted(directory.rglob('*'))]:
            assert not path.is_symlink()
            info = tar.gettarinfo(str(path),arcname=str(path.relative_to(args.output)))
            info.uid = info.gid = 0
            info.uname = info.gname = ''
            info.mtime = 0
            if path.is_file():
                with path.open('rb') as data: tar.addfile(info,data)
            else: tar.addfile(info)
meta = {'version':version,'filename':archive.name,'directory':directory.name,
        'bytes':archive.stat().st_size,'sha256':sha(archive),'source_tree_sha256':tree,
        'source_revision':revision}
(args.output/'runtime.json').write_text(json.dumps(meta,indent=2)+'\n')
(args.output/'SHA256SUMS').write_text(f"{meta['sha256']}  {archive.name}\n")
print(json.dumps(meta,indent=2))
