"""Extract completed USB input journeys; no-change inputs are never page turns."""
from pathlib import Path
import argparse,hashlib,json,math,re,statistics,sys
PROFILES={
'library-entry':['home_focus']*2+['library_open'],
'warm-row0':['home_focus']*2+['library_open','book_open']+['page_next']*8+['page_previous']*8+['save_and_home'],
'balanced-row0':['home_focus']*2+['library_open','book_open']+(['page_next']*4+['page_previous']*4)*2+['save_and_home'],
'cohort-row1':['home_focus']*2+['library_open','library_focus','book_open']+['page_previous','page_next']*4+['save_and_home'],
'cohort-row2':['home_focus']*2+['library_open','library_focus','library_focus','book_open']+['page_previous','page_next']*4+['save_and_home'],
}

def require(condition,message):
    if not condition:raise ValueError(message)

def extract(report,roles):
    require(report['connection']['closed'] and 'error' not in report,'unclean or failed connection')
    records=report['protocol']['records'];requests=[r for r in records if r['direction']=='request' and r['body'].startswith('PRESS ')]
    require(len(requests)==len(roles),'press/profile arity mismatch')
    result=[]
    for request,role in zip(requests,roles):
        replies=[r for r in records if r['direction']=='response' and (r['boot'],r['session'],r['request_id'])==(request['boot'],request['session'],request['request_id'])]
        require(len(replies)==3 and [r['body'] for r in replies[:2]]==['ACCEPTED','CONSUMED'],'incomplete or unordered acknowledgements')
        end=replies[-1];require(re.fullmatch(r'(RENDER_RETURN|NO_VISUAL_CHANGE) gen=(0|[1-9][0-9]*)',end['body']) is not None,'invalid terminal acknowledgement')
        times=[request['monotonic'],*[r['monotonic'] for r in replies]];require(all(type(t) in (float,int) and math.isfinite(t) and t>=0 for t in times) and times==sorted(times),'invalid or reversed timestamps')
        visual=end['body'].startswith('RENDER_RETURN ')
        result.append({'role':role,'action':request['body'].removeprefix('PRESS '),'visual_completed':visual,
                       'request_to_completion_ms':1000*(end['monotonic']-request['monotonic']),
                       'consumed_to_completion_ms':1000*(end['monotonic']-replies[1]['monotonic']),
                       'completion':end['body'],'request_id':request['request_id']})
    return result

def percentile(values,q):
    values=sorted(values);index=(len(values)-1)*q;low=math.floor(index);high=math.ceil(index)
    return values[low]+(values[high]-values[low])*(index-low)

def analyze(root):
    samples=[];inputs={};runtime=[];boots=[]
    for name,roles in PROFILES.items():
        p=root/(name+'.json');raw=p.read_bytes();report=json.loads(raw);inputs[name]=hashlib.sha256(raw).hexdigest()
        require('build=2f37398a0fea ' in report['hello'],'wrong frozen C++ build')
        require(('screen=0 ' if name!='library-entry' else 'screen=6 ') in report['status'],'unexpected final screen')
        boots.append(report['protocol']['boot'])
        for sample in extract(report,roles):samples.append({'profile':name,**sample})
        runtime.extend(row for row in report['firmware_rows'] if row['row']=='runtime')
    require(len(set(boots))==len(boots),'fresh-boot cohort assumption not met')
    summaries={}
    for role in sorted({s['role'] for s in samples if s['role']!='home_focus'}):
        values=[s['request_to_completion_ms'] for s in samples if s['role']==role and s['visual_completed']]
        skipped=sum(s['role']==role and not s['visual_completed'] for s in samples)
        summaries[role]={'completed_samples':len(values),'no_change_excluded':skipped,'median_ms':statistics.median(values) if values else None,'p95_ms':percentile(values,.95) if values else None,'minimum_ms':min(values) if values else None,'maximum_ms':max(values) if values else None}
    reading=[r for r in runtime if r['kind']=='reader']
    evidence={'cpp_source':'2f37398a0fea6cfc511535592e6112263582ff9a','build':'2f37398a0fea','power':'USB','orientation':'landscape','profiles':list(PROFILES),'input_sha256':inputs,'measurements':samples,'summary':summaries,'minimum_observed_runtime_heap_free_bytes':min(r['heap_min_free'] for r in reading),'minimum_observed_loop_stack_free_bytes':min(r['stack_min_free'] for r in reading),'loop_stack_bytes':16384,'cold_cache_measured':False,'purge_cache_count':0,'firmware_switch_count':0,'private_content_or_frame_capture_count':0,'first_ever_book_load_measured':False,'hardware_sleep_wake_or_battery_verified':False,'rust_device_comparison_available':False,'performance_goal_met':False,'notes':['Three distinct nonprivate Library rows; structural diversity unverified.','Host request-to-terminal RENDER_RETURN includes USB/queue/application/storage/render/blocking-display return, not frame-download time.','NO_VISUAL_CHANGE excluded from page-turn counts; initial home-focus clamps are not latency samples.','Existing card caches retained, no claim of cold first-ever loading.','Controller terminal paint return is an electronic completion proxy, not optical quality evidence.','Baseline only: no paired Rust device result, no>=10% claim.']}
    return evidence


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('input',type=Path)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    require(not sys.flags.optimize,'Python optimization is unsupported')
    evidence=analyze(args.input)
    evidence['analyzer_sha256']=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    with args.output.open('x') as stream:json.dump(evidence,stream,indent=2);stream.write('\n')
    print('PASS C++ baseline analysis; no Rust performance claim: '+str(args.output))

if __name__=='__main__':main()
