import subprocess as s,time,json,signal,sys,urllib.request,websocket
from pathlib import Path
root=Path('/backups/disk2/blent-work/t621')
records=[]; active=None; handle=None
pages=json.load(urllib.request.urlopen('http://127.0.0.1:19221/json'))
page=next(p for p in pages if p['url'].startswith('http://127.0.0.1:19222'))
ws=websocket.create_connection(page['webSocketDebuggerUrl'],origin='http://localhost:19221',timeout=20)
seq=0

def js(expr):
 global seq
 seq+=1;ws.send(json.dumps({'id':seq,'method':'Runtime.evaluate','params':{'expression':expr,'awaitPromise':True,'returnByValue':True}}))
 while True:
  r=json.loads(ws.recv())
  if r.get('id')==seq:break
 if 'exceptionDetails' in r.get('result',{}):raise RuntimeError(r)
 return r['result']['result'].get('value')

def start(lens,tag):
 global active,handle
 handle=(root/(tag+'.log')).open('w')
 active=s.Popen(['/home/netto/.local/bin/blent','cameras','--lens',lens,'--width','1280','--height','720','--fps','30'],stdout=handle,stderr=s.STDOUT)
 records.append({'stage':'start','lens':lens,'tag':tag,'at':time.time(),'pid':active.pid})

def stop():
 global active,handle
 if active and active.poll() is None:active.send_signal(signal.SIGTERM)
 if active:
  rc=active.wait(timeout=8);records.append({'stage':'stop','at':time.time(),'exit':rc})
 active=None
 if handle:handle.close();handle=None

def sample(stage):
 out={name:js('sampleCamera('+json.dumps(name)+')') for name in ['Blent Front','Blent Rear']}
 records.append({'stage':stage,'at':time.time(),'outputs':out});(root/'acceptance.json').write_text(json.dumps(records,indent=2)+'\n')
 return out

def open_both():
 for name in ['Blent Front','Blent Rear']:
  records.append({'stage':'open','label':name,'settings':js('openCamera('+json.dumps(name)+')')})

try:
 for lens in ['front','rear']:
  start(lens,lens);time.sleep(4);assert active.poll() is None
  open_both();time.sleep(2);data=sample(lens+'-active')
  selected='Blent '+lens.capitalize();inactive='Blent '+('Rear' if lens=='front' else 'Front')
  assert max(v['nonblack'] for v in data[selected]['values'])>0,(lens,'no image')
  assert max(v['max'] for v in data[inactive]['values'])<=8,(lens,'inactive not black')
  stop();time.sleep(.5);data=sample(lens+'-stopped')
  assert all(max(v['max'] for v in d['values'])<=8 for d in data.values()),'stop retained image'
  js('closeCameras()')
 for i,lens in enumerate(['front','rear','front','rear']):
  start(lens,'rapid-'+str(i));time.sleep(.6);stop()
 time.sleep(1)
 start('front','after-rapid');time.sleep(4);open_both();data=sample('after-rapid-active')
 assert max(v['nonblack'] for v in data['Blent Front']['values'])>0
 stop();time.sleep(.5);sample('after-rapid-stopped');js('closeCameras()')
finally:
 stop()
 try:js('closeCameras()')
 finally:ws.close()
 text=s.check_output(['adb','shell','-tt','dumpsys','media.camera'],text=True,timeout=10)
 lines=text.split('Active Camera Clients:',1)[1].split('Allowed user IDs:',1)[0].strip()
 records.append({'stage':'final-camera-clients','value':lines,'at':time.time()})
 records.append({'stage':'reverse-mappings','value':s.check_output(['adb','reverse','--list'],text=True),'at':time.time()})
 (root/'acceptance.json').write_text(json.dumps(records,indent=2)+'\n')
print('Native camera checks completed')
