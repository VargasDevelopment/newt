"""Independent complete-page expectations and heterogeneous differential inputs.
These are authored expectations, not recorded output. Root must run the drivers.
"""
def request(text,width=80,height=32,size=8,align='left',focus=0,min_pages=4294967295,max_feeds=4294967295,stop=0,read=4096,fail=-1,calls=1):
    return f'L {text.encode().hex() or "-"} {width} {height} {size} {align} {focus} {min_pages} {max_feeds} {stop} {read} {fail} {calls}\n'
CASES = {
 'single':request('<body><p>a</p></body>'),
 'collapse':request('<body><p>  a \t b\r\n </p></body>'),
 'style':request('<body><p>a<b>b</b>c</p></body>'),
 'headings':request('<body><h1>A</h1><h3>B</h3><small>C</small></body>'),
 'css':request('<html><head><style>p.x {text-align:center;font-size:150%} b {font-weight:normal}</style></head><body><p class="x">a<b>b</b></p></body></html>'),
 'links':request('<body><p><a id="x" href="#x">abc</a></p></body>'),
 'wrap':request('<body><p>aa bb cc dd ee ff gg</p></body>',width=20,height=16),
 'cjk':request('<body><p>日本語。」「A韓國한국 TV를</p></body>',width=32,align='justify'),
 'rtl':request('<body><p>אב(12) لا بَاب</p></body>',width=32),
 'focus':request('<body><p>affinity O’Neil 123</p></body>',focus=1),
 'soft-hyphen':request('<body><p>ab\u00adcd efgh</p></body>',width=16),
 'suppression':request('<html><head><title>hidden</title></head><body><script>hidden</script><div style="display:none">hidden</div><p>x</p></body></html>'),
 'entity':request('<body><p>&nbsp;&amp;&#x1f600;</p></body>',read=1),
 'hardbreak':request('<body><p>a<br/><br/>b</p><hr/><p>c</p></body>'),
 'same-chunk-error':request('<body><p>a</p></broken>'),
 'source-error':request('<body><p>a</p></body>',fail=0),
 'stop':request('<body><p>aa bb cc dd ee ff</p></body>',width=8,height=8,stop=1),
 'zero-pages':request('<body><p>a</p></body>',min_pages=0),
 'empty':request(''),
 'paragraph-cap':request('<body><p>'+'a '*4500+'</p></body>',width=200,height=200),
}
PRODUCER = {
 'zero-feeds':request('<body><p>a</p></body>',max_feeds=0),
 'bounded-no-page':request('<body>'+' '*8192+'<p>a</p></body>',max_feeds=1,calls=2),
 'bounded-eof':request('<body><p>a</p></body>',max_feeds=1,calls=3),
}
# Complete page/run/image/link expectations independent of either implementation.
PAGES = {
 'single': [((0,0,1,0,0),[('T','61',0,8,8,0)])],
 'collapse': [((0,0,1,0,0),[('T','612062',0,8,8,0)])],
 'style': [((0,0,3,0,0),[('T','61',0,8,8,0),('T','62',4,8,8,1),('T','63',9,8,8,0)])],
 'zero-pages': [],'empty': [],'source-error': [],
}
# Dimension-only PNG fixture: actual pinned probe accepts the first 26 bytes.
def png(w,h):
    return (b'\x89PNG\r\n\x1a\n'+b'\x00\x00\x00\x0dIHDR'+w.to_bytes(4,'big')+h.to_bytes(4,'big')+b'\x08\x00').hex()
