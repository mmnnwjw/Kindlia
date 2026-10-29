"""从 cargo metadata 输出生成当前依赖许可清单，保留单独的上游原始声明。"""
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1]
metadata = json.loads((root / 'build/metadata.json').read_text(encoding='utf-8-sig'))
header = '''# Third-party notices

Kindlia is derived from [KinNovel](https://github.com/mmnnwjw/KinNovel) at
`3f89ad2953bfda5378d9856aea5bb4c7ebc09265`. Original code attribution and license
notices are retained. Kindlia is distributed under GPL-3.0-or-later; see LICENSE.
The original upstream notice is preserved in
[docs/UPSTREAM-THIRD-PARTY-NOTICES.md](docs/UPSTREAM-THIRD-PARTY-NOTICES.md).
That document records KinNovel's components and fonts; components absent from
Kindlia are not implied to be distributed here.

## Native code and data

- FBInk, commit `886f25f13368859ad8a899b88d04c26e19cda32e`, GPL-3.0-or-later:
  https://github.com/NiLuJe/FBInk. Source and license in `rust/third_party/FBInk/`.
- libwebp decoder, commit `a4d7a715337ded4451fec90ff8ce79728e04126c`, BSD-3-Clause:
  https://github.com/webmproject/libwebp. Copyright (c) 2010, Google Inc. All rights
  reserved. Source, COPYING and PATENTS in `rust/third_party/libwebp/`.
- rime-pinyin-simp dictionary, Apache-2.0:
  https://github.com/rime/rime-pinyin-simp. Embedded as `kn-ime/data/pinyin.br`;
  upstream license in `rust/crates/kn-ime/data/LICENSE-rime-pinyin-simp`.
- zhconv-rs (GPL-2.0-or-later) embeds OpenCC conversion data; see
  https://github.com/Gowee/zhconv-rs and https://github.com/BYVoid/OpenCC.
- Default cover: https://n-cdn.novelia.cc/static/no_cover.c90924af53e4.webp,
  bundled at the user's request as assets/no_cover.webp; artwork rights remain
  with its respective owner.
- No additional application fonts are bundled. Fonts selected via config.toml
  retain their own licenses.

The AutoNovel protocol was checked against
https://github.com/auto-novel/auto-novel at
`ce6fcb3c49675097d78b1d87b06eb8390c7db963` (GPL-3.0). Novel text and illustrations
remain the property of their respective rights holders and are not shipped here.

## Resolved Rust dependencies

Generated from `cargo metadata --format-version 1 --locked --offline` for the
current Cargo.lock. This includes build dependencies and platform-specific
packages; not every row is linked into every target. Registry source packages
carry their complete license texts.

| Crate | Version | Declared license |
|---|---|---|
'''
rows = []
for package in sorted(metadata['packages'], key=lambda p: (p['name'], p['version'])):
    if package.get('source'):
        rows.append(f'| {package["name"]} | {package["version"]} | {package.get("license") or "See package license file"} |')
(root / 'THIRD-PARTY-NOTICES.md').write_text(header + '\n'.join(rows) + '\n', encoding='utf-8')
print(f'{len(rows)} dependency license entries')
