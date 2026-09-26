from pathlib import Path
import json,statistics
root=Path('/tmp/blent-performance');rows=[]
def stats(values):
 v=sorted(values);return {'min_ms':v[0],'mean_ms':statistics.mean(v),'p50_ms':statistics.median(v),'p95_ms':v[int((len(v)-1)*.95)],'max_ms':v[-1]}
for folder in sorted(root.glob('phase-quiet-*')):
 scene={int(a):(int(b),int(c)) for a,b,c in (l.split() for l in (folder/'scene.log').read_text().splitlines())}
 for entry in json.loads((folder/'summary.json').read_text()):
  trial=folder/entry['trial'];result=json.loads((trial/'result.json').read_text());frames=[tuple(map(int,l.split()[1:])) for l in (trial/'encoder.log').read_text().splitlines() if l.startswith('[gpu-frame]')]
  assert len(frames)==len(entry['decoded_scene_ids'])
  starts=[v[1] for v in frames]
  source_age=[(frame[1]-scene[code][0])/1e6 for frame,code in zip(frames[30:],entry['decoded_scene_ids'][30:])]
  encode=[(packet['ready_ns']-frame[1])/1e6 for packet,frame in zip(result['packets'][30:],frames[30:])]
  rows.append(dict(campaign=folder.name,trial=entry['trial'],cadence=stats([(b-a)/1e6 for a,b in zip(starts,starts[1:])]),source_to_capture=stats(source_age),capture_to_packet=stats(encode),source_to_ack=entry['source_update_to_ack'],packet_to_ack=entry['packet_to_ack'],cache=result['android']['metrics']))
(root/'pacing-analysis.json').write_text(json.dumps(rows,indent=2)+'\n')
for r in rows:print(r['campaign'],r['trial'],'cadence mean',round(r['cadence']['mean_ms'],3),'source age p95',round(r['source_to_capture']['p95_ms'],2),'encode p95',round(r['capture_to_packet']['p95_ms'],2),'ACK p95',round(r['source_to_ack']['p95_ms'],2))
