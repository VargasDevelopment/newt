"""Runtime negative controls against already independently validated receipts."""
import copy

def run(protocol, corpus, case, rows):
    wire=corpus.wire(case)
    def raw(items): return ''.join(' '.join(r)+'\n' for r in items)
    def rejected(items):
        try: protocol.response(raw(items),wire)
        except ValueError: return
        raise RuntimeError('protocol negative control accepted')
    for i,row in enumerate(rows):
        if row[0] in ('E','S','D','END','M','P','F','C'):
            altered=copy.deepcopy(rows);altered[i]=altered[i][:-1]
            rejected(altered)
    for replacement in (['S','14'],['D','-1','0','0','0','0','0'],['R','0','1','2'],['E','11','0','0','0','0','0','0','0']):
        rejected([replacement]+copy.deepcopy(rows))
    if 'payload' in case and case['payload']:
        altered=copy.deepcopy(rows)
        row=next(r for r in altered if r[0]=='C' and int(r[2])>0)
        value=bytes.fromhex(row[3]);row[3]=(bytes([value[0]^1])+value[1:]).hex()
        protocol.response(raw(altered),wire)
        try: corpus.check(case,altered)
        except RuntimeError: pass
        else: raise RuntimeError('wrong chapter payload accepted')
    if case['id']=='valid-nav-not-parsed':
        # expected-v3 is Newt's own capture, so a regeneration from code that
        # skips a required member must still fail: the reply stays well-formed,
        # and only corpus.py's independent member order refuses it.
        begin=max(i for i,r in enumerate(rows) if r[:2]==['E','7'])
        end=next(i for i in range(begin,len(rows)) if rows[i][:2]==['E','10'])
        altered=copy.deepcopy(rows[:begin]+rows[end+1:])
        protocol.response(raw(altered),wire)
        try: corpus.check(case,altered)
        except RuntimeError: pass
        else: raise RuntimeError('regenerated reference without a required member accepted')
    changed={**case,'status':(case['status']+1)%14}
    try: corpus.check(changed,rows)
    except RuntimeError: return
    raise RuntimeError('independent expectation tampering accepted')
