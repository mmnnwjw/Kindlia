"""离线性能采样；数据写入独立目录，不连接网络或操纵 Kindle UI。"""
import argparse
import datetime
import json
import os
from pathlib import Path
import subprocess
import time

root = Path(__file__).resolve().parents[1]
p = argparse.ArgumentParser()
p.add_argument('epub', type=Path)
p.add_argument('--binary', type=Path, default=root / 'rust/target/x86_64-pc-windows-gnu/debug/kindlia.exe')
p.add_argument('--output', type=Path)
args = p.parse_args()
stamp = datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f')
data = root / 'build' / ('epub-profile-' + stamp)
data.mkdir(parents=True)
env = os.environ.copy()
env['KINDLIA_HOME'] = str(data)
try:
    import psutil
except ImportError:
    psutil = None
with (data / 'stdout.json').open('w', encoding='utf-8') as out, (data / 'stderr.log').open('w', encoding='utf-8') as err:
    child = subprocess.Popen([str(args.binary.resolve()), '--profile-epub', str(args.epub.resolve())],
                             env=env, stdout=out, stderr=err)
    peak = 0
    proc = psutil.Process(child.pid) if psutil else None
    while child.poll() is None:
        if proc:
            try:
                info = proc.memory_info()
                peak = max(peak, getattr(info, 'peak_wset', info.rss))
            except psutil.Error:
                pass
        time.sleep(0.005)
if child.returncode:
    raise SystemExit(f'Profile failed ({child.returncode}); see {data / "stderr.log"}')
report = json.loads((data / 'stdout.json').read_text(encoding='utf-8'))
report['sampled_peak_working_set_mib'] = round(peak / 1024**2, 2) if proc else None
report['sample_data'] = str(data)
output = args.output or data / 'report.json'
output.parent.mkdir(parents=True, exist_ok=True)
output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
print(output.resolve())
print(json.dumps({k: v for k, v in report.items() if k not in ('chapter_parse', 'key', 'sample_data')}, ensure_ascii=False))
