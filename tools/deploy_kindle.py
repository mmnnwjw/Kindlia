"""保留用户数据的原位更新；程序、迁移数据和备份均位于 Kindlia 扩展目录。"""
import argparse
import datetime
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import shlex
import socket
import tarfile
import uuid
import zipfile
import paramiko

root = Path(__file__).resolve().parents[1]
p = argparse.ArgumentParser()
p.add_argument('--host', required=True)
p.add_argument('--port', type=int, default=2222)
p.add_argument('--source-address', help='Optional local interface address for direct LAN access')
p.add_argument('--fingerprint', required=True)
p.add_argument('--package', type=Path, default=root / 'dist/kindlia-1.0.0.zip')
p.add_argument('--clean', action='store_true', help='仅在用户明确要求清洁安装时使用')
args = p.parse_args()
token = uuid.uuid4().hex[:10]
stamp = datetime.datetime.now(datetime.timezone(datetime.timedelta(hours=8))).strftime('%Y%m%d-%H%M%S') + '-' + token
target = '/mnt/us/extensions/kindlia'
stage = target + '/.updates/' + token
archive_path = stage + '/package.tar'
backup = target + '/backups/' + stamp
probe = '/tmp/kindlia-install-check-' + token
expected = {}
memory = io.BytesIO()
with zipfile.ZipFile(args.package) as package:
    with tarfile.open(fileobj=memory, mode='w') as tar:
        for name in package.namelist():
            parts = PurePosixPath(name).parts
            if not name.startswith('kindlia/') or '..' in parts or '\\' in name or len(parts) < 2 or parts[1] in ('backups', '.updates', 'data', 'cache', 'fonts', 'books', 'logs') or parts[1] == 'config.toml':
                raise RuntimeError('Unexpected package path: ' + name)
            payload = package.read(name)
            info = tarfile.TarInfo(name)
            info.size = len(payload)
            info.mode = 0o644
            tar.addfile(info, io.BytesIO(payload))
            expected[name.removeprefix('kindlia/')] = hashlib.sha256(payload).hexdigest()

client = paramiko.SSHClient()
client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
source_sock = socket.create_connection((args.host,args.port),timeout=10,source_address=(args.source_address,0)) if args.source_address else None
client.connect(args.host, port=args.port, username='root', password='', look_for_keys=False, allow_agent=False, timeout=15, banner_timeout=20, sock=source_sock)
if client.get_transport().get_remote_server_key().get_fingerprint().hex() != args.fingerprint.lower():
    client.close()
    raise RuntimeError('Device SSH host key mismatch')

def run(command, timeout=30):
    _, out, err = client.exec_command('set -e\n' + command, timeout=timeout)
    output = out.read().decode('utf-8', 'replace')
    error = err.read().decode('utf-8', 'replace')
    if out.channel.recv_exit_status():
        raise RuntimeError(f'Device operation failed: {error}')
    return output

def exists(path):
    return run('if test -e ' + shlex.quote(path) + '; then echo yes; fi').strip() == 'yes'

def verify(directory):
    actual = {}
    for line in run('cd ' + shlex.quote(directory) + '\nsha256sum ' + ' '.join(shlex.quote(n) for n in expected)).splitlines():
        checksum, name = line.split(None, 1)
        actual[name.lstrip('*')] = checksum.lower()
    if actual != expected:
        raise RuntimeError('Installed files do not match package')

def checksum(path):
    if not exists(path):
        return None
    return run('sha256sum ' + shlex.quote(path)).split()[0]

