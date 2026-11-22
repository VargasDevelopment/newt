"""MIT, original synthetic TrueType fixture from OpenType table specifications.
No font acquisition, fonttools or expected values copied from either engine.
"""
import struct, importlib.util
from pathlib import Path
H=lambda *x:struct.pack('>'+'H'*len(x),*x)
S=lambda *x:struct.pack('>'+'h'*len(x),*x)
U=lambda *x:struct.pack('>'+'I'*len(x),*x)
def font(root):
    # A closed clockwise 400x700 rectangle at x=100. At 10px/1000 UPEM:
    # xoff=1, yoff=-7, width=4, height=7, advance=6; all coverage bytes FF.
    rect=S(1,100,0,500,700)+H(3,0)+bytes([1]*4)+S(100,0,400,0)+S(0,700,0,-700)
    rect+=bytes((-len(rect))%4)
    glyf=rect*8;loca=U(0,0,*[len(rect)*i for i in range(1,9)])
    head=U(0x10000,0x10000,0,0x5f0f3cf5)+H(0,1000)+bytes(16)+S(0,0,500,700)+H(0,8)+S(2,1,0)
    hhea=U(0x10000)+S(800,-200,200)+H(600)+S(0,0,500,1,0,0)+bytes(8)+S(0)+H(9)
    maxp=U(0x10000)+H(9,4,1,0,0,2,0,0,0,0,0,0,0,0)
    # Format 12 cmap handles Latin input and the five Unicode ligature candidates.
    pairs=[(32,2),(65,1),(102,3),(105,4),(108,5),(0xfb00,6),(0xfb01,7),(0xfb02,8)]
    cmap_body=H(12,0)+U(16+12*len(pairs),0,len(pairs))+b''.join(U(cp,cp,g)for cp,g in pairs)
    cmap=H(0,1,3,10)+U(12)+cmap_body
    name='NRUST Rectangle'.encode('utf-16-be');names=H(0,1,18)+H(3,1,0x409,1,len(name),0)+name
    spec=importlib.util.spec_from_file_location('font_layout_fixture',root/'rust/tests/font_layout/corpus.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
    gsub=m.table(4,[m.ligature(result=7,sequence=(3,4))])
    # Pair adjustment -100 units for f,i => -64 in 26.6 at 10px.
    cov=m.coverage(3);pair=H(1,12,4,0,1,18)+cov+H(1,4,(-100)&65535)
    tables={b'head':head,b'hhea':hhea,b'maxp':maxp,b'hmtx':(H(600)+S(100))*9,b'loca':loca,b'glyf':glyf,b'cmap':cmap,b'name':names,b'post':U(0x30000)+bytes(28),b'GSUB':gsub,b'GPOS':m.table(2,[pair])}
    n=len(tables);power=2**(n.bit_length()-1);out=U(0x10000)+H(n,power*16,power.bit_length()-1,n*16-power*16);offset=12+n*16;records=b'';body=b''
    for tag,data in sorted(tables.items()):
        padded=data+bytes((-len(data))%4);checksum=sum(struct.unpack('>'+'I'*(len(padded)//4),padded))&0xffffffff
        records+=tag+U(checksum,offset,len(data));body+=padded;offset+=len(padded)
    out=bytearray(out+records+body)
    head_off=next(struct.unpack_from('>I',out,12+i*16+8)[0]for i in range(n)if out[12+i*16:16+i*16]==b'head')
    checksum=sum(struct.unpack('>'+'I'*(len(out)//4),out))&0xffffffff
    struct.pack_into('>I',out,head_off+8,(0xb1b0afba-checksum)&0xffffffff)
    return bytes(out)
def cases(root):
    b=font(root);prefix='FTFONT1\n'+b.hex()+'\n'
    yield 'rectangle',prefix+'INIT 0 10 400 0 0\nMETRIC 65 640\nRASTER 65 640\nLINE 640\nKERN 102 105 640\nLIGAG 102 105 0\nLIGA 102 105\nSTATUS\n',[
        'INIT 1','METRIC 1 384 1 7 4 7','RASTER 1 4 7 1 -7 6 '+'ff'*28,'LINE 1 512 -128 768','KERN -64','LIGAG 7','LIGA 64257','STATUS 1 0 0 0 0']
    yield 'stream',prefix+'INIT 1 10 400 0 0\nMETRIC 65 640\nRASTER 65 640\nLIGAG 102 105 0\nRELEASE 0\nLIGAG 102 105 0\nDEINIT\nINIT 1 10 400 0 0\nLIGAG 102 105 0\n',None
    yield 'budgets',prefix+'BUDGET 0 0\nINIT 1 10 400 0 0\nLIGAG 102 105 0\nKERN 102 105 640\nINIT 0 10 400 0 0\nLIGAG 102 105 0\nKERN 102 105 640\n', ['BUDGET','INIT 1','LIGAG 0','KERN 0','INIT 1','LIGAG 7','KERN -64']
    yield 'failures',prefix+'INIT 0 10 400 0 0\nMETRIC 0 640\nSTATUS\nMETRIC 65 63\nSTATUS\nGMETRIC 9999 640\nSTATUS\nDEINIT\nSTATUS\n',None
    yield 'style',prefix+'INIT 0 10 700 1 0\nMETRIC 65 640\nRASTER 65 640\nOPTIONS 1 40 0 0 0 0\nMETRIC 65 672\nRASTER 65 672\nINIT 0 10 400 0 0\nRASTER 65 640\n',None
    yield 'inspect',prefix+''.join(f'INSPECT {m} {n}\n'for m in (0,1)for n in(0,1,4,64)),None
    yield 'allocation',prefix+'INIT 0 10 400 0 0\nDEINIT\nALLOC 0\nINIT 1 10 400 0 0\nSTATUS\nALLOC 4294967295\nINIT 0 10 400 0 0\nRASTER 65 640\n',None
    yield 'read-failure',prefix+'INIT 1 10 400 0 1\nSTATUS\nINSPECT 1 4\n',None
    yield 'options',prefix+''.join(f'INIT 0 10 400 0 0\nOPTIONS {h} {v} {m} 1 12 100\nMETRIC 65 640\nRASTER 65 640\nSTATUS\n' for h,v,m in [(0,40,0),(1,40,0),(2,40,0),(3,40,1),(4,35,0),(4,36,0),(4,40,1)]),None
