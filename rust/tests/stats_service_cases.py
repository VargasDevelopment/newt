"""Independent SS1 inputs/witnesses. Wire layouts from PINNED original C++.
No Rust-produced values used as expectations. Execution remains pending.
"""
import hashlib
import struct
import zlib
from dataclasses import dataclass
A = '11' * 32
B = '22' * 32

def run(op, digest=A, ms=0, pages=0, sessions=0, day=9800, hour=12,
        end=0, permille=65535, cancel=0, tail=''):
    return f'SS1 run {op} {digest} {ms} {pages} {sessions} {day} {hour} {end} {permille} {cancel} {tail}'.strip()

def seal(b):
    value = 2166136261
    for c in b[:-4]:
        value = ((value ^ c) * 16777619) & 0xffffffff
    struct.pack_into('<I', b, len(b)-4, value)
    return b.hex()

def record(size, magic, version=1):
    b = bytearray(size)
    struct.pack_into('<III', b, 0, magic, version, 1)
    return b

def progress(digest, ms, pages, sessions):
    d = bytes.fromhex(digest)
    b = bytearray(52)
    b[:4] = b'NWTR'
    struct.pack_into('<IIQIIQII', b, 4, 1, 1, int.from_bytes(d[:8], 'little'), 0, 0, ms, pages, sessions)
    struct.pack_into('<I', b, 48, zlib.crc32(b[:48]))
    ns = 'nb' + digest[:13]
    return [f'SS1 put {ns} digest {digest}', f'SS1 put {ns} slot0 {b.hex()}']

def cross(digest=A, binding=1, completed=1, wrong_namespace=False):
    g = record(180, 0x4743574e)
    g[12:15] = bytes([1, 1, 17])
    g[16] = 2  # CrossInk v2
    struct.pack_into('<I', g, 29, 1)  # imported finished = 1
    s = record(160, 0x4243574e)
    name = b'book_17'
    s[12:12+len(name)] = name
    s[44:48] = bytes([1, 1, 12, binding])
    s[48] = 2
    s[59] = completed
    if binding:
        s[124:156] = bytes.fromhex(digest)
    sha = hashlib.sha256(name).hexdigest()
    ns = 'ci' + ('0'*13 if wrong_namespace else sha[:13])
    return [f'SS1 sha {name.hex()} {sha}', f'SS1 put newt_stats_ci slot0 {seal(g)}', f'SS1 put {ns} slot0 {seal(s)}']

@dataclass
class Case:
    name: str
    commands: list
    reports: list
    contains: tuple = ()
    excludes: tuple = ()

