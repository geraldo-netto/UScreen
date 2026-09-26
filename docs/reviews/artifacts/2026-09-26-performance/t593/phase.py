from pathlib import Path
import os, subprocess as s,json
root=Path('/tmp/blent-performance'); env=dict(os.environ,LD_LIBRARY_PATH='/tmp/blent-t585/runtime/squashfs-root/usr/lib/uscreen-ffmpeg')
for rate in [29,30]:
 out=root/f'phase-quiet-{rate}'
 cmd=['python3','scripts/benchmarks/gpu-capture.py','--serial','8002RH1010011900','--helper',str(root/'gpu-phase-probe'),'--ffmpeg','/tmp/blent-t585/runtime/test-bin/ffmpeg','--evdi-helper',str(Path('dist/blent-1.2.3/bin/evdi_helper').resolve()),'--scene','/tmp/blent-t585/t579-scene','--output-name','DVI-I-3-2','--same-gpu','/dev/dri/renderD129','--cross-gpu','/dev/dri/renderD128','--edid','/tmp/blent-next.edid','--output',str(out),'--card','2','--x','5120','--frames','180','--trials','3','--scene-fps',str(rate),'--variants','gpu-same','gpu-damage']
 s.run(cmd,env=env,check=True)
 # gpu-capture archives production sources; candidate binary uses these explicit development sources.
 import shutil
 shutil.copytree(root/'gpu-phase',out/'candidate-sources')
 s.run(['python3','scripts/benchmarks/summarize-gpu-capture.py',str(out),'--ffmpeg','/tmp/blent-t585/runtime/test-bin/ffmpeg'],env=env,check=True)