def image_request(text,w,h,**kwargs):return request(text,**kwargs).rstrip('\n')+' '+png(w,h)+'\n'
CASES.update({
 'image-fit':image_request('<body><img src="img.png"/></body>',100,50,width=40,height=32),
 'image-flow':image_request('<body><p>a</p><svg:image xmlns:svg="s" xmlns:xlink="x" xlink:href="img.png"/><p>b</p></body>',10,12,width=40,height=32),
 'image-missing':image_request('<body><img src="missing.png"/><p>x</p></body>',10,12),
 'image-unknown':request('<body><img src="img.png"/><p>x</p></body>').rstrip('\n')+' '+('00'*26)+'\n',
 'image-duplicate':image_request('<body><img src="img.png"/><img src="./img.png"/></body>',8,8),
})
PAGES['image-fit']=[((0,0,0,1,0),[('I','696d672e706e67',0,0,40,20)])]
# One sorted FIBH pattern, ab1c: break after ab with 2/3 minima.
FIBH=(b'FIBH\x01\x00\x03\x00'+(1).to_bytes(4,'little')+(0).to_bytes(4,'little')+b'\x03abc\x00\x00\x01\x00').hex()
CASES.update({
 'hyphenation':request('<body><p>abcdef abcdef</p></body>',width=16).rstrip('\n')+' - 1 '+FIBH+'\n',
 'arabic-uncovered':request('<body><p>لا باب</p></body>').rstrip('\n')+' - 0\n',
 'korean-keep-all':request('<body><p>한국어한국어 TV를 한국</p></body>',width=24,align='justify').rstrip('\n')+' - 1 - ko-keep-all\n',
 'inline-margin':request('<body><p>a<span style="font-size:150%;margin-left:1em">AV</span>b</p></body>'),
 'multibyte-locators':request('<body><p>😀 日本 ab cd ef gh</p><p id="after">z</p></body>',width=16,height=8),
})
# Full hand-derived protocols for small cases, including reads and all callbacks.
n=len('<body><p>a</p></body>')
COMPLETE={
 'single':f'O Ok 0 Parsing\nR 0 {n} {n}\nFH 8 8\nFA 97 8 0 4\nFA 97 8 0 4\nFB 8 8\nFA 97 8 0 4\nP 0 0 1 0 0\nT 61 0 8 8 0\nS Ok {n} Done 1 1\nEND\n',
 'zero-pages':'O Ok 0 Parsing\nS Ok 0 Parsing 0 0\nEND\n',
 'empty':'O Ok 0 Parsing\nS ParseError 0 Failed 0 0\nEND\n',
 'source-error':f'O Ok 0 Parsing\nR 0 {n} -1\nS IoError 0 Failed 0 0\nEND\n',
}
ni=len('<body><img src="img.png"/></body>')
COMPLETE['image-fit']=(f'R 0 {ni} {ni}\nR 0 {ni} {ni}\nR 0 26 26\nO Ok 0 Parsing\nR 0 {ni} {ni}\nP 0 0 0 1 0\nI 696d672e706e67 0 0 40 20\nS Ok {ni} Done 1 0\nEND\n')
CASES['invalid-width']=request('<body/>',width=0)
PAGES['invalid-width']=[]
COMPLETE['invalid-width']='O Unsupported 0 Idle\nS Unsupported 0 Idle 0 0\nEND\n'
PAGES.update({
 'entity':[((0,0,1,0,0),[('T','c2a026f09f9880',0,8,8,0)])],
 'suppression':[((0,0,1,0,0),[('T','78',0,8,8,0)])],
 'image-missing':[((0,0,1,0,0),[('T','78',0,8,8,0)])],
 'image-unknown':[((0,0,1,0,0),[('T','78',0,8,8,0)])],
 'image-duplicate':[((0,0,0,2,0),[('I','696d672e706e67',36,0,8,8),('I','696d672e706e67',36,12,8,8)])],
 'links':[((0,0,1,0,1),[('T','616263',0,8,8,0),('K','-','78',0,0,12,10)])],
})

# A suppressed attribute has no caller copy budget. This exceeds the old bridge.
large_head='<html><head class="'+'x'*70000+'"/><body><p>a</p></body></html>'
CASES['large-suppressed-attribute']=request(large_head)
def raw_reads(n):
    return ''.join(f'R {at} {min(512,n-at)} {min(512,n-at)}\n' for at in range(0,n,512))
n=len(large_head)
COMPLETE['large-suppressed-attribute']=('O Ok 0 Parsing\n'+raw_reads(n)+
 'FH 8 8\nFA 97 8 0 4\nFA 97 8 0 4\nFB 8 8\nFA 97 8 0 4\nP 0 0 1 0 0\nT 61 0 8 8 0\n'+f'S Ok {n} Done 1 1\nEND\n')
large_image='<body><img src="img.png" href="'+'x'*70000+'"/></body>'
CASES['large-unused-image-href']=image_request(large_image,100,50,width=40,height=32)
n=len(large_image)
COMPLETE['large-unused-image-href']=(f'R 0 2048 2048\n'+raw_reads(n)+'R 0 26 26\nO Ok 0 Parsing\n'+raw_reads(n)+
 f'P 0 0 0 1 0\nI 696d672e706e67 0 0 40 20\nS Ok {n} Done 1 0\nEND\n')
CASES['hyphenation-miss']=request('<body><p>zzzzz</p></body>',width=16).rstrip('\n')+' - 1 '+FIBH+'\n'

# A nonfinal justified line with a compressed closing/opening pair and spare pixels.
CASES['compressed-cjk-justification']=request('<body><p>日」「日本日本日本</p></body>',width=15,align='justify')