def base_cases():
    seed = run('seed')
    yield Case('empty-seed', [seed], ['seed done 0 0 0'])
    yield Case('seed-repeat-no-double', [seed, seed], ['seed done 0 0 0', 'seed done 0 0 0'])
    yield Case('full-namespace-seed', progress(A, 9000, 5, 2)+progress(B, 3000, 7, 1)+[seed], ['seed done 0 12 12'], ('list:nb','list:sb'))
    yield Case('seed-cancel', [run('seed', cancel=1)], ['seed cancelled 0 0 0'], (), ('put:', 'aside:', 'repair:'))
    yield Case('seed-scan-cancel', progress(A, 1000, 1, 1)+[run('seed', cancel=1)], ['seed cancelled 0 0 0'], (), ('put:slot',))
    yield Case('seed-end-failed-begin', ['SS1 put nb1111111111111 digest '+A, 'SS1 open nb1111111111111 2', seed], ['seed done 0 0 0'], ('begin:nb1111111111111:1|end',))
    yield Case('unavailable', ['SS1 fault 1 0 0 0 0 0', seed], ['seed unavailable 0 0 0'])
    yield Case('listing-failure', ['SS1 fault 0 0 0 0 0 1', seed], ['seed unavailable 0 0 0'], (), ('put:',))
    yield Case('lostack-reconciled', ['SS1 fault 0 0 0 1 0 0', seed], ['seed done 0 0 0'])
    yield Case('lostack-unconfirmed', ['SS1 fault 0 0 1 0 0 0', seed], ['seed unconfirmed 0 0 0'])
    yield Case('fold-idempotent', [seed, run('fold',ms=60000,pages=3,sessions=1),run('fold',ms=60000,pages=3,sessions=1)], ['seed done 0 0 0','fold done 0 60 3','fold done 0 0 0'])
    yield Case('same-book-lease', [seed,run('fold',ms=1000,pages=2)], ['seed done 0 0 0','fold done 0 1 2'], (), ('aside:', 'repair:'))
    yield Case('handover-empty-final', [seed,run('fold',digest=B,ms=2000,pages=1)], ['seed done 0 0 0','handover done 0 0 0'])
    yield Case('handover-final-delta', [seed]+progress(A,7000,3,1)+[run('fold',digest=B,ms=9000,pages=2)], ['seed done 0 0 0','handover done 0 7 3'])
    yield Case('mark-finish-undo', [seed,run('mark',tail='1'),run('mark',tail='0')], ['seed done 0 0 0','mark done 1 0 0','mark done 0 0 0'])
    yield Case('mark-other-waits', [seed,run('mark',digest=B,tail='1')], ['seed done 0 0 0','mark waiting 0 0 0'])
    yield Case('fold-cancel-preeffects', [seed,'SS1 clear',run('fold',ms=1000,cancel=1)], ['seed done 0 0 0','fold cancelled 0 0 0'], (), ('put:',))
    yield Case('fold-unconfirmed-no-credit', [seed,'SS1 fault 0 0 1 0 0 0',run('fold',ms=7000,pages=3)], ['seed done 0 0 0','fold unconfirmed 0 0 0'])
    yield Case('midnight-split', [seed,run('marked',ms=9000,pages=4,sessions=2,day=9801,tail='9800 2 6000 2 1')], ['seed done 0 0 0','fold done 0 9 4'])
    yield Case('midnight-stale-boundary', [seed,run('fold',ms=8000,pages=3,sessions=2),run('marked',ms=9000,pages=4,sessions=2,day=9801,tail='9800 2 6000 2 2')], ['seed done 0 0 0','fold done 0 8 3','fold done 0 1 1'])
    for tag,opts,finished in [('bound',{},1),('other',{'digest':B},2),('unbound',{'binding':0},2),('incomplete',{'completed':0},2),('wrong-name',{'wrong_namespace':True},2)]:
        yield Case('cross-'+tag,[seed]+cross(**opts)+[run('mark',tail='1'),run('summary')], ['seed done 0 0 0','mark done 1 0 0','fold done 0 0 0'], (f'S 0 0 0 {finished} ',))
    for tag,raw,result in [('corrupt',record(180,0x5354574e).hex(),'done'),('short','00','unsupported'),('newer',seal(record(180,0x5354574e,2)),'unsupported')]:
        yield Case('ledger-'+tag,[f'SS1 put newt_stats slot0 {raw}',seed], [f'seed {result} 0 0 0'])
    # Newer month permits an UNDated fold; newer ledger stops the service.
    future = seal(record(104,0x4d54574e,2))
    yield Case('newer-target-undated',[seed, f'SS1 put sm202610 slot0 {future}',run('fold',ms=5000,pages=2,day=9800)], ['seed done 0 0 0','fold done 0 5 2'])
    yield Case('handover-partial-facts-commit',[seed,'SS1 fault 0 0 2 0 0 0',run('fold',digest=B)], ['seed done 0 0 0','handover unconfirmed 0 0 0'])
    yield Case('midnight-partial-retry', [seed,'SS1 fault 0 0 2 0 0 0',run('marked',ms=9000,pages=4,sessions=2,day=9801,tail='9800 2 6000 2 1'),'SS1 fault 0 0 0 0 0 0',run('marked',ms=9000,pages=4,sessions=2,day=9801,tail='9800 2 6000 2 1')], ['seed done 0 0 0','fold unconfirmed 0 6 2','fold done 0 3 2'])
    # No second NWTS was written: the month archive failed first. Retry credits
    # the remaining3seconds/2pages; a separate nsfault witness targets NWTS.
    newer_book=seal(record(64,0x4254574e,2))
    yield Case('handover-newer-facts', [seed,f'SS1 put sb{B[:13]} slot0 {newer_book}',run('fold',digest=B)], ['seed done 0 0 0','handover waiting 0 0 0'])
    yield Case('cancel-only-before-effects', [seed,run('fold',ms=1000,cancel=2)], ['seed done 0 0 0','fold done 0 1 0'])
    yield Case('archive-before-head',[seed,run('fold',ms=1000,day=9800),'SS1 clear',run('fold',ms=3000,day=9801)], ['seed done 0 0 0','fold done 0 1 0','fold done 0 2 0'], ('begin:sm202610:0', 'begin:sm202611:1', 'begin:newt_stats:0'))
    yield Case('archive-unconfirmed-stops-head',[seed,run('fold',ms=1000,day=9800),'SS1 clear','SS1 fault 0 0 1 0 0 0',run('fold',ms=3000,day=9801)], ['seed done 0 0 0','fold done 0 1 0','fold unconfirmed 0 0 0'], (), ('begin:newt_stats:0',))
    yield Case('cross-unavailable-source',[seed]+cross()+['SS1 open ci'+hashlib.sha256(b'book_17').hexdigest()[:13]+' 2',run('mark',tail='1')], ['seed done 0 0 0','mark unavailable 0 0 0'])
    yield Case('cross-finish-undo-remark',[seed]+cross()+[run('mark',tail='1'),run('mark',tail='0'),run('mark',tail='1'),run('summary')], ['seed done 0 0 0','mark done 1 0 0','mark done 0 0 0','mark done 1 0 0','fold done 0 0 0'], ('S 0 0 0 1 ',))
    yield Case('seed-repeat-preserves-lease',[seed,run('fold',ms=6000,pages=2),seed,run('fold',ms=7000,pages=3),run('summary')], ['seed done 0 0 0','fold done 0 6 2','seed done 0 0 0','fold done 0 1 1','fold done 0 0 0'], ('S 7 3 ',))

