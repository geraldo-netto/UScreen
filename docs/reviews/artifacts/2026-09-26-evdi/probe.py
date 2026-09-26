from pathlib import Path
import subprocess as s,time,json,datetime,collections
root=Path('/tmp/blent-investigation/t615')
def counts(log):
 return {name:log.count(text) for name,text in {'connector':'Connector state: connected','edid':'Edid property set','import_reject':"Don't allow imported",'ddcci':'Ignored ddc/ci','connected_by':'Connected with Task','double_connect':'Double connect','update_pending':'Update was already requested','grab_without_ready':'Update ready not sent'}.items()}
rows=[]
for label,args in [('idle',None),('query',['xrandr','--prop']),('current',['xrandr','--current','--prop'])]*3:
 since=datetime.datetime.now().strftime('%Y-%m-%d %H:%M:%S.%f')
 start=time.monotonic()
 if args:
  result=s.run(args,capture_output=True,text=True,check=True,timeout=5)
  (root/(label+'-outputs.txt')).write_text(result.stdout)
 else:time.sleep(.25)
 elapsed=time.monotonic()-start
 time.sleep(.1)
 result=s.run(['journalctl','-k','-b','--since',since,'--no-pager','-o','short-monotonic','-g','evdi'],text=True,capture_output=True,timeout=10)
 assert result.returncode in (0,1),result.stderr
 log=result.stdout
 rows.append({'mode':label,'wall_ms':elapsed*1000,'counts':counts(log)})
 (root/f'probe-{len(rows)}.log').write_text(log)
(root/'query-probes.json').write_text(json.dumps(rows,indent=2)+'\n')
print(json.dumps(rows,indent=2))
