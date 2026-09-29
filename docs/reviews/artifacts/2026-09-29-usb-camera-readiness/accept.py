"""T540 operator-assisted check. Run only with a cable operator ready.
Uses T621's local real-camera page and isolated CDP browser. No images retained.
"""
import subprocess as s,time,json,signal,urllib.request,websocket
from pathlib import Path
root=Path('/backups/disk2/blent-work/t540')
records=[];active=None;handle=None;seq=0
pages=json.load(urllib.request.urlopen('http://127.0.0.1:19221/json'))
page=next(p for p in pages if p['url'].startswith('http://127.0.0.1:19222'))
ws=websocket.create_connection(page['webSocketDebuggerUrl'],origin='http://localhost:19221',timeout=20)

def js(expr):
 global seq
 seq+=1;ws.send(json.dumps({'id':seq,'method':'Runtime.evaluate','params':{'expression':expr,'awaitPromise':True,'returnByValue':True}}))
 while True:
  r=json.loads(ws.recv())
  if r.get('id')==seq:break
 if 'exceptionDetails' in r.get('result',{}):raise RuntimeError(r)
 return r['result']['result'].get('value')

def note(stage,**values):
 records.append(dict(stage=stage,at=time.time(),**values))
 (root/'acceptance.json').write_text(json.dumps(records,indent=2)+'\n')

def start(tag):
 global active,handle
 handle=(root/(tag+'.log')).open('w')
 active=s.Popen(['/home/netto/.local/bin/blent','cameras','--lens','front','--width','1280','--height','720'],stdout=handle,stderr=s.STDOUT)
 time.sleep(4);assert active.poll() is None
 js('openCamera("Blent Front")');js('openCamera("Blent Rear")')
 note(tag,front=js('sampleCamera("Blent Front")'),rear=js('sampleCamera("Blent Rear")'))
 assert max(v['nonblack'] for v in records[-1]['front']['values'])>0
 assert max(v['max'] for v in records[-1]['rear']['values'])<=8

def stop():
 global active,handle
 if active and active.poll() is None:active.send_signal(signal.SIGTERM)
 if active:
  try:active.wait(timeout=10)
  except s.TimeoutExpired:
   active.kill();active.wait(timeout=3)
 active=None
 if handle:handle.close();handle=None

def connected():
 try:
  p=s.run(['adb','-s','8002RH1010011900','get-state'],capture_output=True,text=True,timeout=2)
  return p.returncode==0 and p.stdout.strip()=='device'
 except s.TimeoutExpired:return False

def wait_state(wanted,seconds):
 deadline=time.monotonic()+seconds
 while time.monotonic()<deadline:
  if connected()==wanted:return
  time.sleep(.3)
 raise TimeoutError('Physical USB transition not observed')

try:
 note('before',display_pid=s.check_output(['systemctl','--user','show','blent.service','-p','MainPID','--value'],text=True).strip())
 start('before-unplug')
 note('operator-ready');print('READY: unplug tablet USB, wait five seconds, then reconnect.',flush=True)
 wait_state(False,90);note('adb-disconnected');time.sleep(3)
 data={name:js('sampleCamera('+json.dumps(name)+')') for name in ['Blent Front','Blent Rear']}
 note('unplugged-output',outputs=data)
 assert all(max(v['max'] for v in d['values'])<=8 for d in data.values()),'USB loss retained camera image'
 wait_state(True,90);note('adb-reconnected');time.sleep(3)
 text=s.check_output(['adb','shell','-tt','dumpsys','media.camera'],text=True,timeout=10)
 clients=text.split('Active Camera Clients:',1)[1].split('Allowed user IDs:',1)[0].strip()
 note('reconnect-before-start',camera_clients=clients,mappings=s.check_output(['adb','reverse','--list'],text=True))
 assert clients=='[]','Camera restarted without fresh selection'
 data=js('sampleCamera("Blent Front")');note('reconnected-output',front=data)
 assert max(v['max'] for v in data['values'])<=8
 stop();js('closeCameras()')
 start('explicit-restart');stop();time.sleep(1)
 data=js('sampleCamera("Blent Front")');note('final-output',front=data)
 assert max(v['max'] for v in data['values'])<=8
 js('closeCameras()')
 deadline=time.monotonic()+10
 while True:
  text=s.check_output(['adb','shell','-tt','dumpsys','media.camera'],text=True,timeout=5)
  clients=text.split('Active Camera Clients:',1)[1].split('Allowed user IDs:',1)[0].strip()
  if clients=='[]':break
  assert time.monotonic()<deadline,'Camera resources did not retire'
  time.sleep(.3)
 mappings=s.check_output(['adb','reverse','--list'],text=True)
 note('final-resources',camera_clients=clients,mappings=mappings)
 assert 'tcp:8890 tcp:8890' in mappings and 'tcp:8891 tcp:8891' in mappings
 assert len([line for line in mappings.splitlines() if line.strip()])==2,'Unexpected retained mapping; inspect ownership'
 note('passed-camera-checks')
finally:
 stop()
 try:js('closeCameras()')
 finally:ws.close()
 state=s.check_output(['systemctl','--user','is-active','blent.service'],text=True).strip()
 note('cleanup',display_state=state)
 assert state=='active'
# Also inspect fresh display render ACKs/tablet output: service state alone is not video recovery.