# Additional boundary witnesses address the first independent review.

def sleep_request(*, day=9800, unsaved=0, habit=1, week=1,
                  progress_flag=1, initial_habit=4, initial_week=1,
                  initial_days=165, initial_streak=4321):
    tail=f'{unsaved} {habit} {week} {progress_flag} {initial_habit} {initial_week} {initial_days} {initial_streak}'
    return run('sleep',day=day,tail=tail)

def cases():
    yield from base_cases()
    seed=run('seed');done='fold done 0 0 0'
    yield Case('midnight-second-head-unconfirmed',[seed,'SS1 nsfault newt_stats 2',run('marked',ms=9000,pages=4,sessions=2,day=9801,tail='9800 2 6000 2 1')],['seed done 0 0 0','fold unconfirmed 0 6 2'],('begin:sm202611:1',))
    yield Case('midnight-second-head-readable-retry',[seed,'SS1 nsfault newt_stats 2',run('marked',ms=9000,pages=4,sessions=2,day=9801,tail='9800 2 6000 2 1'),'SS1 nsfault - 0',run('marked',ms=9000,pages=4,sessions=2,day=9801,tail='9800 2 6000 2 1')],['seed done 0 0 0','fold unconfirmed 0 6 2',done])
    yield Case('midnight-armed',[seed,run('marked',ms=9000,pages=4,sessions=2,day=9801,tail='9800 1 6000 2 1')],['seed done 0 0 0','fold done 0 9 4'])
    yield Case('midnight-session-boundary-rollback',[seed,run('fold',ms=8000,pages=3,sessions=2),run('marked',ms=9000,pages=4,sessions=3,day=9801,tail='9800 2 8500 3 1')],['seed done 0 0 0','fold done 0 8 3','fold done 0 1 1'])
    yield Case('seed-open-unreadable',progress(A,1000,1,1)+[f'SS1 put nb{A[:13]} slot0 00',seed],['seed unavailable 0 0 0'],(),('put:',))
    yield Case('seed-nonopen-undercount',progress(B,1000,1,1)+[f'SS1 put nb{B[:13]} slot0 00',seed],['seed done 0 0 0'])
    yield Case('mark-unconfirmed-finish',[seed,'SS1 fault 0 0 1 0 0 0',run('mark',tail='1')],['seed done 0 0 0','mark unconfirmed 0 0 0'])
    collision=A[:-2]+'99'
    yield Case('cross-same-prefix-full-digest',[seed]+cross(digest=collision)+[run('mark',tail='1'),run('summary')],['seed done 0 0 0','mark done 1 0 0',done],('S 0 0 0 2 ',))
    bad='ci'+'f'*13
    yield Case('cross-match-then-unreadable',[seed]+cross()+[f'SS1 put {bad} slot0 00',f'SS1 open {bad} 2','SS1 clear',run('mark',tail='1')],['seed done 0 0 0','mark unavailable 0 0 0'],('begin:'+bad+':1',),('put:',))
    yield Case('facts-leased',[seed,run('mark',tail='1'),run('facts')],['seed done 0 0 0','mark done 1 0 0',done],('B 0 0 0 9800 65535 1\n',))
    yield Case('facts-stored',[seed,run('mark',tail='1'),run('fold',digest=B),run('facts')],['seed done 0 0 0','mark done 1 0 0','handover done 0 0 0',done],('B 0 0 0 9800 65535 1\n',))
    corrupt_book=record(64,0x4254574e).hex()
    yield Case('facts-corrupt-stored',[run('seed',digest=B),f'SS1 put sb{A[:13]} slot0 {corrupt_book}',run('facts')],['seed done 0 0 0',done],('B 65535 65535 65535 0 65535 0\n',))
    yield Case('sleep-untrusted-preserves',[sleep_request(day=0)],[done],('E 1 4 1 165 4321\n','U 0 - -\n'),('begin:','read:','put:'))
    yield Case('sleep-unseeded-preserves',[sleep_request()],[done],('E 1 4 1 165 4321\n','U 0 - -\n'),('begin:newt_stats_sk:',))
    yield Case('sleep-disabled-preserves',[seed,'SS1 clear',sleep_request(habit=0,week=0)],['seed done 0 0 0',done],('E 1 4 1 165 4321\n','U 1 4 0\n'),('begin:newt_stats_sk:',))
    yield Case('sleep-week-only',[seed,run('fold',ms=60000),'SS1 clear',sleep_request(habit=0,initial_habit=3,initial_week=0)],['seed done 0 0 0','fold done 0 60 0',done],('E 1 3 1 1 0\n','U 1 4 60\n'))
    yield Case('sleep-habit-only',[seed,run('fold',ms=1800000),'SS1 clear',sleep_request(week=0)],['seed done 0 0 0','fold done 0 1800 0',done],('E 1 2 1 165 4321\n','U 1 4 1800\n'),('begin:newt_stats_sk:',))
    dated=[seed,run('fold',ms=1000,day=9800),run('fold',ms=1801000,day=9801)]
    yield Case('sleep-archived-today-preserves',dated+['SS1 clear',sleep_request(day=9800)],['seed done 0 0 0','fold done 0 1 0','fold done 0 1800 0',done],('E 1 4 1 165 4321\n','U 0 - -\n'),('begin:sm','begin:newt_stats_sk:'))
    for tag,status,habit in [('absent',1,2),('unreadable',2,0)]:
        yield Case('sleep-archive-'+tag,dated+[f'SS1 open sm202610 {status}','SS1 clear',sleep_request(day=9801,week=0,initial_week=0)],['seed done 0 0 0','fold done 0 1 0','fold done 0 1800 0',done],(f'E 1 {habit} 0 165 4321\n','U 1 4 1801\n'),('begin:newt_stats_sk:',))
    yield Case('refresh-unchanged-no-write',[seed,run('fold',ms=60000),run('refresh'),'SS1 clear',run('refresh')],['seed done 0 0 0','fold done 0 60 0',done,done],(),('put:','aside:'))
    corrupt_streak=record(28,0x4b54574e).hex()
    yield Case('refresh-corrupt-cache',[seed,run('fold',ms=60000),f'SS1 put newt_stats_sk slot0 {corrupt_streak}','SS1 clear',run('refresh')],['seed done 0 0 0','fold done 0 60 0',done],('aside:newt_stats_sk',))
    yield Case('refresh-unreadable-history',dated+['SS1 open sm202610 2','SS1 clear',run('refresh',day=9801)],['seed done 0 0 0','fold done 0 1 0','fold done 0 1800 0','fold unavailable 0 0 0'],(),('put:','aside:'))


