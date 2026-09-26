import argparse,json,os,re,subprocess,time
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--seconds',type=int,default=20);p.add_argument('--output',required=True);p.add_argument('--camera-port',type=int);a=p.parse_args()
ticks=os.sysconf('SC_CLK_TCK');page=os.sysconf('SC_PAGESIZE');rows=[]
for index in range(a.seconds+1):
    processes=[]
    for path in Path('/proc').glob('[0-9]*/stat'):
        try:
            text=path.read_text();name=text.split('(',1)[1].rsplit(')',1)[0];fields=text.rsplit(') ',1)[1].split()
            if name not in ['blent','helper','ffmpeg','adb','camera-feedback']: continue
            processes.append({'pid':int(path.parent.name),'name':name,'identity':int(fields[19]),'cpu_s':(int(fields[11])+int(fields[12]))/ticks,'rss_bytes':int(fields[21])*page})
        except (OSError,ValueError,IndexError): pass
    ports=[8890]+([a.camera_port] if a.camera_port else [])
    sockets={str(port):subprocess.run(['ss','-tinp',f'sport = :{port}'],capture_output=True,text=True,timeout=2).stdout for port in ports}
    rows.append({'at':time.time(),'processes':processes,'sockets':sockets})
    if index<a.seconds: time.sleep(1)
Path(a.output).write_text(json.dumps({'boundary':'unprivileged process CPU/RSS and local TCP staging counters; not USB bus utilization','samples':rows},indent=2)+'\n')
