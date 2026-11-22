"""Independent, literal complete-pixel expectations; never runs either renderer."""
def font(maximum=2097151, missing=2097151, raster_missing=2097151, advance=2, kern=0, lig=0, w=1,h=1,x=0,y=0,pixels='ff',bitmap_advance=7):
    return [maximum,missing,raster_missing,advance,bitmap_advance,kern,lig,w,h,x,y,pixels]
def image(x=0,y=0,w=4,h=2,status=0,extra=0,rows=()):
    return [x,y,w,h,status,extra,len(rows),*[v for row in rows for v in row]]
def text_trace(cps=(65,),style=0,size=16,kern=True):
    # Explicit expected virtual dispatch order for one registered synthetic font.
    result=[];prev=0
    for cp in cps:
        if prev and kern:result += [f'h{prev}:0',f'h{cp}:0',f'k{prev}:{cp}:{size}:{style}:0']
        result += [f'h{cp}:0',f'r{cp}:{size}:0',f'h{cp}:0',f'a{cp}:{size}:{style}:0']
        prev=cp
    return ','.join(result)
def case(name,pixels,trace='-',answers='-',missing=0,first=999,status=0,mode=0,w=8,h=4,stride=2,fmt=1,rot=0,fill=255,arena=128,fonts=None,entries=None,runs=None,images=(),queries=()):
    if fonts is None: fonts=[font()]
    if entries is None: entries=[(0,0)] if fonts else []
    if runs is None: runs=[(0,0,16,0,'41')]
    tokens=['PR1',mode,w,h,stride,fmt,rot,fill,arena,len(fonts)]
    for f in fonts:tokens.extend(f)
    tokens += [len(entries)]
    for e in entries:tokens.extend(e)
    tokens += [len(runs)]
    for r in runs:tokens.extend(r)
    tokens += [len(images)]
    for i in images:tokens.extend(i)
    tokens += [len(queries)]
    for q in queries:tokens.extend(q)
    return dict(name=name,input=' '.join(map(str,tokens))+'\n',expected=f'R {missing} {first} {status} {pixels}\nT {trace}\nQ {answers}\n')
