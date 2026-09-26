import datetime,gzip,json,re
from pathlib import Path
root=Path(__file__).resolve().parent
text=gzip.open(root.parent/'t612/native-auto.log.gz','rt').read()
ready={};acks={}
for line in text.splitlines():
    match=re.search(r'encoder_epoch=(\d+) sequence=(\d+)',line)
    if not match: continue
    identity=tuple(map(int,match.groups()))
    try: at=datetime.datetime.fromisoformat(line[:27].replace('Z','+00:00')).timestamp()
    except ValueError: continue
    if 'Packet ready' in line: ready[identity]=at
    elif 'Render ACK received' in line:
        us=int(re.search(r'packet_ready_to_ack_us=(\d+)',line)[1]);acks[identity]=(at,us)
def percentile(values,p): return sorted(values)[int((len(values)-1)*p)] if values else None
def socket_summary(rows,seconds):
    sockets={}
    for port in rows[0]['sockets']:
        texts=[r['sockets'][port] for r in rows];amount={}
        for field in ['bytes_acked','bytes_received','bytes_sent']:
            first=re.search(field+r':(\d+)',texts[0]);last=re.search(field+r':(\d+)',texts[-1])
            if first and last: amount[field+'_mbps']=round((int(last[1])-int(first[1]))*8/seconds/1e6,4)
        queues=[int(m[1]) for t in texts for m in re.finditer(r'ESTAB\s+\d+\s+(\d+)\s',t)]
        amount['max_sampled_sendq_bytes']=max(queues,default=None);sockets[port]=amount
    return sockets

def summarize(name):
    rows=json.loads(gzip.open(root/(name+'.gz'),'rt').read())['samples']
    camera_ports=[p for p in rows[0]['sockets'] if p!='8890']
    if camera_ports: rows=[r for r in rows if 'ESTAB' in r['sockets'][camera_ports[0]]]
    start,end=rows[0]['at'],rows[-1]['at'];seconds=end-start
    initial={(p['pid'],p['identity']):p for p in rows[0]['processes']};final={(p['pid'],p['identity']):p for p in rows[-1]['processes']}
    cpu=[dict(pid=p['pid'],name=p['name'],cpu_seconds=round(p['cpu_s']-initial[k]['cpu_s'],3),peak_sampled_rss=max(x['rss_bytes'] for r in rows for x in r['processes'] if x['pid']==p['pid'])) for k,p in final.items() if k in initial]
    timing=[acks[k][1] for k,at in ready.items() if start<=at<=end and k in acks]
    outputs=[k for k,at in ready.items() if start<=at<=end]
    sockets=socket_summary(rows,seconds)
    return {'seconds':round(seconds,3),'display_encoded':len(outputs),'display_acks':len(timing),'display_ack_fps':round(len(timing)/seconds,2),'display_ack_ms_p50_p95_p99':[round(percentile(timing,p)/1000,3) for p in [.5,.95,.99]],'processes':cpu,'local_tcp':sockets,'start':start,'end':end}
camera=json.loads((root/'camera.json').read_text());times=camera.pop('decoded_ms');camera['decoded_fps']=round(camera['frames']/camera['seconds'],2);camera['longest_decoded_gap_ms']=max(b-a for a,b in zip(times,times[1:]))
result={'display_only':summarize('display-only.json'),'combined':summarize('combined.json'),'camera':camera,'limits':'Uncontrolled desktop content; local TCP staging counters, not USB utilization; camera boundary stops at decoded output, no V4L2 or presentation.'}
(root/'summary.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
