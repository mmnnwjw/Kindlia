"""Prepare files for manual R2 upload. This script never uploads anything."""
import argparse,hashlib,json,shutil,datetime,tomllib
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--package',type=Path,required=True);p.add_argument('--output',type=Path,default=Path('dist/r2'));p.add_argument('--history',type=Path);p.add_argument('--notes',type=Path);a=p.parse_args()
root=Path(__file__).resolve().parents[1];version=tomllib.loads((root/'rust/Cargo.toml').read_text(encoding='utf-8'))['workspace']['package']['version']
if a.package.name!=f'kindlia-{version}.zip':p.error('Package name must match current version')
sig=a.package.with_name(a.package.name+'.sig')
if not sig.is_file():p.error('Run tools/sign_update.py first')
# Verify both the payload and signature before preparing a mirror feed.
from nacl.signing import VerifyKey
import re
public=re.search(r'PUBLIC_KEY = "([0-9a-f]+)"',(root/'tools/sign_update.py').read_text(encoding='utf-8'))[1]
sha=hashlib.sha256(a.package.read_bytes()).hexdigest();VerifyKey(bytes.fromhex(public)).verify(f'kindlia-update-v1\n{a.package.name}\n{sha}\n'.encode(),bytes.fromhex(sig.read_text().strip()))
now=datetime.datetime.now(datetime.timezone.utc).isoformat().replace('+00:00','Z')
entry={'tag_name':'v'+version,'name':'Kindlia '+version,'published_at':now,'created_at':now,'prerelease':False,'draft':False,'body':a.notes.read_text(encoding='utf-8') if a.notes else '', 'assets':[{'name':a.package.name,'size':a.package.stat().st_size,'digest':'sha256:'+sha}]}
history=json.loads(a.history.read_text(encoding='utf-8')) if a.history else []
if not isinstance(history,list):p.error('History must be a JSON array')
a.output.mkdir(parents=True,exist_ok=True)
for src in [a.package,sig]:
 target=a.output/src.name
 if target.resolve()!=src.resolve():shutil.copyfile(src,target)
(a.output/'releases.json').write_text(json.dumps([entry]+[r for r in history if r.get('tag_name')!=entry['tag_name']],ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
print('Prepared for manual upload to bucket ROOT:')
for n in ['releases.json',a.package.name,sig.name]:print(n)
