"""JPEG syntax and hand-authored DC grid answers; no oracle-derived fixtures."""
import struct
from vectors import segment,request,jpeg

def progressive(width=16,height=16,sampling=0x11,color=False,values=(0,64,128,192),quant=1,al=0,restart=0,luma_only=False):
    comps=[(1,sampling)]+([(2,0x11),(3,0x11)] if color else [])
    scan=comps[:1] if luma_only else comps
    # Categories 0..15: sixteen distinct length-five codes. AC EOB length1.
    tables=b'\0'+bytes([0,0,0,0,16]+[0]*11)+bytes(range(16))
    tables+=b'\x10'+bytes([1]+[0]*15)+b'\0'
    q=segment(0xdb,b'\0'+bytes([quant])*64)
    sof=segment(0xc2,b'\x08'+struct.pack('>HHB',height,width,len(comps))+b''.join(bytes([i,hv,0]) for i,hv in comps))
    out=bytearray(b'\xff\xd8'+q+segment(0xc4,tables)+sof)
    if restart:out+=segment(0xdd,struct.pack('>H',restart))
    out+=segment(0xda,bytes([len(scan)])+b''.join(bytes([i,0])for i,_ in scan)+bytes([0,0,al]))
    bits='';pred=[0]*3;block=0
    def flush():
        nonlocal bits
        bits+='1'*((-len(bits))%8)
        for i in range(0,len(bits),8):
            b=int(bits[i:i+8],2);out.append(b)
            if b==255:out.append(0)
        bits=''
    inter=len(scan)>1;hm=sampling>>4;vm=sampling&15
    cols=(width+8*hm-1)//(8*hm) if inter else (width+7)//8
    rows=(height+8*vm-1)//(8*vm) if inter else (height+7)//8
    for m in range(cols*rows):
        if restart and m and m%restart==0:flush();out.extend([255,0xd0+((m//restart-1)%8)]);pred=[0]*3
        for ci,(_,hv) in enumerate(scan):
            for _ in range((hv>>4)*(hv&15) if inter else 1):
                value=values[block%len(values)] if ci==0 else 0
                if ci==0:block+=1
                diff=value-pred[ci];pred[ci]=value;cat=abs(diff).bit_length();bits+=format(cat,'05b')
                if cat:bits+=format(diff if diff>=0 else diff+(1<<cat)-1,f'0{cat}b')
    flush()
    # Later scans are deliberately not consumed by the preview. Complete the JPEG
    # with zero chroma DC and zero AC; al fixture includes a zero DC refinement.
    out+=segment(0xdd,b'\0\0')
    if color and luma_only:
        for ci,hv in comps[1:]:
            out+=segment(0xda,bytes([1,ci,0,0,0,0]));bits='0'*(((width+8*hm-1)//(8*hm))*((height+8*vm-1)//(8*vm))*5);flush()
    if al:
        out+=segment(0xda,bytes([len(scan)])+b''.join(bytes([i,0])for i,_ in scan)+bytes([0,0,(al<<4)|(al-1)]))
        bits='0'*(cols*rows*sum((hv>>4)*(hv&15)if inter else 1 for _,hv in scan));flush()
    for ci,hv in comps:
        out+=segment(0xda,bytes([1,ci,0,1,63,0]))
        cw=(width*(hv>>4)+hm-1)//hm;ch=(height*(hv&15)+vm-1)//vm
        bits='0'*(((cw+7)//8)*((ch+7)//8));flush()
    return bytes(out)+b'\xff\xd9'

def cases():
    for color in (False,True):
        for sampling in (0x11,0x21,0x22):
            for dst in ((2,2),(7,5),(1,1)):
                data=progressive(17,19,sampling,color)
                for mode in ('P','A'):
                    yield f'prog-{color}-{sampling}-{dst}-{mode}',request(data,mode,*dst,chunk=1)
    for interval in (1,2,7):
        data=progressive(49,33,0x22,True,restart=interval)
        yield f'prog-restart-{interval}',request(data,'P',9,5,chunk=3)
        yield f'prog-stop-{interval}',request(data,'P',9,5,stop=2,chunk=1)
        yield f'prog-wrong-restart-{interval}',request(data.replace(b'\xff\xd0',b'\xff\xd3',1),'P',9,5)
        yield f'prog-missing-restart-{interval}',request(data.replace(b'\xff\xd0',b'\xff\xd9',1),'P',9,5)
    for args in ({'quant':2},{'al':1},{'color':True,'sampling':0x22,'luma_only':True}):
        yield f'prog-options-{args}',request(progressive(**args),'P',3,3,chunk=7)
    for dims in ((2,2),(3,3),(1,1),(1,3),(3,1)):
        yield f'prog-independent-{dims}',request(progressive(), 'P',*dims)
    yield 'prog-independent-stop',request(progressive(),'P',3,3,stop=1)
    data=progressive()
    for cut in (1,2,5,70,len(data)//2,len(data)-2):
        yield f'prog-truncated-{cut}',request(data[:cut],'P',3,3,chunk=1)
    for fail in (0,2,10,90,150,len(data)):
        yield f'prog-io-{fail}',request(data,'P',3,3,chunk=1,fail=fail)
    for marker in (b'\xff\xc0',b'\xff\xc1'):
        yield f'prog-baseline-mislabeled-{marker.hex()}',request(data.replace(b'\xff\xc2',marker),'P',3,3)
    sos=data.index(b'\xff\xda');tail=sos+2+int.from_bytes(data[sos+2:sos+4],'big')
    for field,value in ((tail-3,1),(tail-2,63),(tail-1,0x10)):
        bad=data[:field]+bytes([value])+data[field+1:]
        yield f'prog-unsupported-scan-{field}',request(bad,'P',3,3)
    yield 'prog-ignored-later-truncation',request(data[:tail+20],'P',3,3)
    for fail in (tail,tail+1,tail+2):
        yield f'prog-entropy-io-{fail}',request(data,'P',3,3,chunk=1,fail=fail)
    yield 'prog-stuffed-entropy-clamp',request(progressive(values=(32767,32767,32767,32767)),'P',2,2,chunk=1)
    # Sixteen-bit DQT, same DC quantizer2, so independently checked output agrees.
    wideq=data[:2]+segment(0xdb,b'\x10'+struct.pack('>H',2)*64)+data[2+69:]
    yield 'prog-quant16',request(wideq,'P',3,3,chunk=1)
    frame=data.index(b'\xff\xc2');zero_width=data[:frame+7]+b'\0\0'+data[frame+9:]
    yield 'prog-zero-width',request(zero_width,'P',3,3)
    chroma=progressive(color=True,sampling=0x11)
    frame=chroma.index(b'\xff\xc2')
    # Cb sampling2x1 larger than Y is unsupported by the original preview.
    chroma=chroma[:frame+14]+b'\x21'+chroma[frame+15:]
    yield 'prog-chroma-larger-than-luma',request(chroma,'P',3,3)
    for mode in ('P','A'):
        yield f'prog-app-{mode}',request(data[:2]+segment(0xe1,bytes(1600))+data[2:],mode,3,3,chunk=3)
    yield 'auto-baseline',request(jpeg(17,9,dc=64),'A',8,4,chunk=1)
    yield 'auto-short',request(b'\xff\xd8','A',3,3)
    for index,image in enumerate((progressive(17,19),jpeg(31,9),b'\xff\xd8')):
        yield f'probe-metadata-{index}',request(image,'I',1,1,chunk=1)
    yield 'baseline-10703-witness',request(jpeg(coefficients={9:64}))

def independent(reply,req):
    # Four block averages 128,136 / 144,152 give this linear 3x3 surface.
    answers={(2,2):['8088','9098'],(3,3):['808488','888c90','909498'],(1,1):['98'],(1,3):['88','90','98'],(3,1):['909498']}
    for dims,rows in answers.items():
        if req==request(progressive(),'P',*dims):
            assert reply==f'JP1 0 {len(rows)}'+''.join(f' O:{y}:{row}' for y,row in enumerate(rows))+'\n'
    if req==request(progressive(),'P',3,3,stop=1):assert reply=='JP1 0 1 O:0:808488\n'
    if req==request(progressive(quant=2),'P',3,3,chunk=7):assert reply=='JP1 0 3 O:0:808890 O:1:9098a0 O:2:a0a8b0\n'

    data=progressive()
    wideq=data[:2]+segment(0xdb,b'\x10'+struct.pack('>H',2)*64)+data[2+69:]
    if req==request(wideq,'P',3,3,chunk=1):assert reply=='JP1 0 3 O:0:808890 O:1:9098a0 O:2:a0a8b0\n'
    if req==request(progressive(values=(32767,32767,32767,32767)),'P',2,2,chunk=1):assert reply=='JP1 0 2 O:0:ffff O:1:ffff\n'

    if req==request(progressive(17,19),'I',1,1,chunk=1):assert reply=='JP1 0 1 I:17:19:1\n'
    if req==request(jpeg(31,9),'I',1,1,chunk=1):assert reply=='JP1 0 1 I:31:9:0\n'
    if req==request(b'\xff\xd8','I',1,1,chunk=1):assert reply=='JP1 0 0\n'