def corpus():
    c=[];blank='ffffffffffffffff';one='7fffffffffffffff';tr=text_trace()
    for rot,p in enumerate([one,'ffffffffffff7fff','feffffffffffffff','fffffffffffffeff']):
        c.append(case('rotation-'+str(rot),p,tr,rot=rot))
    for fmt,p in [(0,'e3ffffffffffffff'),(1,'f3ffffffffffffff')]:
        c.append(case('threshold-'+str(fmt),p,tr,fmt=fmt,fonts=[font(w=6,pixels='00272877788c')]))
    c.append(case('gray-coverage','ffd8d7888773ffffffff'+'ff'*30,tr,fmt=2,stride=10,fonts=[font(w=6,pixels='00272877788c')]))
    c.append(case('gray-min-overlap','3b'+'c8'*39,text_trace()+','+text_trace(),fmt=2,stride=10,fill=200,fonts=[font(pixels='c4')],runs=[(0,0,16,0,'41'),(0,0,16,0,'41')]))
    c.append(case('zero-full','bfffffffffffffff',tr,fonts=[font(w=2,pixels='00ff')]))
    c.append(case('synthetic-bold','3fffffffffffffff',text_trace(style=1),runs=[(0,0,16,1,'41')]))
    c.append(case('real-bold',one,text_trace(style=1),entries=[(0,1)],runs=[(0,0,16,1,'41')]))
    c.append(case('clip-negative','7fffffffffffffff',tr,fonts=[font(w=2,x=-1,pixels='ffff')]))
    c.append(case('square-panel','ffffffffffffefff',tr,w=4,h=4,rot=0,runs=[(3,3,16,0,'41')]))
    c.append(case('kerning-order','5fffffffffffffff',text_trace((65,66)),fonts=[font(advance=3,kern=-1)],runs=[(0,0,16,0,'4142')]))
    c.append(case('underline-small','7fffffff3fffffff',text_trace(style=4),runs=[(0,0,16,4,'41')]))
    c.append(case('underline-large','ffffffff3fff3fff',text_trace(style=4,size=28),runs=[(0,-1,28,4,'41')]))
    c.append(case('missing','ffffffffffffffff',text_trace((65,66)),fonts=[font(maximum=0)],runs=[(0,0,16,0,'4142')],missing=2,first=65))
    c.append(case('whitespace','ffffffffffffffff',text_trace((32,160)),fonts=[font(maximum=0)],runs=[(0,0,16,0,'20c2a0')]))
    c.append(case('raster-missing','ffffffffffffffff',tr,fonts=[font(raster_missing=65)],missing=1,first=65))
    c.append(case('empty-preserves-first',blank,runs=[],fonts=[]))
    for name,b,cp,count in [('stray','80',128,1),('truncated','e282',130,1),('overlong-space','c0a0',999,0),('high-leader','ff80',448,1),('interrupted','e241',2,2),('zero-cp','f8808080',0,1)]:
        c.append(case('utf8-'+name,blank,fonts=[],runs=[(0,0,16,0,b)],missing=count,first=cp))
    # Identical bank object in distinct registrations: pointer identity still permits kern/ligature.
    c.append(case('same-bank-style-identity',blank,
        'h65:0,h65:0,h66:0,h66:0,k65:66:16:1:0,h65:0,h65:0,h66:0,h66:0,g65:66:1:0,h67:0,h67:0',
        answers='-3 67 5',mode=3,fonts=[font(missing=65,kern=-3,lig=67)],entries=[(0,0),(0,1)],runs=[],queries=[(1,65,66,16,1),(2,65,66,16,1),(7,0,0,0,0)]))
    c.append(case('fallback-score',blank,'h65:0,h65:1,h65:2',answers='258',mode=3,fonts=[font(),font(),font()],entries=[(0,0),(1,2),(2,1)],runs=[],queries=[(6,65,0,16,3)]))
    c.append(case('missing-face-zero-fallback',blank,'h65:0,h65:1',answers='1',mode=3,fonts=[font(maximum=0),font(maximum=0)],entries=[(0,1),(1,0)],runs=[],queries=[(6,65,0,16,2)]))
    c.append(case('strict-covers',blank,'h65:0,h65:1',answers='0',mode=3,fonts=[font(maximum=0),font(maximum=0)],entries=[(0,0),(1,1)],runs=[],queries=[(3,65,0,16,0)]))
    c.append(case('first-face-metrics',blank,'l16:0,s16:0',answers='17 15 5',mode=3,entries=[(0,0),(0,1)],runs=[],queries=[(4,0,0,16,0),(5,0,0,16,0),(7,0,0,0,0)]))
    c.append(case('empty-chain-metrics',blank,answers='16 16 0 -1',mode=3,fonts=[],runs=[],queries=[(4,0,0,16,0),(5,0,0,16,0),(7,0,0,0,0),(6,65,0,16,0)]))
    # Diffusion residuals: [100,100,100,100] -> B W B B; next [128]*4 -> W B W W.
    rows=[(0,4,'64646464'),(1,4,'80808080')]
    c.append(case('image-diffusion','4fffbfffffffffff','d0:104,i0:0:4,i0:1:4,e0:0',mode=1,runs=[],images=[image(rows=rows)]))
    # Full byte clipping is independent of diffusion, and row width may exceed placement width.
    c.append(case('image-width-clamp','3fffffffffffffff','d0:112,i0:0:4,e0:0',mode=1,runs=[],images=[image(w=2,rows=[(0,4,'00000000')])]))
    c.append(case('image-gray-overwrite','ffffff'+'ff'*5+'00ffffff'+'ff'*20,'d0:104,i0:1:4,e0:0',mode=1,fmt=2,stride=8,runs=[],images=[image(rows=[(1,4,'00ffffff')])]))
    c.append(case('image-gray-wide','00000000'+'ff'*28,'d0:112,i0:0:4,e0:0',mode=1,fmt=2,stride=8,runs=[],images=[image(w=2,rows=[(0,4,'00000000')])]))
    c.append(case('image-partial-row','7fffbfffffffffff','d0:104,i0:0:1,i0:1:2,e0:0',mode=1,runs=[],images=[image(rows=[(0,1,'64'),(1,2,'6496')])]))
    c.append(case('image-clipped','ffffffffffffffff','d0:112,i0:0:2,e0:0',mode=1,runs=[],images=[image(x=-1,w=2,rows=[(0,2,'6464')])]))
    c.append(case('image-last-error',blank,'d0:104,e0:1,d1:104,e1:6,d2:104,e2:9,d3:104,e3:0',mode=1,runs=[],images=[image(status=s) for s in [1,6,9,0]],status=9))
    c.append(case('image-oom-early',blank,mode=1,runs=[],arena=23,images=[image(),image(status=1)],status=7))
    c.append(case('image-decoder-oom',blank,'d0:0,e0:7,d1:0,e1:0',mode=1,runs=[],arena=24,images=[image(extra=1),image()],status=7))
    c.append(case('image-attempt-zero','7fff7fffffffffff','d0:104,i0:0:1,e0:1,d1:104,i1:0:1,e1:0',mode=1,runs=[],images=[image(rows=[(0,1,'64')],status=1),image(y=1,rows=[(0,1,'64')])],status=1))
    c.append(case('combined-text-before-image','ff'*32,tr+',d0:104,i0:0:1,e0:0',mode=2,fmt=2,stride=8,images=[image(rows=[(0,1,'ff')])]))
    c.append(case('cross-font-kern',blank,'h65:0,h65:1,h66:0,h66:1',answers='0',mode=3,fonts=[font(maximum=65),font(missing=65)],entries=[(0,0),(1,0)],runs=[],queries=[(1,65,66,16,0)]))
    c.append(case('cross-font-ligature',blank,'h65:0,h65:1,h66:0,h66:1,g65:66:0:0,h67:0,h67:1',answers='0',mode=3,fonts=[font(maximum=66,lig=67),font(missing=65)],entries=[(0,0),(1,0)],runs=[],queries=[(2,65,66,16,0)]))
    c.append(case('fallback-bold',one,'h65:0,h65:1,r65:16:1,h65:0,h65:1,a65:16:1:1',fonts=[font(maximum=0),font()],entries=[(0,0),(1,1)],runs=[(0,0,16,1,'41')]))
    c.append(case('dither-floor-at-zero',blank,tr,fmt=0,fonts=[font(pixels='28')]))
    c.append(case('sharp-threshold-exact',one,tr,fonts=[font(pixels='78')]))
    c.append(case('gray-zero-keeps-dark','11'*32,tr,fmt=2,stride=8,fill=17,fonts=[font(pixels='00')]))
    for rot,p in enumerate([one,'ffffffffffff7fff','feffffffffffffff','fffffffffffffeff']):
        c.append(case('image-rotation-'+str(rot),p,'d0:104,i0:0:1,e0:0',mode=1,rot=rot,runs=[],images=[image(rows=[(0,1,'00')])]))
    c.append(case('image-truncation-negative',blank,'d0:112,i0:0:2,e0:0',mode=1,runs=[],images=[image(w=2,rows=[(0,2,'c898')])]))
    c.append(case('image-zero-exact','7fffffffffffffff','d0:104,i0:0:1,e0:0',mode=1,runs=[],images=[image(rows=[(0,1,'7f')])]))
    c.append(case('image-unsupported-ignored',blank,'d0:104,e0:1,d1:104,e1:6',mode=1,runs=[],images=[image(status=1),image(status=6)],status=1))
    c.append(case('dither-fringe',one,tr,fmt=0,fonts=[font(pixels='3c')]))
    c.append(case('dither-high-band',blank,tr,fmt=0,fonts=[font(pixels='78')],runs=[(0,3,16,0,'41')]))
    c.append(case('gray-keep-dark','11'*32,tr,fmt=2,stride=8,fill=17,fonts=[font(pixels='c4')]))
    c.append(case('utf8-four',blank,fonts=[],runs=[(0,0,16,0,'f0808080')],missing=1,first=0))
    c.append(case('measured-not-bitmap-advance','5fffffffffffffff',text_trace((65,66)),fonts=[font(advance=2,bitmap_advance=7)],runs=[(0,0,16,0,'4142')]))
    # Hand-derived panel rows for logical coverage [60,255,0;120,0,255].
    # Fringe (0,0)=60 passes logical Bayer15, but fails panel Bayer240
    # in Portrait (panel 0,3). No renderer output supplies these bytes.
    matrix = [
        ['3fff5fffffffffff','ffffbfff7fff3fff','fcfffefffdffffff','fffffffffafffcff'],
        ['bfff5fffffffffff','ffffbfff7fffbfff','fdfffefffdffffff','fffffffffafffdff'],
        [],
    ]
    # Gray rows include two untouched padding bytes; spell each complete row.
    matrix[2] = [
        'c300ffffffffffffffff'+'87ff00ffffffffffffff'+'ff'*20,
        'ff'*10+'ff00ffffffffffffffff'+'00ffffffffffffffffff'+'c387ffffffffffffffff',
        'ffffffffffff87c3ffff'+'ffffffffffffff00ffff'+'ffffffffffff00ffffff'+'ff'*10,
        'ff'*20+'ffffffffff00ff87ffff'+'ffffffffffff00c3ffff',
    ]
    for fmt, frames in enumerate(matrix):
        for rot, pixels in enumerate(frames):
            c.append(case(f'matrix-{fmt}-{rot}',pixels,text_trace(),fmt=fmt,rot=rot,
                          stride=10 if fmt==2 else 2,fonts=[font(w=3,h=2,pixels='3cff007800ff')]))
    # Independent image matrix: two black samples at logical (0,0) and (1,1).
    image_mono = ['7fffbfffffffffff','ffffffffbfff7fff',
                  'fefffdffffffffff','fffffffffdfffeff']
    image_gray = [
        '00ffffffffffffffffff'+'ff00ffffffffffffffff'+'ff'*20,
        'ff'*20+'ff00ffffffffffffffff'+'00ffffffffffffffffff',
        'ffffffffffffff00ffff'+'ffffffffffff00ffffff'+'ff'*20,
        'ff'*20+'ffffffffffff00ffffff'+'ffffffffffffff00ffff',
    ]
    for fmt in range(3):
        for rot, pixels in enumerate(image_gray if fmt==2 else image_mono):
            c.append(case(f'image-matrix-{fmt}-{rot}',pixels,
                          'd0:108,i0:0:3,i0:1:3,e0:0',mode=1,runs=[],fmt=fmt,rot=rot,
                          stride=10 if fmt==2 else 2,
                          images=[image(w=3,rows=[(0,3,'00ffff'),(1,3,'ff00ff')])]))
    return c
