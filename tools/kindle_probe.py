"""Kindle hardware probe. Explicit start/stop/gesture actions; captures are host-side.
Build tools/kindle_probe.c with zig cc -target arm-linux-musleabihf -static.
Requires paramiko and Pillow. Uses a pinned host fingerprint; never logs session data.
"""
import argparse,json,time,shlex,socket
from pathlib import Path
import paramiko
from PIL import Image, ImageChops
import io
p=argparse.ArgumentParser();p.add_argument('action',choices=['capture','tap','swipe','logs','status','stop','start','reader-cycle']);p.add_argument('values',nargs='*');p.add_argument('--host',required=True);p.add_argument('--port',type=int,default=2222);p.add_argument('--fingerprint',required=True);p.add_argument('--swap',action='store_true');p.add_argument('--mirror-x',action='store_true');p.add_argument('--mirror-y',action='store_true');p.add_argument('--source-address');a=p.parse_args()
source_sock=socket.create_connection((a.host,a.port),10,source_address=(a.source_address,0)) if a.source_address else None
c=paramiko.SSHClient();c.set_missing_host_key_policy(paramiko.AutoAddPolicy());c.connect(a.host,port=a.port,username='root',password='',look_for_keys=False,allow_agent=False,timeout=15,banner_timeout=20,sock=source_sock)
assert c.get_transport().get_remote_server_key().get_fingerprint().hex()==a.fingerprint
root='/mnt/us/extensions/kindlia'
def run(cmd,binary=False):
 _,o,e=c.exec_command(cmd,timeout=30);data=o.read();err=e.read();rc=o.channel.recv_exit_status()
 if rc:raise RuntimeError(err.decode('utf-8','replace'))
 return data if binary else data.decode('utf-8','replace')
try:
 if a.action=='capture':
  time.sleep(.8)
  data=run('/tmp/kindlia-probe capture',True)
  for _ in range(4):
   time.sleep(.15);fresh=run('/tmp/kindlia-probe capture',True)
   if fresh==data:break
   data=fresh
  out=Path(a.values[0]);out.parent.mkdir(parents=True,exist_ok=True);im=Image.open(io.BytesIO(data));im.save(out);print(out.resolve())
 elif a.action in ('tap','swipe'):
  ranges=json.loads(run('/tmp/kindlia-probe info'));v=list(map(int,a.values));raw=[]
  assert len(v)==(2 if a.action=='tap' else 4)
  for i in range(0,len(v),2):
   x,y=v[i:i+2];assert 0<=x<1236 and 0<=y<1648
   if a.mirror_x:x=1235-x
   if a.mirror_y:y=1647-y
   nx,ny=x/1235,y/1647
   if a.swap:nx,ny=ny,nx
   raw.extend([round(ranges['x'][0]+nx*(ranges['x'][1]-ranges['x'][0])),round(ranges['y'][0]+ny*(ranges['y'][1]-ranges['y'][0]))])
  run('/tmp/kindlia-probe '+a.action+' '+' '.join(map(str,raw)));print(a.action, v)
 elif a.action=='reader-cycle':
  # This scenario is deliberately tied to the observed KPW5 light/portrait UI.
  reference=Image.open(a.values[0]).convert('L').crop((0,65,1236,1610))
  count=int(a.values[1]) if len(a.values)>1 else 3
  assert 1<=count<=10
  output=Path(a.values[0]).parent
  def shot():return Image.open(io.BytesIO(run('/tmp/kindlia-probe capture',True))).copy().convert('L')
  def state(im):
   if im.size!=(1236,1648):raise RuntimeError('Scenario requires 1236x1648 portrait')
   bg=im.getpixel((2,2))
   if bg<128:raise RuntimeError('Scenario requires current light mode; preferences are not changed')
   if bg>245:return 'reader'
   dark=sum(n for n,v in im.crop((20,20,100,95)).getcolors(6000) if v<50)
   return 'menu' if dark>100 else 'root'
  def wait(wanted,body=False):
   deadline=time.monotonic()+20
   while time.monotonic()<deadline:
    im=shot()
    if state(im)==wanted and (not body or ImageChops.difference(reference,im.crop((0,65,1236,1610))).getbbox() is None):return im
    time.sleep(.15)
   im.save(output/'cycle-unexpected.png')
   raise RuntimeError('Did not reach '+wanted+' with the expected reading position; saved cycle-unexpected.png')
  def tap(x,y):run(f'/tmp/kindlia-probe tap {x} {y}')
  def memory():
   import re
   return {k:int(v) for k,v in re.findall(r'(VmRSS|VmHWM):\s+(\d+)',run('p=$(pidof kindlia-bin);grep -E "VmRSS|VmHWM" /proc/$p/status'))}
  current=state(shot())
  if current=='root':tap(600,260)
  elif current=='menu':tap(600,830)
  wait('reader',True)
  samples=[memory()]
  for i in range(count):
   tap(600,830);wait('menu');tap(1180,55);wait('root');tap(600,260)
   im=wait('reader',True);im.save(output/f'cycle-stable-{i+1}.png');samples.append(memory());print('validated cycle',i+1,samples[-1],flush=True)
  (output/'validated-cycle-memory.json').write_text(json.dumps(samples,indent=2),encoding='utf-8')
 elif a.action=='logs':print(run('tail -120 '+root+'/logs/kindlia.log'))
 elif a.action=='status':print(run('p=$(pidof kindlia-bin || true); echo pid=$p; if [ -n "$p" ]; then cat /proc/$p/status | grep -E "VmRSS|VmHWM|Threads"; cat /proc/$p/stat; fi; cat /proc/uptime; cat /sys/class/power_supply/bd71827_bat/capacity'))
 elif a.action=='stop':
  run('p=$(pidof kindlia-bin || true); if [ -n "$p" ]; then kill -TERM $p; fi')
  for _ in range(30):
   if not run('pidof kindlia-bin 2>/dev/null || true').strip():break
   time.sleep(.25)
  else:raise RuntimeError('Kindlia did not exit after SIGTERM')
  time.sleep(.3);print('Kindlia stopped')
 elif a.action=='start':
  pid=run('pidof kindlia-bin 2>/dev/null || true').strip()
  if pid:print('Kindlia already running:',pid)
  else:
   lines=int(run('wc -l < '+root+'/logs/kindlia.log').strip())
   run('nohup env KN_DEBUG=1 sh '+root+'/bin/kindlia.sh >/tmp/kindlia-test-launch.log 2>&1 </dev/null &')
   for _ in range(30):
    time.sleep(.25)
    if '[frame]' in run('tail -n +'+str(lines+1)+' '+root+'/logs/kindlia.log'):break
   else:raise RuntimeError('No rendered frame after starting Kindlia; inspect launch log')
   print('Kindlia first frame ready')
finally:c.close()
