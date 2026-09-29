"""T718 isolated PipeWire acceptance: no physical devices, no stored audio."""
import tempfile,subprocess,os,time,json,select,threading,struct,sys
with tempfile.TemporaryDirectory(prefix='blent-pw-') as d:
 env=dict(os.environ,XDG_RUNTIME_DIR=d,PIPEWIRE_RUNTIME_DIR=d)
 server=subprocess.Popen(['pipewire'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 clients=[]
 try:
  for _ in range(50):
   if os.path.exists(d+'/pipewire-0'):break
   time.sleep(.02)
  source=subprocess.Popen([sys.argv[1],'40','blent_microphone_t718_private'],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE);clients.append(source)
  assert select.select([source.stdout],[],[],5)[0], 'ready timeout'
  assert source.stdout.readline()==b'READY\n'
  consumer=subprocess.Popen(['pw-cat','--record','--target','0','--rate','48000','--channels','1','--format','s16','-'],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE);clients.append(consumer)
  time.sleep(.2)
  nodes=json.loads(subprocess.check_output(['pw-dump'],env=env))
  for n in nodes:
   name=n.get('info',{}).get('props',{}).get('node.name')
   if name not in ('blent_microphone_t718_private','pw-cat'):continue
   direction='Output' if name.startswith('blent_') else 'Input'
   parameter='{ direction: '+direction+', mode: dsp, format: { mediaType: audio, mediaSubtype: raw, format: F32P, rate: 48000, channels: 1, position: [ MONO ] } }'
   subprocess.run(['pw-cli','set-param',str(n['id']),'PortConfig',parameter],env=env,check=True,stdout=subprocess.DEVNULL)
  time.sleep(.1)

  subprocess.run(['pw-link','blent_microphone_t718_private:capture_MONO','pw-cat:input_MONO'],env=env,check=True)
  def feed():
   for _ in range(100):
    source.stdin.write(b'\0'+struct.pack('<480h',*([1234]*480)));source.stdin.flush();time.sleep(.01)
  t=threading.Thread(target=feed);t.start()
  data=bytearray();deadline=time.monotonic()+1.3
  while time.monotonic()<deadline:
   if select.select([consumer.stdout],[],[],.1)[0]:data+=consumer.stdout.read1(4096)
  t.join(); source.stdin.close();source.wait(timeout=2)
  samples=struct.unpack('<%dh'%(len(data)//2),data)
  assert len(samples) > 4800
  assert 1234 in samples, 'native source lost PCM'
  assert set(samples[-480:]) == {0}, 'native underflow repeated stale speech'
  remaining=json.loads(subprocess.check_output(['pw-dump'],env=env))
  assert not any(n.get('info',{}).get('props',{}).get('node.name')=='blent_microphone_t718_private' for n in remaining)
  consumer.terminate();consumer.wait(timeout=2)
  import audio_transport
  audio_transport.check(sys.argv[2], env, d)
  for args in [[], ['0','bad'], ['201','bad'], ['40','bad/name']]:
   assert subprocess.run([sys.argv[1], *args], env=env, capture_output=True, timeout=2).returncode != 0
  failed=subprocess.Popen([sys.argv[1],'40','blent_microphone_failure'],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE);clients.append(failed)
  assert select.select([failed.stdout],[],[],5)[0], 'failure fixture readiness timeout'
  assert failed.stdout.readline()==b'READY\n'
  server.terminate();server.wait(timeout=2)
  failed.wait(timeout=2)  # T718 graph loss must retire the adapter with stdin still open.
 finally:
  for p in clients:
   if p.poll() is None:p.terminate();p.wait(timeout=2)
  server.terminate();server.wait(timeout=2)