installed = []
saved = set()
committed = False
try:
    run('mkdir -p ' + shlex.quote(stage))
    stdin, out, err = client.exec_command('cat > ' + shlex.quote(archive_path), timeout=30)
    stdin.channel.sendall(memory.getvalue())
    stdin.channel.shutdown_write()
    if out.channel.recv_exit_status():
        raise RuntimeError('Upload failed')
    run('tar -xf ' + shlex.quote(archive_path) + ' -C ' + shlex.quote(stage))
    verify(stage + '/kindlia')
    run('sh -n ' + shlex.quote(stage + '/kindlia/bin/kindlia.sh'))
    help_output = run('cp ' + shlex.quote(stage + '/kindlia/bin/kindlia') + ' ' + probe + '\nchmod 700 ' + probe + '\n' + probe + ' --help')
    if not help_output.startswith('Kindlia'):
        raise RuntimeError('Unexpected executable output')
    # Let the running app save progress and return the display before touching data.
    run('pids=$(pidof kindlia-bin 2>/dev/null || true)\nif [ -n "$pids" ]; then kill -TERM $pids; for i in 1 2 3 4 5 6 7 8 9 10; do pidof kindlia-bin >/dev/null 2>&1 || break; sleep 1; done; if pidof kindlia-bin >/dev/null 2>&1; then exit 3; fi; fi')
    active_config = '/mnt/us/kindlia/config.toml' if exists('/mnt/us/kindlia/config.toml') else target + '/config.toml'
    config_hash = checksum(active_config)
    run('mkdir -p ' + shlex.quote(backup + '/program'))
    if config_hash:
        run('cp -p ' + shlex.quote(active_config) + ' ' + shlex.quote(backup + '/config.toml'))
    for name in expected:
        old = target + '/' + name
        if exists(old):
            destination = backup + '/program/' + name
            run('mkdir -p ' + shlex.quote(str(PurePosixPath(destination).parent)) + '\ncp -p ' + shlex.quote(old) + ' ' + shlex.quote(destination))
            saved.add(name)
    for name in expected:
        destination = target + '/' + name
        run('mkdir -p ' + shlex.quote(str(PurePosixPath(destination).parent)) + '\nmv ' + shlex.quote(stage + '/kindlia/' + name) + ' ' + shlex.quote(destination))
        installed.append(name)
    verify(target)
    # New code is verified before migration. Its journal handles migration I/O rollback.
    migration_output = run('KINDLIA_HOME=' + shlex.quote(target) + ' ' + probe + ' --migrate-data', timeout=60)
    if 'migration complete' not in migration_output:
        raise RuntimeError('Migration did not confirm completion')
    committed = True
    if args.clean:
        reserved = {'backups', '.updates'} | {PurePosixPath(n).parts[0] for n in expected}
        sftp = client.open_sftp()
        for name in sftp.listdir(target):
            if name not in reserved:
                run('mkdir -p ' + shlex.quote(backup + '/data') + '\nmv ' + shlex.quote(target + '/' + name) + ' ' + shlex.quote(backup + '/data/' + name))
        sftp.close()
    elif checksum(target + '/config.toml') != config_hash:
        raise RuntimeError('Configuration hash changed during migration')
    run('mkdir -p ' + target + '/fonts ' + target + '/books\nsync')
    # Stage contains only files created by this installer; data never lives there.
    run('rm -f ' + probe + ' ' + shlex.quote(archive_path) + '\nfind ' + shlex.quote(stage) + r' -depth -type d -exec rmdir {} \;')
    report = {'host': args.host, 'port': args.port, 'installed_at': stamp, 'clean_install': args.clean,
              'data_preserved': not args.clean, 'configuration_sha256': config_hash,
              'backup': backup, 'target': target, 'data': target, 'legacy_data_migrated': not exists('/mnt/us/kindlia'),
              'binary_sha256': expected['bin/kindlia'], 'files_verified': len(expected),
              'package': str(args.package.resolve()), 'package_sha256': hashlib.sha256(args.package.read_bytes()).hexdigest(),
              'device_help_passed': True, 'launched_ui': False}
    (root / 'build/deployment.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, indent=2))
except Exception:
    if not committed:
        for name in reversed(installed):
            current = target + '/' + name
            failed = backup + '/failed-new/' + name
            if exists(current):
                run('mkdir -p ' + shlex.quote(str(PurePosixPath(failed).parent)) + '\nmv ' + shlex.quote(current) + ' ' + shlex.quote(failed))
            if name in saved:
                run('cp -p ' + shlex.quote(backup + '/program/' + name) + ' ' + shlex.quote(current))
    else:
        print('Program and data migration committed; keeping new version and internal backups for recovery.')
    raise
finally:
    client.close()