def assert_output(case,result):
    from stats_service_validation import validate
    validate(result, case.commands + ['SS1 dump'])
    reports = [' '.join(row.split()[1:6]) for row in result.splitlines() if row.startswith('R ')]
    assert reports == case.reports, (case.name, reports, case.reports)
    for witness in case.contains:
        assert witness in result, (case.name, 'missing', witness)
    trace = next(row for row in result.splitlines() if row.startswith('T '))
    for witness in case.excludes:
        assert witness not in trace, (case.name, 'forbidden', witness)
    if case.name == 'archive-before-head':
        assert trace.index('begin:sm202610:0') < trace.index('begin:sm202611:1') < trace.index('begin:newt_stats:0')
    if case.name in ('seed-cancel','seed-scan-cancel','fold-cancel-preeffects','cancel-only-before-effects'):
        last = [line for line in result.splitlines() if line.startswith('R ')][-1]
        assert last.split()[-1]=='1',(case.name,'cancellation callback count')
    if case.name in ('mark-unconfirmed-finish','midnight-second-head-unconfirmed'):
        import struct
        spaces={tag:[] for tag in ('V','D')}
        for line in result.splitlines():
            words=line.split()
            if len(words)==4 and words[0] in spaces and words[1]=='newt_stats':
                spaces[words[0]].append(bytes.fromhex(words[3]))
        visible=max(spaces['V'],key=lambda b:struct.unpack_from('<I',b,8)[0])
        durable=max(spaces['D'],key=lambda b:struct.unpack_from('<I',b,8)[0])
        if case.name=='mark-unconfirmed-finish':
            assert struct.unpack_from('<H',visible,90)[0]&1
            assert struct.unpack_from('<H',visible,28)[0]==1
            assert not (struct.unpack_from('<H',durable,90)[0]&1)
        else:
            assert struct.unpack_from('<Q',visible,64)[0]==9000
            assert struct.unpack_from('<Q',durable,64)[0]==6000
    if case.name=='cross-match-then-unreadable':
        import hashlib
        assert trace.index('begin:ci'+hashlib.sha256(b'book_17').hexdigest()[:13]+':1') < trace.index('begin:ci'+'f'*13+':1')
    return result

def wrapped(case):
    return 'stats_service '+ ('\n'.join(case.commands+['SS1 dump'])+'\n').encode().hex()

def corpus():
    return [wrapped(case) for case in cases()]

def assert_expectations(commands,replies):
    from stats_service_validation import validate_wire
    lookup={wrapped(case):case for case in cases()}
    for command,reply in zip(commands,replies):
        if command.startswith('stats_service '):assert_output(lookup[command],validate_wire(reply,command))
