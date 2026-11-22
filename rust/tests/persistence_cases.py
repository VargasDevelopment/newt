# SPDX-License-Identifier: MIT
# MIT; see LICENSE.
"""Future execution only: independent expectations, then differential equality.
Pass already-built --rust and --oracle executables; this script builds nothing.
--case names are also the actual protocol witnesses for mutations.json.
"""
import argparse
import hashlib
import struct
import subprocess
from dataclasses import dataclass


def u(value, size):
    return value.to_bytes(size, 'little')


def blob(value):
    return u(len(value), 2) + value


def checksum(b):
    h = 2166136261
    for v in b:
        h = ((h ^ v) * 16777619) & 0xffffffff
    return h


def seal(b, magic, generation=0, version=1):
    b = bytearray(b)
    b[:12] = struct.pack('<III', magic, version, generation)
    b[-4:] = u(checksum(b[:-4]), 4)
    return bytes(b)


D = bytes(range(1, 33))
BOOK = 'sb' + D.hex()[:13]
HEAD = 'sd' + D.hex()[:13]


def book(d=D, generation=0, unknown=False):
    b = bytearray(64)
    b[12:44] = d
    # C++ BookFacts default uses unknown start-permille, unset dates.
    b[52:54] = u(65535, 2)
    if unknown:
        b[44:50] = u(65535, 2) * 3
    return seal(b, 0x4254574e, generation)


def head(block=0, watermark=0, generation=0):
    b = bytearray(232)
    b[12:44] = D
    b[46:48] = u(block, 2)
    b[52:60] = u(watermark, 8)
    return seal(b, 0x4454574e, generation)


def archive(block, generation=0):
    b = bytearray(176)
    b[12:44] = D
    b[44:46] = u(block, 2)
    return seal(b, 0x4554574e, generation)


def stamped(b, generation):
    return seal(b, int.from_bytes(b[:4], 'little'), generation)


def records_text(records):
    return ';'.join(f'{n}:{a.hex()},{b.hex()}' for n, (a, b) in sorted(records.items()))


@dataclass
class Case:
    name: str
    request: bytes
    expected: str


def request(op=0, kind=2, *, cancel=0, opened=0, unavailable=0,
            fail_read=255, failures=0, fail_put_from=0, durable=0, repair=1, digest=D,
            parameter=0, payload=b'', records=None):
    records = records or {}
    result = bytes([op, kind, cancel, opened, unavailable, fail_read,
                    failures, durable, repair]) + u(fail_put_from, 2) + digest + u(parameter, 2) + blob(payload)
    result += u(len(records), 2)
    for n, (a, b) in sorted(records.items()):
        result += blob(n.encode()) + blob(a) + blob(b)
    return result


