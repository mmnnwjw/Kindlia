"""记录复用文件来源与当前差异；只读 KinNovel，输出写在 Kindlia docs/。"""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--source', type=Path, required=True)
args = parser.parse_args()
source = args.source.resolve()
if source == root:
    parser.error('Source must be the original KinNovel workspace')


def git(directory, *arguments):
    return subprocess.check_output(['git', '-C', str(directory), *arguments], encoding='utf-8').strip()


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


files = []
for name in git(source, 'ls-files').splitlines():
    current = root / name
    original = source / name
    if original.is_file() and current.is_file() and name != 'README.md':
        files.append({'path': name, 'source_sha256': sha(original), 'current_sha256': sha(current),
                      'modified': original.read_bytes() != current.read_bytes()})
native = []
for name in ('FBInk', 'libwebp'):
    directory = source / 'rust/third_party' / name
    count = 0
    for relative in git(directory, 'ls-files').splitlines():
        current = root / 'rust/third_party' / name / relative
        original = directory / relative
        if original.is_file() and current.is_file():
            files.append({'path': str(current.relative_to(root)).replace('\\', '/'),
                          'source_sha256': sha(original), 'current_sha256': sha(current),
                          'modified': original.read_bytes() != current.read_bytes()})
            count += 1
    native.append({'name': name, 'commit': git(directory, 'rev-parse', 'HEAD'), 'files': count})
manifest = {'source': 'https://github.com/mmnnwjw/KinNovel', 'commit': git(source, 'rev-parse', 'HEAD'),
            'native': native, 'files': files}
output = root / 'docs/UPSTREAM-FILES.json'
output.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
print(f'{len(files)} reused files; {sum(item["modified"] for item in files)} modified; {output}')
