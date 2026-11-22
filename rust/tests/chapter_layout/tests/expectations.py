"""Authored expectations; never populated from a producer response."""
from dataclasses import dataclass
from typing import Literal
from cases import CASES, COMPLETE, PRODUCER, request
from protocol import parse_reply

@dataclass(frozen=True)
class Witness:
    name: str
    wire: str
    cpp: str
    rust: str
    family: Literal['ordinary','producer','plain','resource'] = 'ordinary'
    def validate(self):
        parse_reply(self.cpp,self.wire)
        parse_reply(self.rust,self.wire)

def plain(data, read=4096, fail=-1, **kw):
    return 'T'+request(data,read=read,fail=fail,**kw)[1:]

# Plain readAt requests 4096 even when fewer bytes remain, unlike the XML reader.
def plain_a(name, data, reads):
    wire=plain(data)
    trace=('O Ok 0 Idle\n'+reads+
      'FH 8 8\nFA 97 8 0 4\nFA 97 8 0 4\nFB 8 8\nFA 97 8 0 4\n'
      'P 0 0 1 0 0\nT 61 0 8 8 0\nS Ok 0 Done 1 1\nEND\n')
    return Witness(name,wire,trace,trace,'plain')

WITNESSES=[Witness(k,CASES[k],v,v) for k,v in COMPLETE.items()]
WITNESSES=[Witness(w.name,w.wire,w.cpp.replace("R 0 26 26\n","B 0 26 26\n"),w.rust.replace("R 0 26 26\n","B 0 26 26\n"),w.family) for w in WITNESSES]
WITNESSES += [plain_a('plain-a','a','R 0 4096 1\nR 1 4096 0\n'),
              plain_a('plain-bom','\ufeffa','R 0 4096 4\nR 4 4096 0\n')]
for name, data, fail in [('plain-empty','',-1),('plain-io','a',0)]:
    raw='O Ok 0 Idle\nR 0 4096 '+('0' if fail<0 else '-1')+'\nS '+('Ok' if fail<0 else 'IoError')+' 0 Done 0 0\nEND\n'
    WITNESSES.append(Witness(name,plain(data,fail=fail),raw,raw,'plain'))
# Finite zero does not feed main XML. No image catalog means prescan does no reads.
n=len('<body><p>a</p></body>')
raw='Q 0 0\nO Ok 0 Parsing\nS Ok 0 Parsing 0 0\nEND\n'
WITNESSES.append(Witness('producer-zero','X'+PRODUCER['zero-feeds'][1:],raw,raw,'producer'))
# Differential extensions supplement independent witnesses; never called independent.
DIFFERENTIAL={**CASES,**{k:'X'+v[1:] for k,v in PRODUCER.items()}}
DIFFERENTIAL.update({
 'plain-split-bom':plain('\ufeffa',read=1),
 'plain-crlf-chunks':plain('a\r\n\r\nb',read=2),
 'plain-stop':plain('aa bb cc dd ee ff',width=8,height=8,stop=1,read=3),
 'plain-late-io':plain('a\n\nb',read=2,fail=1),
})

for name,data,chunk in [('plain-whitespace-crlf','\r\n\r\n',1),('plain-whitespace-chunk','  ',1)]:
    reads=''.join(f'R {i} 4096 1\n' for i in range(len(data)))+f'R {len(data)} 4096 0\n'
    raw='O Ok 0 Idle\n'+reads+'S Ok 0 Done 0 0\nEND\n'
    WITNESSES.append(Witness(name,plain(data,read=chunk),raw,raw,'plain'))
# Empty BOM's first-read-only behavior: one chunk discards it; splitting retains
# U+FEFF and must be compared as a differential extension until independently reviewed.
raw='O Ok 0 Idle\nR 0 4096 3\nR 3 4096 0\nS Ok 0 Done 0 0\nEND\n'
WITNESSES.append(Witness('plain-bom-only',plain('\ufeff'),raw,raw,'plain'))

def producer_case(name,text,tail,cancel=-1,fail=-1,max_feeds=4294967295,calls=1):
    wire='X'+request(text,fail=fail,max_feeds=max_feeds,calls=calls)[1:].rstrip('\n')+f' - 1 - - {cancel}\n'
    WITNESSES.append(Witness(name,wire,tail,tail,'producer'))

text="<!DOCTYPE body [<!ENTITY x 'v'>]><body>&x;</body>"
n=len(text)
producer_case('chapter-internal-dtd',text,
    f'Q 0 0\nO Ok 0 Parsing\nQ 1 0\nR 0 {n} {n}\nQ 2 0\nS ParseError 0 Failed 0 0\nEND\n')
producer_case('chapter-cancel-open','<body/>',
    'Q 0 1\nO IoError 0 Failed\nS Unsupported 0 Failed 0 0\nEND\n',cancel=0)
producer_case('chapter-read-before-cancel','<body/>',
    'Q 0 0\nO Ok 0 Parsing\nQ 1 0\nR 0 7 -1\nS IoError 0 Failed 0 0\nEND\n',cancel=2,fail=0)
text='<body><p>a</broken>'
n=len(text)
producer_case('chapter-parse-before-cancel',text,
    f'Q 0 0\nO Ok 0 Parsing\nQ 1 0\nR 0 {n} {n}\nQ 2 0\nS ParseError 0 Failed 0 0\nEND\n',cancel=3)
producer_case('chapter-post-parse-cancel','<body/>',
    'Q 0 0\nO Ok 0 Parsing\nQ 1 0\nR 0 7 7\nQ 2 0\nQ 3 1\nS IoError 0 Failed 0 0\nEND\n',cancel=3)