def cases():
    result = []

    def add(name, code, output=book(), trace=(), visible=None, durable_state=None, **kwargs):
        initial = kwargs.get('records', {})
        visible = initial if visible is None else visible
        durable_state = initial if durable_state is None else durable_state
        expected = f'{code}|{output.hex()}|{";".join(trace)}|{records_text(visible)}|{records_text(durable_state)}'
        result.append(Case(name, request(**kwargs), expected))

    read = [f'begin:{BOOK}:1', 'read:slot0', 'read:slot1', 'end']
    write = [f'begin:{BOOK}:0', 'read:slot0', 'read:slot1']
    valid = book(generation=7)
    add('absent', 1, trace=[f'begin:{BOOK}:1'])
    add('unavailable-missing', 4, opened=1, unavailable=1, trace=[f'begin:{BOOK}:1'])
    add('damaged-open', 2, opened=3, trace=[f'begin:{BOOK}:1'])
    add('failed-open', 4, opened=2, trace=[f'begin:{BOOK}:1'])
    add('empty-open', 2, records={BOOK: (b'', b'')}, trace=read)
    add('valid', 0, records={BOOK: (valid, b'')}, trace=read)
    add('read-failed-first', 4, fail_read=0, records={BOOK: (valid, b'')}, trace=read[:2]+['end'])
    add('read-failed-second', 4, fail_read=1, records={BOOK: (valid, b'')}, trace=read)
    add('size-unsupported', 3, records={BOOK: (b'x', valid)}, trace=read)
    add('version-unsupported', 3, records={BOOK: (seal(valid, 0x4254574e, 7, 2), valid)}, trace=read)
    add('corrupt', 2, records={BOOK: (bytes(64), b'')}, trace=read)
    add('torn-inactive', 0, records={BOOK: (valid, bytes(64))}, trace=read)
    conflict = bytearray(valid)
    conflict[44:46] = u(9497, 2)
    conflict = stamped(conflict, 7)
    add('equal-conflicting', 2, records={BOOK: (valid, conflict)}, trace=read)
    add('half-range', 2, records={BOOK: (valid, book(generation=7+0x80000000))}, trace=read)
    add('equal-identical', 0, records={BOOK: (valid, valid)}, trace=read)
    other = D[:7]+bytes([D[7]^1])+D[8:]
    add('digest-collision', 2, digest=other, output=book(other), records={BOOK:(valid,b'')}, trace=read)
    for label, failures, durable_reads, code in [('save',0,0,0),('retry',1,0,0),('unconfirmed',2,0,4),('durable-no-retry',1,1,4)]:
        new = book(generation=1)
        puts = 2 if failures and not durable_reads else 1
        after = {BOOK:(new,b'')}
        add(label,code,op=1,payload=book(),failures=failures,durable=durable_reads,output=b'',
            trace=write+[f'put:slot0:{new.hex()}']*puts+(['read:slot0'] if code==0 else [])+['end'],
            visible=after,durable_state=after if code==0 else {})
    for label, generation in [('always-write',7),('generation-wrap',0xffffffff)]:
        old = book(generation=generation)
        new = book(generation=(generation+1)&0xffffffff)
        add(label,0,op=1,payload=book(),records={BOOK:(old,b'')},output=b'',
            trace=write+[f'put:slot1:{new.hex()}','read:slot1','end'],visible={BOOK:(old,new)},durable_state={BOOK:(old,new)})
    add('save-corrupt',1,op=1,payload=book(),output=b'',records={BOOK:(bytes(64),b'')},trace=write+['end'])
    add('save-unsupported',2,op=1,payload=book(),output=b'',records={BOOK:(b'x',b'')},trace=write+['end'])
    add('cancel',255,op=1,cancel=1,payload=book(),output=b'')
    unknown = book(unknown=True)
    repair_bytes = stamped(unknown,1)
    add('repair-in-place',1,op=3,records={BOOK:(bytes(64),b'')},output=unknown,
        trace=read+[f'repair:{BOOK}:{repair_bytes.hex()}'],visible={BOOK:(repair_bytes,b'')},durable_state={BOOK:(repair_bytes,b'')})
    add('repair-failed',0,op=3,repair=0,records={BOOK:(bytes(64),b'')},output=unknown,trace=read+[f'repair:{BOOK}:{repair_bytes.hex()}'])
    new = book(generation=1)
    add('store-repair-failed',4,op=4,payload=book(),repair=0,output=b'',records={BOOK:(bytes(64),b'')},trace=write+['end',f'repair:{BOOK}:{new.hex()}'])
    add('set-aside',1,op=2,kind=4,output=b'',records={HEAD:(head(),b'')},trace=[f'aside:{HEAD}'],visible={},durable_state={})
    add('namespace-nibble',0,op=6,kind=0,output=BOOK.encode()+b'\0')
    add('namespace-month',0,op=6,kind=1,parameter=24322,output=b'sm202611'+bytes(8))
    add('namespace-archive',0,op=6,kind=3,parameter=0x1abc,output=('se'+D.hex()[:10]+'abc').encode()+b'\0')
    ledger=bytearray(180)
    ledger[88:90]=u(65535,2)
    ledger=seal(ledger,0x5354574e)
    add('ledger-load',0,kind=0,output=ledger,records={'newt_stats':(stamped(ledger,4),b'')},
        trace=['begin:newt_stats:1','read:slot0','read:slot1','end'])
    saved_ledger=stamped(ledger,1)
    add('ledger-save',0,op=1,kind=0,payload=ledger,output=b'',
        trace=['begin:newt_stats:0','read:slot0','read:slot1',f'put:slot0:{saved_ledger.hex()}','read:slot0','end'],
        visible={'newt_stats':(saved_ledger,b'')},durable_state={'newt_stats':(saved_ledger,b'')})
    # Heterogeneous schema probes retain runtime calls for each store surface.
    for label, kind, name, encoded, parameter in [
        ('month',1,'sm202601',seal(bytearray(12)+u(24312,2)+bytes(90),0x4d54574e),24312),
        ('streak',3,'newt_stats_sk',seal(bytes(28),0x4b54574e),0),
        ('head',4,HEAD,head(),0),
        ('archive',5,'se'+D.hex()[:10]+'099',archive(153),153),
    ]:
        add(label+'-load',0,kind=kind,parameter=parameter,records={name:(stamped(encoded,4),b'')},output=encoded,
            trace=[f'begin:{name}:1','read:slot0','read:slot1','end'])
        saved=stamped(encoded,1)
        add(label+'-save',0,op=1,kind=kind,parameter=parameter,payload=encoded,output=b'',
            trace=[f'begin:{name}:0','read:slot0','read:slot1',f'put:slot0:{saved.hex()}','read:slot0','end'],
            visible={name:(saved,b'')},durable_state={name:(saved,b'')})
    add('named-book',0,kind=6,payload=BOOK.encode(),records={BOOK:(valid,b'')},trace=read)
    global_copy=bytearray(180)
    global_copy[12]=2  # Unrecoverable is a readable frozen copy, no imported history.
    global_copy=seal(global_copy,0x4743574e,4)
    add('crossink-frozen-unrecoverable',0,kind=7,output=bytes(97),records={'newt_stats_ci':(global_copy,b'')},
        trace=['begin:newt_stats_ci:1','read:slot0','read:slot1','end'])
    add('crossink-unreadable',0,kind=7,output=b'\x01'+bytes(96),records={'newt_stats_ci':(b'x',b'')},
        trace=['begin:newt_stats_ci:1','read:slot0','read:slot1','end'])
    source_name=b'book_123'
    source_digest=hashlib.sha256(source_name).digest()
    source_namespace=b'ci'+source_digest.hex()[:13].encode()
    add('source-namespace',0,op=7,kind=0,digest=source_digest,payload=source_name,output=source_namespace+b'\0')
    source=bytearray(160)
    source[12:12+len(source_name)]=source_name
    source[44]=2
    source=seal(source,0x4243574e)
    add('named-source',1,op=7,kind=1,digest=source_digest,parameter=15,payload=source_namespace+source,output=b'')
    add('named-source-mismatch',0,op=7,kind=1,digest=source_digest,parameter=15,payload=b'ci0000000000000'+source,output=b'')
    # Fold into a missing head: independently derive the dated bucket and watermark.
    day = 9497
    block = day//62
    folded = bytearray(head(block=block,watermark=2000))
    folded[44:46] = u(day,2)
    folded[72:76] = u(2,4)  # midnight is the night period
    index = day % 62
    folded[104+2*index:106+2*index] = u(2,2)
    folded = stamped(folded,1)
    fold = u(0,8)+u(0,4)*2+u(2000,8)+u(0,4)*2+u(day,2)+b'\0'
    add('fold-first',0,op=5,payload=fold,output=b'',trace=[f'begin:{HEAD}:1',f'begin:{HEAD}:0','read:slot0','read:slot1',f'put:slot0:{folded.hex()}','read:slot0','end'],visible={HEAD:(folded,b'')},durable_state={HEAD:(folded,b'')})
    add('fold-unsupported',0,op=5,payload=fold,output=b'',records={HEAD:(b'x',b'')},trace=[f'begin:{HEAD}:1','read:slot0','read:slot1','end'])
    # Head switch must confirm the old block before attempting any new-head write.
    old_block = block
    old_head = bytearray(head(block=old_block,watermark=0))
    old_head[44:46] = u(day,2)
    old_head = stamped(old_head,8)
    switch_fold = fold[:-3]+u(day+62,2)+b'\0'
    archive_name = 'se'+D.hex()[:10]+f'{old_block:03x}'
    target_name = 'se'+D.hex()[:10]+f'{block+1:03x}'
    archived = archive(old_block,1)
    first = [f'begin:{HEAD}:1','read:slot0','read:slot1','end',
             f'begin:{archive_name}:0','read:slot0','read:slot1']
    initial = {HEAD:(old_head,b'')}
    partial = {**initial,archive_name:(archived,b'')}
    add('fold-archive-unconfirmed',0,op=5,payload=switch_fold,failures=2,output=b'',records=initial,
        trace=first+[f'put:slot0:{archived.hex()}']*2+['end'],visible=partial)
    switched = bytearray(folded)
    switched[46:48] = u(block+1,2)
    switched = stamped(switched,9)
    final = {**partial,HEAD:(old_head,switched)}
    add('fold-switch',0,op=5,payload=switch_fold,output=b'',records=initial,
        trace=first+[f'put:slot0:{archived.hex()}','read:slot0','end',f'begin:{target_name}:1',
                     f'begin:{HEAD}:0','read:slot0','read:slot1',f'put:slot1:{switched.hex()}','read:slot1','end'],
        visible=final,durable_state=final)
    # A stale head lost five seconds already credited to NWTS: only one new
    # second belongs on this later day, while the complete watermark advances.
    stale = head(watermark=1000, generation=7)
    advanced = u(5000,8)+u(0,4)*2+u(6500,8)+u(0,4)*2+u(day+1,2)+b'\0'
    credited = bytearray(head(block=block,watermark=6000))
    credited[44:46] = u(day+1,2)
    credited[72:76] = u(1,4)
    credited[104+2*((day+1)%62):106+2*((day+1)%62)] = u(1,2)
    credited = stamped(credited,8)
    credited_records = {HEAD:(stale,credited)}
    add('fold-stale-head-advanced-ledger',0,op=5,payload=advanced,output=b'',records={HEAD:(stale,b'')},
        trace=[f'begin:{HEAD}:1','read:slot0','read:slot1','end',f'begin:{HEAD}:0','read:slot0','read:slot1',f'put:slot1:{credited.hex()}','read:slot1','end'],
        visible=credited_records,durable_state=credited_records)
    # The archive sync succeeds; both later head writes lose acknowledgement.
    # Cold authority keeps the old head and the successfully committed archive.
    add('fold-archive-saved-head-unconfirmed',0,op=5,payload=switch_fold,fail_put_from=2,output=b'',records=initial,
        trace=first+[f'put:slot0:{archived.hex()}','read:slot0','end',f'begin:{target_name}:1',
                     f'begin:{HEAD}:0','read:slot0','read:slot1',f'put:slot1:{switched.hex()}',f'put:slot1:{switched.hex()}','end'],
        visible=final,durable_state=partial)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rust', required=True)
    parser.add_argument('--oracle', required=True)
    parser.add_argument('--case', action='append')
    args = parser.parse_args()
    selected = [c for c in cases() if not args.case or c.name in args.case]
    if not selected or args.case and set(args.case)-{c.name for c in selected}:
        raise ValueError('unknown or empty witness selection')
    text = ''.join(c.request.hex()+'\n' for c in selected)
    outputs = []
    for binary in (args.oracle,args.rust):
        lines = subprocess.run([binary],input=text,text=True,capture_output=True,check=True).stdout.splitlines()
        if len(lines) != len(selected):
            raise AssertionError('response count')
        for case, line in zip(selected,lines):
            if line != case.expected:
                raise AssertionError(f'{binary} {case.name}\nexpected {case.expected}\nactual   {line}')
        outputs.append(lines)
    assert outputs[0] == outputs[1]


if __name__ == '__main__':
    main()


def corpus():
    return ['stats_persist ' + case.request.hex() for case in cases()]

def assert_expectations(inputs, replies):
    expected = {'stats_persist ' + case.request.hex(): (case.name, case.expected) for case in cases()}
    for command, reply in zip(inputs, replies):
        if command in expected:
            name, wanted = expected[command]
            if reply != wanted:
                raise AssertionError(f'Persistence independent expectation {name}: {reply} != {wanted}')
