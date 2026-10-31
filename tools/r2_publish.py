"""同步 Kindlia GitHub Release 到 R2 的 kindlia-updates 桶。

上传 ZIP、独立 Ed25519 签名，再发布 releases.json；经 update1.mmnnwjw.dpdns.org
回读校验。客户端以 SHA256 查询参数规避同名资产替换后的旧边缘缓存。
凭据通过环境变量或 --local-secrets 读取，不输出凭据。
"""
import argparse
import hashlib
import json
import os
import sys
import urllib.request

import boto3
from botocore.config import Config
from nacl.signing import VerifyKey

from sign_update import PUBLIC_KEY

def message(package, sha256):
    return f'kindlia-update-v1\n{package}\n{sha256}\n'.encode()

REPO = 'mmnnwjw/Kindlia'
BUCKET = 'kindlia-updates'
PUBLIC = 'https://update1.mmnnwjw.dpdns.org'
# Cloudflare 会拦 Python-urllib 默认的 User-Agent (403)
UA = 'Kindlia-release/1.0'
KEEP_FIELDS = ('tag_name', 'name', 'published_at', 'created_at', 'prerelease', 'draft', 'body')


def http_get(url, token=None):
    headers = {'User-Agent': UA}
    if token:
        headers['Authorization'] = 'Bearer ' + token
    with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=120) as r:
        return r.read()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('tag')
    ap.add_argument('--zip')
    ap.add_argument('--local-secrets', action='store_true')
    args = ap.parse_args()
    tag = args.tag
    name = f"kindlia-{tag.removeprefix('v')}.zip"

    if args.local_secrets:
        d = os.path.expanduser('~/.kinovel-secrets')
        rd = lambda n: open(os.path.join(d, n), encoding='utf-8').read().strip()
        endpoint, key_id, secret, token = rd('S3_API'), rd('Access_Key_ID'), rd('Secret_Access_Key'), rd('GithubToken')
    else:
        endpoint, key_id, secret = (os.environ[k] for k in ('R2_ENDPOINT', 'R2_ACCESS_KEY_ID', 'R2_SECRET_ACCESS_KEY'))
        token = os.environ.get('GH_TOKEN')

    releases = json.loads(http_get(f'https://api.github.com/repos/{REPO}/releases?per_page=20', token))
    releases = [r for r in releases if not r.get('draft')]
    current = next((r for r in releases if r['tag_name'] == tag), None)
    if current is None:
        sys.exit(f'GitHub 上没有发行版 {tag}')
    asset = next((a for a in current.get('assets', []) if a['name'] == name), None)
    if asset is None:
        sys.exit(f'{tag} 没有资产 {name}')

    if args.zip:
        data = open(args.zip, 'rb').read()
        sig = open(args.zip + '.sig', 'rb').read()
    else:
        data = http_get(asset['browser_download_url'])
        sig = http_get(asset['browser_download_url'] + '.sig')
    digest = hashlib.sha256(data).hexdigest()
    expected = (asset.get('digest') or '').removeprefix('sha256:')
    if expected and expected != digest:
        sys.exit(f'{name}: sha256 {digest} 与 GitHub 资产 {expected} 不一致')
    if len(data) != asset['size']:
        sys.exit(f'{name}: 大小 {len(data)} 与 GitHub 资产 {asset["size"]} 不一致')
    VerifyKey(bytes.fromhex(PUBLIC_KEY)).verify(message(name, digest), bytes.fromhex(sig.decode().strip()))

    listing = []
    for r in releases:
        item = {k: r.get(k) for k in KEEP_FIELDS}
        item['assets'] = [
            {'name': a['name'], 'size': a['size'], 'digest': a.get('digest') or (f'sha256:{digest}' if a is asset else None)}
            for a in r.get('assets', []) if a['name'].endswith('.zip')
        ]
        listing.append(item)
    body = json.dumps(listing, ensure_ascii=False, indent=1).encode('utf-8')

    s3 = boto3.client('s3', endpoint_url=endpoint, aws_access_key_id=key_id, aws_secret_access_key=secret,
                      region_name='auto', config=Config(signature_version='s3v4'))
    s3.put_object(Bucket=BUCKET, Key=name, Body=data, ContentType='application/zip', CacheControl='public, max-age=3600')
    s3.put_object(Bucket=BUCKET, Key=name + '.sig', Body=sig, ContentType='text/plain', CacheControl='public, max-age=3600')
    print(f'uploaded {name} + .sig: {len(data)} B, sha256 {digest}')
    s3.put_object(Bucket=BUCKET, Key='releases.json', Body=body, ContentType='application/json; charset=utf-8',
                  CacheControl='no-cache')
    print(f'uploaded releases.json: {len(listing)} releases, {len(body)} B')

    # 经公开域名读回来
    # 与客户端一样带 ?sha256= (Cloudflare 默认缓存 .zip, 原地替换过的版本不带参数可能读到边缘节点上的旧包)
    got = http_get(f'{PUBLIC}/{name}?sha256={digest}')
    if hashlib.sha256(got).hexdigest() != digest or http_get(f'{PUBLIC}/{name}.sig?sha256={digest}') != sig:
        sys.exit(f'{PUBLIC}/{name} 读回来的内容不一致')
    got = json.loads(http_get(f'{PUBLIC}/releases.json?t={digest[:8]}'))
    if not any(r['tag_name'] == tag for r in got):
        sys.exit(f'{PUBLIC}/releases.json 里没有 {tag}')
    print(f'public check OK: {PUBLIC}/{name}, {PUBLIC}/releases.json')


if __name__ == '__main__':
    main()
