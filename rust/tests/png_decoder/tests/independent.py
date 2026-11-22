"""Authored samples precede filtering/compression; no decoder/oracle supplies expectations."""
import math
import struct
import zlib
import vectors as v

LEGAL = ((0,(1,2,4,8,16)),(2,(8,16)),(3,(1,2,4,8)),(4,(8,16)),(6,(8,16)))


def authored(color, depth, width=9, height=7):
    maximum = (1 << depth)-1
    channels = {0:1,2:3,3:1,4:2,6:4}[color]
    # Deliberately include intermediate 16-bit values with nonzero rounding residue.
    return [[[((x*43+y*83+c*67+19) % (maximum+1)) if color==3 else
              ((x*15427+y*7951+c*22739+32768) % (maximum+1))
              for c in range(channels)] for x in range(width)] for y in range(height)]


def expected_pixels(image, color, depth, transparent, gamma=None, display=0):
    maximum=(1 << depth)-1
    result=[]
    for row in image:
        out=[]
        for samples in row:
            if color==3:
                i=samples[0]
                rgba=[(i*17)&255,(255-i*13)&255,(i*59)&255,(i*37)&255 if transparent else 255]
                scale=255
            else:
                rgb=samples[:3] if color in (2,6) else samples[:1]*3
                alpha=samples[-1] if color in (4,6) else (0 if transparent and samples==image[0][0] else maximum)
                rgba=rgb+[alpha];scale=maximum
            converted=[(s*255+scale//2)//scale for s in rgba]
            if gamma and display:
                exponent=(100000/gamma)/(display/1000)
                converted[:3]=[math.floor((s/scale)**exponent*255+0.5) for s in rgba[:3]]
            out.append(bytes(converted).hex())
        result.append(out)
    return result


def pixel_cases():
    for color,depths in LEGAL:
        for depth in depths:
            image=authored(color,depth)
            if color==3:
                image=[[[p[0] % min(1<<depth,16)] for p in row] for row in image]
            for f in range(5):
                name=f'ind-c{color}-d{depth}-f{f}'
                data=v.png(color=color,depth=depth,filters=(f,),transparent=True,image=image,split=7)
                yield name,v.request(data),expected_pixels(image,color,depth,True)
            for display in (1000,2200):
                data=v.png(color=color,depth=depth,gamma=45455,image=image)
                yield f'ind-gamma-c{color}-d{depth}-{display}',v.request(data,gamma=display),expected_pixels(image,color,depth,False,45455,display)


def check_pixels(words, pixels):
    nr=int(words[2]); events=words[5+nr:]
    h,w=len(pixels),len(pixels[0])
    expected=[f'I:{w}:{h}:0']+[f'P:{x}:{y}:1:1:{p}' for y,row in enumerate(pixels) for x,p in enumerate(row)]+['D']
    if words[1]!='0' or words[4]!='000000' or events!=expected:
        raise AssertionError('independently authored full-image expectation differs')


def literal_cases():
    image=[[[0],[64]],[[128],[255]]]
    data=v.png(width=2,height=2,color=0,depth=8,image=image,filters=(0,))
    for name,kwargs,rows in (
        ('literal-gray',dict(dw=2,dh=2),['O:0:2:0040','O:1:2:80ff']),
        ('literal-scale',dict(dw=1,dh=1),['O:0:1:6f']),
        ('literal-stop',dict(dw=2,dh=2,stop=1),['O:0:2:0040'])):
        req=v.request(data,mode=1,sw=2,sh=2,**kwargs)
        yield name,req,f'PN1 0 1 {len(rows)} 000000 R:0:{len(data)}:{len(data)} '+ ' '.join(rows)+'\n'
    yield 'literal-io',v.request(data,mode=1,sw=2,sh=2,dw=1,dh=1,fail=0),f'PN1 1 1 0 000000 R:0:{len(data)}:-1\n'
    for cut in (1,7,8):
        prefix=data[:cut]
        # Low-level stalls on an incomplete signature. The wrapper ignores a
        # non-error partial feed and flushes its initial white row at EOF.
        consumed=8 if cut==8 else 0
        yield f'literal-trunc-feed-{cut}',v.request(prefix),f'PN1 0 1 0 000000 F:0:{cut}:{consumed}\n'
        yield f'literal-trunc-rows-{cut}',v.request(prefix,mode=1,sw=1,sh=1,dw=1,dh=1),f'PN1 0 1 1 000000 R:0:{cut}:{cut} O:0:1:ff\n'
    header=data[:33]
    for kind,bg in ((b'bKGD','000000'),(b'BKGD','808080')):
        prefix=header+v.chunk(kind,b'\x80\x00')
        yield 'literal-background-'+kind.decode(),v.request(prefix),f'PN1 0 1 0 {bg} F:0:{len(prefix)}:{len(prefix)}\n'
    # A/C tie: authored unfiltered rows [2,3], [0,100].
    scan=b'\x00\x02\x03\x04\xfe\x64'
    tie=b'\x89PNG\r\n\x1a\n'+v.chunk(b'IHDR',struct.pack('>IIBBBBB',2,2,8,0,0,0,0))+v.chunk(b'IDAT',zlib.compress(scan))+v.chunk(b'IEND',b'')
    yield 'literal-paeth',v.request(tie),None


def check_literal(name, words, text, expected):
    if name=='literal-paeth':
        check_pixels(words, [['020202ff','030303ff'],['000000ff','646464ff']])
    elif text!=expected:
        raise AssertionError(f'literal trace differs: {name}')
