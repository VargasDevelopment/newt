"""JPEG syntax authored from baseline coding rules, never decoder output."""
import struct

def segment(m,b): return bytes([255,m])+struct.pack('>H',len(b)+2)+b

def jpeg(width=8,height=8,sampling=0x11,color=False,dc=0,ac=False,restart=0,prefix=b'',ac_index=1,quant=None,coefficients=None,dc_values=None):
    n=3 if color else 1
    qvalues=bytes([1]*64 if quant is None else quant)
    assert len(qvalues)==64 and all(qvalues)
    q=segment(0xdb,b'\0'+qvalues)
    # DC categories 0..11 have canonical length-four codes 0000..1011.
    # AC EOB=00, (run0,size1)=01, (run0,size2)=10, ZRL=11.
    tables=b''
    for t in range(2 if color else 1):
        tables+=bytes([t])+bytes([0,0,0,12]+[0]*12)+bytes(range(12))
        tables+=bytes([0x10+t])+bytes([0,4]+[0]*14)+bytes([0,1,2,0xf0])
    ac_codes=None
    if coefficients is not None:
        symbols=[0,0xf0];previous_index=0
        for index,value in sorted(coefficients.items()):
            assert 1<=index<=63 and value!=0
            run=index-previous_index-1
            while run>=16:run-=16
            symbols.append((run<<4)|abs(value).bit_length());previous_index=index
        symbols=sorted(set(symbols));length=len(symbols).bit_length()
        lengths=bytes([0]*(length-1)+[len(symbols)]+[0]*(16-length))
        ac_codes={value:format(index,f'0{length}b')for index,value in enumerate(symbols)}
        tables=b''
        for t in range(2 if color else 1):
            tables+=bytes([t])+bytes([0,0,0,12]+[0]*12)+bytes(range(12))
            tables+=bytes([0x10+t])+lengths+bytes(symbols)
    h=segment(0xc4,tables)
    frame=bytes([8])+struct.pack('>HHB',height,width,n)+b''.join(bytes([i+1,sampling if i==0 else 0x11,0]) for i in range(n))
    scan=bytes([n])+b''.join(bytes([i+1,0 if i==0 else 0x11]) for i in range(n))+bytes([0,63,0])
    mx=sampling>>4;my=sampling&15;ny=mx*my
    entropy=bytearray();bits='';previous=[0,0,0];seq=0
    def flush():
        nonlocal bits
        bits+='1'*((-len(bits))%8)
        for i in range(0,len(bits),8):
            b=int(bits[i:i+8],2);entropy.append(b)
            if b==255:entropy.append(0)
        bits=''
    total=((width+mx*8-1)//(mx*8))*((height+my*8-1)//(my*8))
    for m in range(total):
        if restart and m and m%restart==0:
            flush();entropy.extend([255,0xd0+(seq%8)]);seq+=1;previous=[0,0,0]
        for block in range(ny+(2 if color else 0)):
            c=0 if block<ny else block-ny+1
            target=(dc if dc_values is None else dc_values[(m*ny+block)%len(dc_values)]) if c==0 else 0;diff=target-previous[c];previous[c]=target
            cat=abs(diff).bit_length();bits+=format(cat,'04b')
            if cat:bits+=format(diff if diff>=0 else diff+(1<<cat)-1,f'0{cat}b')
            if ac_codes is not None:
                last=0
                for index,value in sorted((coefficients if c==0 else {}).items()):
                    run=index-last-1
                    while run>=16:bits+=ac_codes[0xf0];run-=16
                    category=abs(value).bit_length()
                    bits+=ac_codes[(run<<4)|category]
                    bits+=format(value if value>=0 else value+(1<<category)-1,f'0{category}b')
                    last=index
                bits+=ac_codes[0]
                continue
            if ac and c==0:
                if ac_index==1:bits+='01'+'1'
                else:bits+='01'+'1'+'01'+'1' # adjacent zigzag1,2 coefficients
            bits+='00'
    flush()
    return prefix+b'\xff\xd8'+q+h+segment(0xc0,frame)+(segment(0xdd,struct.pack('>H',restart)) if restart else b'')+segment(0xda,scan)+entropy+b'\xff\xd9'

def request(data,mode='D',a=0,b=0,stop=0,chunk=1048576,fail=1048576):
    return f'JP1 {mode} {a} {b} {stop} {chunk} {fail} {data.hex()}\n'

def cases():
    for color in (False,True):
        for sampling in (0x11,0x21,0x22):
            for scale in range(4):
                for ac in (False,True):
                    label=f'{color}-{sampling}-{scale}-{ac}'
                    data=jpeg(37,25,sampling,color,64,ac)
                    yield label,request(data,a=scale)
    for value in (-512,-64,-1,0,64,512,2040):yield f'dc-{value}',request(jpeg(dc=value))
    for interval in (1,2,9):
        data=jpeg(161,33,0x22,True,64,True,interval)
        yield f'restart-{interval}',request(data,chunk=1)
        yield f'restart-stop-{interval}',request(data,stop=2,chunk=3)
        yield f'row-{interval}',request(data,'R',13,7,chunk=7)
        yield f'row-stop-{interval}',request(data,'R',13,7,stop=2,chunk=1)
        yield f'bad-restart-{interval}',request(data.replace(b'\xff\xd0',b'\xff\xd3',1))
    yield 'ac-two-axes',request(jpeg(ac=True,ac_index=2))
    yield 'ac-independent',request(jpeg(ac=True))
    data=jpeg(17,9,dc=-64)
    for cut in (1,2,5,70,len(data)-5,len(data)-2):yield f'partial-{cut}',request(data[:cut])
    for pos in (0,10,90,len(data)-4):yield f'io-{pos}',request(data,fail=pos,chunk=1)
    yield 'soi-search',request(jpeg(prefix=b'abcdef'))
    yield 'app-skip',request(data[:2]+segment(0xe1,bytes(1600))+data[2:],chunk=3)
    yield 'progressive-rejection',request(data.replace(b'\xff\xc0',b'\xff\xc2'))
    yield 'tiny-scale-empty',request(jpeg(1,1),a=3)
    eof=jpeg()
    yield 'row-eof-before-source-failure',request(eof,'R',8,8,chunk=1,fail=len(eof))
    yield 'direct-source-failure-at-eof',request(eof,chunk=1,fail=len(eof))
    for index in (9,12,35,63):
        yield f'higher-ac-{index}',request(jpeg(coefficients={index:32}))
        yield f'nonuniform-quant-ac-{index}',request(jpeg(quant=[2]+[(k%13)+1 for k in range(1,64)],coefficients={index:16}))
    yield 'distinct-neighboring-blocks',request(jpeg(32,16,0x22,True,dc_values=[0,64,128,-64,192,-128,256,-192]))
    yield 'nonuniform-quant-dc',request(jpeg(dc=64,quant=[2]+[(k%13)+1 for k in range(1,64)]))
    yield 'row-band-overwrite-stop',request(jpeg(16,32,0x22,True,dc_values=[0,64,128,-64,192,-128,256,-192]),'R',16,32,stop=1,chunk=1)

def independent(reply,req):
    if req==request(jpeg(16,32,0x22,True,dc_values=[0,64,128,-64,192,-128,256,-192]),'R',16,32,stop=1,chunk=1):
        # First MCU's top row: DC0 gives128; DC64 gives136. The later
        # band has different DC values, so overwriting before flush is visible.
        assert reply=='JP1 0 1 O:0:'+('80'*8+'88'*8)+'\n'
    # Independent flat DC example: quantizer1, coefficient64 => Y=136 everywhere.
    if req==request(jpeg(dc=64)):
        assert reply=='JP1 0 1 B:0:0:7:7:'+('88'*64)+'\n'
    if req==request(jpeg(dc=-64)):
        assert reply=='JP1 0 1 B:0:0:7:7:'+('78'*64)+'\n'

    if req==request(jpeg(ac=True)):
        # One horizontal coefficient +1: cosine sign changes at column4.
        # Its magnitude is <1 after inverse transform; truncation gives128/127.
        assert reply=='JP1 0 1 B:0:0:7:7:'+('808080807f7f7f7f'*8)+'\n'

    if req==request(jpeg(32,16,0x22,True,dc_values=[0,64,128,-64,192,-128,256,-192])):
        first=bytes([128]*8+[136]*8)*8+bytes([144]*8+[120]*8)*8
        second=bytes([152]*8+[112]*8)*8+bytes([160]*8+[104]*8)*8
        assert reply=='JP1 0 2 B:0:0:15:15:'+first.hex()+' B:16:0:31:15:'+second.hex()+'\n'
    if req==request(jpeg(dc=64,quant=[2]+[(k%13)+1 for k in range(1,64)])):
        assert reply=='JP1 0 1 B:0:0:7:7:'+('90'*64)+'\n'
    # Independent separable inverse-DCT bounds, not the decoder's integer/AAN
    # butterfly. Native fixed-point truncation is allowed one sample of error.
    import math
    zigzag=[0,1,8,16,9,2,3,10,17,24,32,25,18,11,4,5,12,19,26,33,40,48,41,34,27,20,13,6,7,14,21,28,35,42,49,56,57,50,43,36,29,22,15,23,30,37,44,51,58,59,52,45,38,31,39,46,53,60,61,54,47,55,62,63]
    for index in (9,12,35,63):
        for quant,coefficient in (([1]*64,32),([2]+[(k%13)+1 for k in range(1,64)],16)):
            if req!=request(jpeg(quant=quant,coefficients={index:coefficient})):continue
            words=reply.strip().split(' ')
            assert words[:3]==['JP1','0','1'] and words[3].startswith('B:0:0:7:7:')
            pixels=bytes.fromhex(words[3].split(':')[-1]);assert len(pixels)==64
            natural=zigzag[index];u=natural%8;v=natural//8
            factor=(1/math.sqrt(2)if u==0 else 1)*(1/math.sqrt(2)if v==0 else 1)
            for y in range(8):
                for x in range(8):
                    exact=128+factor*coefficient*quant[index]*math.cos((2*x+1)*u*math.pi/16)*math.cos((2*y+1)*v*math.pi/16)/4
                    assert abs(pixels[y*8+x]-math.floor(exact))<=1
