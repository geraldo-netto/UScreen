import concurrent.futures,json,os,sys,threading,time
sys.path.insert(0,os.getcwd()+'/scripts/tests')
import test_benchmark_visibility as suite
real_popen=suite.subprocess.Popen
local=threading.local()
def popen(command,**kwargs):
    child=real_popen(command,**kwargs)
    if command[0]=='Xvfb': local.children.append((child,kwargs.get('stderr')))
    return child
suite.subprocess.Popen=popen

def trial(number):
    local.children=[]
    fixture=suite.XVisibilityTests('test_t424_visible_window_and_focused_descendant')
    result={'trial':number}
    try:
        fixture.setUp()
        result['display']=fixture.display_name
        result['problem']=fixture.probe.problem()
        fixture.test_t424_visible_window_and_focused_descendant()
    except Exception as error:
        result['error']=str(error)
    finally:
        result['children']=[]
        for child,log in local.children:
            log.seek(0)
            result['children'].append({'pid':child.pid,'exit':child.poll(),'stderr':log.read(4000)})
        fixture.doCleanups()
    return result
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
    for result in pool.map(trial,range(24)):
        print(json.dumps(result),flush=True)
