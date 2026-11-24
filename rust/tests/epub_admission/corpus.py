"""Independent byte cases extending the sealed candidate vectors."""
import struct
STATUS = dict(zip(('Ok','IoError','NotZip','NotEpub','Encrypted','Truncated','Unsupported','OutOfMemory','ParseError','NotFound','Stale','MetadataTooLarge','PathTooLong'), range(13)))

def cases(v):
    result = []
    def add(name, data, expected, **kw):
        result.append(dict(id=name, data=data, status=expected, **kw))
    for row in v.CASES:
        add(row['id'],v.archive(row['members']),STATUS[row['status']], **{k:value for k,value in row.items() if k not in ('id','members','status')})
    base = v.archive(v.BASE)
    add('short-read-seven',base,0,max_read=7)
    add('empty',b'',2)
    add('eocd-cut',base[:-1],2)
    add('metadata',base,0,op='O',metadata=['4578616d706c65','-','-','-'])
    add('chapter-stored',base,0,op='C',member=2,finish=1,payload=b'<html/>')
    add('chapter-short-read',base,0,op='C',member=2,max_read=1,finish=1,payload=b'<html/>')
    for name,payload,declared,finished in [('extra',b'\x73\x04\x00',0,0),('empty',b'\x03\x00',0,1),('short',b'\x03\x00',1,0)]:
        add('chapter-deflate-'+name,v.archive([('c',payload,8,declared,0)]),0,op='C',finish=finished,payload=b'')
    add('chapter-deflate-payload',v.archive([('c',b'\x73\x04\x00',8,1,0)]),0,op='C',finish=1,payload=b'A')
    add('chapter-binary-payload',v.archive([('c',b'\x00\xffx\r\n',0,5,0)]),0,op='C',finish=1,payload=b'\x00\xffx\r\n')
    data = bytearray(v.archive([('x',b'',0,0,1)]))
    struct.pack_into('<H',data,6,0)
    add('local_mismatch_precedes_encryption',bytes(data),6)
    data = bytearray(base); data[30] = ord('X')
    add('every_member_name_must_match',bytes(data),6)
    add('catalog-name-oom',base,11,cat_bytes=0,domain='typed names versus C++ catalog struct allocation; distinct phase diagnostics')
    for n in (1,64,2047):
        add('low-parse-'+str(n),base,0,parse_bytes=n,domain='Rust caller typed XML/decoder buffers; C++ byte XML scratch OOM, expect C++ OutOfMemory')
    for typed in ('no-entries','no-manifest','no-spine','no-chunk','no-xml','no-filter','fail-backend','no-decoder'):
        data = base
        if typed=='no-decoder': data=v.archive([('META-INF/container.xml',b'\x03\x00',8,1,0)]+v.BASE[1:])
        add('typed-'+typed,data,0 if typed=='no-filter' else 11 if typed in ('no-entries','no-manifest','no-spine') else 7,typed=typed,domain='Independent typed Rust resource case, no C++ equivalence',member_begins=[] if typed=='no-entries' else [0,1] if typed in ('no-manifest','no-spine') else [0,1,3] if typed=='no-filter' else [0])
    return result

def wire(case):
    return 'EA1 {} {} {} {} {} {} {} {}\n'.format(case.get('op','A'),case['data'].hex() or '-',case.get('cat_bytes',524288),case.get('parse_bytes',524288),case.get('max_read',32768),case.get('fail_call',-1),case.get('member',0),case.get('read_cap',2048))

def check(case, rows, cpp=False):
    def need(ok, why):
        if not ok: raise RuntimeError(case['id']+': '+why)
    expected = 7 if cpp and case['id']=='catalog-name-oom' else case['status']
    if cpp and case['id'].startswith(('low-parse-','no-byte-scratch')): expected=7
    need(int(rows[-3][1]) == expected,'independent status '+str(expected))
    if case.get('op','A')=='A': need(rows[-2][1] == rows[-2][4] == '0','unconditional arena reset')
    if 'metadata' in case: need(next(r[1:] for r in rows if r[0]=='M')==case['metadata'],'metadata')
    if 'finish' in case: need(next(int(r[1]) for r in rows if r[0]=='F')==case['finish'],'finish probe')
    if 'payload' in case: need(b''.join(bytes.fromhex(r[3]) for r in rows if r[0]=='C' and r[3]!='-')==case['payload'],'independent chapter payload')
    begins=[int(r[3]) for r in rows if r[:2]==['E','7']]
    if 'member_begins' in case: need(begins==case['member_begins'],'required member order')
    if cpp and case['id'].startswith(('low-parse-','no-byte-scratch')):
        need(begins==[0], 'C++ low byte cap must fail first required expansion before XML')
        need(any(r[:2]==['E','10'] and r[6]=='7' for r in rows),'C++ MemberEnd OOM')
    if case['id']=='valid-nav-not-parsed':
        need(begins==[0,1,3],'required ordering')
        need([int(r[6]) for r in rows if r[:2]==['E','6']]==[0,1,2,3],'comparison counts')
