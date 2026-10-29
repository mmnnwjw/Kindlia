"""打包现成的 Kindle 二进制；不部署到设备、不携带用户数据。"""
from pathlib import Path
import argparse
import zipfile
import struct

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--binary', type=Path, default=root / 'rust/target/armv7-unknown-linux-musleabihf/release/kindlia')
parser.add_argument('--output', type=Path, default=root / 'dist/kindlia-0.2.7.zip')
args = parser.parse_args()
launcher = (root / 'bin/kindlia.sh').read_bytes()
if b'\r' in launcher:
    parser.error('bin/kindlia.sh must use Unix LF line endings for Kindle sh')
if not args.binary.is_file():
    parser.error(f'Kindle binary not found: {args.binary}; run rust/build.ps1 kindle first')
elf = args.binary.read_bytes()
if len(elf) < 52 or elf[:6] != b'\x7fELF\x01\x01' or struct.unpack_from('<H', elf, 18)[0] != 40:
    parser.error('Expected a 32-bit little-endian ARM ELF binary')
phoff = struct.unpack_from('<I', elf, 28)[0]
phsize, phnum = struct.unpack_from('<HH', elf, 42)
if phsize < 32 or phoff + phsize * phnum > len(elf):
    parser.error('Invalid ELF program header table')
if any(struct.unpack_from('<I', elf, phoff + i * phsize)[0] == 3 for i in range(phnum)):
    parser.error('Expected a statically linked binary without an ELF interpreter')
args.output.parent.mkdir(parents=True, exist_ok=True)
with zipfile.ZipFile(args.output, 'w', zipfile.ZIP_DEFLATED) as out:
    for source, target in [
        (args.binary, 'kindlia/bin/kindlia'),
        (root / 'bin/kindlia.sh', 'kindlia/bin/kindlia.sh'),
        (root / 'config.xml', 'kindlia/config.xml'),
        (root / 'menu.json', 'kindlia/menu.json'),
        (root / 'config.example.toml', 'kindlia/config.example.toml'),
        (root / 'README.md', 'kindlia/README.md'),
        (root / 'LICENSE', 'kindlia/LICENSE'),
        (root / 'THIRD-PARTY-NOTICES.md', 'kindlia/THIRD-PARTY-NOTICES.md'),
        (root / 'docs/UPSTREAM-THIRD-PARTY-NOTICES.md', 'kindlia/docs/UPSTREAM-THIRD-PARTY-NOTICES.md'),
        (root / 'docs/GATEWAY.md', 'kindlia/docs/GATEWAY.md'),
        (root / 'rust/crates/kn-ime/data/LICENSE-rime-pinyin-simp', 'kindlia/licenses/LICENSE-rime-pinyin-simp'),
        (root / 'rust/third_party/libwebp/COPYING', 'kindlia/licenses/libwebp-COPYING'),
        (root / 'rust/third_party/libwebp/PATENTS', 'kindlia/licenses/libwebp-PATENTS'),
    ]:
        out.write(source, target)
    # Keep the illustrated setup guide readable in an extracted release package.
    for source in sorted((root / 'docs/images/gateway').iterdir()):
        if source.suffix.lower() not in ('.jpg', '.png'):
            continue
        out.write(source, 'kindlia/docs/images/gateway/' + source.name)
print(args.output.resolve())
