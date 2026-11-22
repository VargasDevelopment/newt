#!/usr/bin/env python3
"""Future execution only: requires explicitly supplied, separately built transports.
Each fixture asserts authored expectations independently against BOTH executables.
No compiler, dependencies, source edits, or mutation execution here.
"""
import argparse
import subprocess
from protocol import validate

MAX = 18446744073709551615

def pattern(at, n, mode=0):
    return bytes(((i*17+i//256)%251 if mode == 0 else (i^(i>>8))&255 if mode == 1 else 0) for i in range(at,at+n)).hex()

def output(ok, data=''):
    return f'O {int(ok)} {data}'

def header(**kw):
    fields=dict(physical=10000,advertised=10000,capacity=4169,prefix=73,limit=4096,probe=0,fail_after=MAX,oversized=0,yield_action=0,yield_on=0,source_action=0,source_on=0,pattern=0,begin=100,end=9000)
    fields.update(kw)
    return ' '.join(map(str,fields.values()))

def fixtures():
    # Literal operation/event expectations, not a second implementation.
    yield 'cache-reread-and-boundary',header()+'\nR 100 4 1\nR 103 2 1\nR 99 1 1\nR 9000 0 0\n',[
        'S 10000','P 100 0','A 4169 4169 0',
        'S 10000','P 100 0','Y','S 10000','P 100 0','R 100 4096 4096','S 10000','P 100 0',output(1,pattern(100,4)),
        'S 10000','P 103 0','Y','S 10000','P 103 0',output(1,pattern(103,2)),output(0,'a5'),output(1),'D 73 4169 0 4196']
    for probe in [-1,1,17]:
        yield f'probe-fallback-{probe}',header(probe=probe,pattern=1)+'\nR 100 5 1\nR 100 2 1\n',[
            'S 10000',f'P 100 {probe}','A 73 73 0','R 100 5 5',output(1,pattern(100,5,1)), 'R 100 2 2',output(1,pattern(100,2,1)),'D 73 73 0 102']
    yield 'no-capacity-no-probe',header(capacity=4168)+'\nR 9000 0 0\nR 100 1 0\nR 100 2 1\n',[
        'S 10000','A 73 73 0',output(1),output(0,'a5'), 'R 100 2 2',output(1,pattern(100,2)),'D 73 73 0 102']
    for begin,end in [(9000,100),(0,10001)]:
        yield f'invalid-{begin}-{end}',header(begin=begin,end=end)+'\nR 100 1 1\n', ['S 10000','A 73 73 0',output(0,'a5'),'D 73 73 0 0']
    yield 'empty-range-caches-but-does-not-read',header(begin=10000,end=10000)+'\nR 10000 0 0\nR 10000 1 1\n',[
        'S 10000','P 10000 0','A 4169 4169 0',output(1),output(0,'a5'),'D 73 4169 0 0']
    yield 'short-refill-tail',header(begin=9993,end=10000,limit=3,pattern=1)+'\nR 9993 7 1\n',[
        'S 10000','P 9993 0','A 4169 4169 0','S 10000','P 9993 0','Y','S 10000','P 9993 0',
        'R 9993 7 3','R 9996 4 3','R 9999 1 1','S 10000','P 9993 0',output(1,pattern(9993,7,1)),'D 73 4169 0 10000']
    for action in [1,2,3]:
        # Post-yield validation. Size mismatch short-circuits the probe.
        checks=['S 10001'] if action==2 else ['S 10000',f'P 100 {(-1 if action==1 else 1)}']
        yield f'yield-invalidation-{action}',header(yield_action=action,yield_on=1)+'\nR 100 2 1\nR 100 0 0\n',[
            'S 10000','P 100 0','A 4169 4169 0','S 10000','P 100 0','Y',*checks,output(0,'a5a5'),output(0),'D 73 4169 0 0']
        yield f'refill-invalidation-{action}',header(source_action=action,source_on=1)+'\nR 100 2 1\nR 100 2 1\n',[
            'S 10000','P 100 0','A 4169 4169 0','S 10000','P 100 0','Y','S 10000','P 100 0',
            'R 100 4096 4096',*checks,output(0,'a5a5'),output(0,'a5a5'),'D 73 4169 0 4196']
    for got,opts in [(0,dict(fail_after=0)),(4097,dict(oversized=1))]:
        yield f'physical-bad-count-{got}',header(**opts)+'\nR 100 2 1\nR 100 2 1\n',[
            'S 10000','P 100 0','A 4169 4169 0','S 10000','P 100 0','Y','S 10000','P 100 0',
            f'R 100 4096 {got}',output(0,'a5a5'),output(0,'a5a5'),'D 73 4169 0 0']
    yield 'cross-chunk-partial-output-is-visible',header(fail_after=1,pattern=2)+'\nR 100 4100 1\n',[
        'S 10000','P 100 0','A 4169 4169 0','S 10000','P 100 0','Y','S 10000','P 100 0','R 100 4096 4096','S 10000','P 100 0',
        'S 10000','P 4196 0','Y','S 10000','P 4196 0','R 4196 4096 0',output(0,'00'*4096+'a5'*4),'D 73 4169 0 4196']
    yield 'direct-short-reads-partial-and-sticky',header(capacity=0,prefix=0,limit=3,fail_after=1)+'\nR 100 5 1\nR 100 1 1\n',[
        'S 10000','A 0 0 0','R 100 5 3','R 103 2 0',output(0,pattern(100,3)+'a5a5'),output(0,'a5'),'D 0 0 0 103']
    yield 'truncated-physical-cache',header(physical=102)+'\nR 100 1 1\n',[
        'S 10000','P 100 0','A 4169 4169 0','S 10000','P 100 0','Y','S 10000','P 100 0','R 100 4096 2','R 102 4094 0',output(0,'a5'),'D 73 4169 0 102']

    yield 'oom-diagnostic-survives-window-drop',header(prefix=100000)+'\nR 100 0 0\n',[
        'S 10000','P 100 0','A 4096 4096 100000',output(1),'D 0 4096 100000 0']
    yield 'successful-crossing-and-end',header(pattern=1)+'\nR 4190 2 1\nR 8284 6 1\nR 8998 2 1\n',[
        'S 10000','P 100 0','A 4169 4169 0',
        'S 10000','P 4190 0','Y','S 10000','P 4190 0','R 4190 4096 4096','S 10000','P 4190 0',output(1,pattern(4190,2,1)),
        'S 10000','P 8284 0','Y','S 10000','P 8284 0',
        'S 10000','P 8286 0','Y','S 10000','P 8286 0','R 8286 714 714','S 10000','P 8286 0',output(1,pattern(8284,6,1)),
        'S 10000','P 8998 0','Y','S 10000','P 8998 0',output(1,pattern(8998,2,1)),'D 73 4169 0 9000']
    yield 'nonsticky-invalid-requests',header()+'\nR 18446744073709551615 1 1\nR 8999 2 1\nR 100 1 0\nR 100 1 1\n',[
        'S 10000','P 100 0','A 4169 4169 0',output(0,'a5'),output(0,'a5a5'),output(0,'a5'),
        'S 10000','P 100 0','Y','S 10000','P 100 0','R 100 4096 4096','S 10000','P 100 0',output(1,pattern(100,1)),'D 73 4169 0 4196']
    # Independent fixed request sequence for 31-byte physical short reads.
    short=[f'R {at} {4196-at} 31' for at in range(100,4192,31)]+['R 4192 4 4']
    yield '31-byte-full-cache-refill',header(limit=31)+'\nR 100 3 1\n',[
        'S 10000','P 100 0','A 4169 4169 0','S 10000','P 100 0','Y','S 10000','P 100 0',
        *short,'S 10000','P 100 0',output(1,pattern(100,3)),'D 73 4169 0 4196']
    yield 'direct-requests-capped-at-4096',header(capacity=4095,prefix=0,pattern=2)+'\nR 100 4100 1\n',[
        'S 10000','A 0 0 0','R 100 4096 4096','R 4196 4 4',output(1,'00'*4100),'D 0 0 0 4200']

    yield 'zero-source',header(physical=0,advertised=0,begin=0,end=0)+'\nR 0 0 0\nR 1 0 0\n',[
        'S 0','P 0 0','A 4169 4169 0',output(1),output(0),'D 73 4169 0 0']
    yield 'negative-physical-after-short-read',header(begin=100,end=104,limit=3,source_action=1,source_on=1)+'\nR 100 2 1\nR 100 2 1\n',[
        'S 10000','P 100 0','A 4169 4169 0','S 10000','P 100 0','Y','S 10000','P 100 0',
        'R 100 4 3','R 103 1 -1',output(0,'a5a5'),output(0,'a5a5'),'D 73 4169 0 103']
    # Deliberate source-contract violation documents the absence of direct-mode
    # revalidation; it does not grant mutable-source support in production.
    yield 'direct-has-no-revalidation',header(capacity=4095,prefix=0,source_action=2,source_on=1)+'\nR 100 2 1\nR 102 2 1\n',[
        'S 10000','A 0 0 0','R 100 2 2',output(1,pattern(100,2)),
        'R 102 2 2',output(1,pattern(102,2)),'D 0 0 0 104']
    yield 'direct-oversized-sticky',header(capacity=4095,prefix=0,oversized=1)+'\nR 100 2 1\nR 100 2 1\n',[
        'S 10000','A 0 0 0','R 100 2 3',output(0,'a5a5'),output(0,'a5a5'),'D 0 0 0 0']

if __name__ == '__main__':
    parser=argparse.ArgumentParser(); parser.add_argument('--cpp',required=True);parser.add_argument('--rust',required=True);args=parser.parse_args()
    for name,stdin,expected in fixtures():
        for executable in [args.cpp,args.rust]:
            result=subprocess.run([executable],input=stdin,text=True,capture_output=True,check=True,timeout=10)
            actual=validate(stdin,result.stdout)
            if actual != expected:
                raise RuntimeError((name,executable,actual,expected,result.stderr))
    print('PASS authored independent traces for both transports')
