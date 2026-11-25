"""Independent archive construction and semantic expectations; no candidate imports.
Not executed by this source-only task. Raw deflate fixture bytes are constants.
"""
import struct

def archive(members):
    body=bytearray(); central=bytearray()
    for name,payload,method,declared,flags in members:
        name=name.encode();offset=len(body)
        body+=struct.pack('<IHHHHHIIIHH',0x04034b50,20,flags,method,0,0,0,len(payload),declared,len(name),0)+name+payload
        central+=struct.pack('<IHHHHHHIIIHHHHHII',0x02014b50,20,20,flags,method,0,0,0,len(payload),declared,len(name),0,0,0,0,0,offset)+name
    return bytes(body+central+struct.pack('<IHHHHIIH',0x06054b50,0,0,len(members),len(members),len(central),len(body),0))
def stored(name,text):
    data=text.encode();return name,data,0,len(data),0
CONTAINER='<container><rootfiles><rootfile full-path="OPS/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>'
OPF='<package><metadata><title>Example</title></metadata><manifest><item id="c" href="chapter.xhtml" media-type="application/xhtml+xml"/><item id="n" href="nav.xhtml" properties="nav" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>'
BASE=[stored('META-INF/container.xml',CONTAINER),stored('OPS/book.opf',OPF),stored('OPS/chapter.xhtml','<html/>'),stored('OPS/nav.xhtml','not XML, still expanded only')]
# Nav XML validity deliberately does not affect Stage 1 acceptance.
CASES=[
 {'id':'valid-nav-not-parsed','members':BASE,'status':'Ok'},
 {'id':'missing-container','members':BASE[1:],'status':'NotEpub'},
 {'id':'missing-spine-member','members':BASE[:2]+BASE[3:],'status':'NotFound'},
 {'id':'missing-nav-optional','members':BASE[:3],'status':'Ok'},
 {'id':'duplicate-before-required','members':BASE+[BASE[0]],'status':'Unsupported','member_begins':[]},
 {'id':'malformed-encryption-maps-encrypted','members':[stored('META-INF/encryption.xml','<')]+BASE,'status':'Encrypted'},
 {'id':'permitted-obfuscation','members':[stored('META-INF/encryption.xml','<x><EncryptionMethod Algorithm="http://www.idpf.org/2008/embedding"/></x>')]+BASE,'status':'Ok'},
 {'id':'content-encryption','members':[stored('META-INF/encryption.xml','<x><EncryptionMethod Algorithm="urn:locked"/></x>')]+BASE,'status':'Encrypted'},
 {'id':'central-encryption','members':[(BASE[0][0],BASE[0][1],0,BASE[0][3],1)]+BASE[1:],'status':'Encrypted'},
 {'id':'required-bad-deflate','members':[('META-INF/container.xml',b'\x07',8,1,0)]+BASE[1:],'status':'IoError','member_begins':[0]},
 {'id':'required-short-deflate','members':[('META-INF/container.xml',b'\x03\x00',8,1,0)]+BASE[1:],'status':'Truncated','member_begins':[0]},
 {'id':'required-excess-deflate','members':[('META-INF/container.xml',b'\x73\x04\x00',8,0,0)]+BASE[1:],'status':'Unsupported','member_begins':[0]},
 {'id':'declared-budget-before-flags','members':[('x',b'',0,4194305,1)],'status':'Unsupported'},
 {'id':'invalid-xml-container','members':[stored('META-INF/container.xml','<')]+BASE[1:],'status':'ParseError'},
 {'id':'short-read-one','members':BASE,'status':'Ok','max_read':1},
 {'id':'first-read-failure','members':BASE,'status':'IoError','fail_call':0},
 {'id':'no-byte-scratch-cache-fallback','members':BASE,'status':'Ok','parse_bytes':0,'domain':'Rust typed scratch supplied; C++ requires separate XML scratch and is not byte-cap equivalent'},
]
# Admission events: InspectBegin/End, CatalogBegin/End, one CompareBegin/End
# pair for the whole duplicate-name heapsort (its comparison total), then
# required members 0,1,3. OPF parses twice. Chapter not expanded.
VALID_REQUIRED=[0,1,3]
VALID_COMPARE_COUNTS=[10]
FAILURE_MATRIX={
 'scan_read_failure':'IoError; InspectEnd only, no catalog; both arenas reset',
 'no_entry_slots':'OutOfMemory; CatalogEnd failure; no CompareBegin',
 'name_arena_exhaustion':'OutOfMemory; catalog keeps partial allocation until admit resets',
 'no_chunk':'OutOfMemory; MemberBegin/End, no StreamBegin',
 'no_decoder_for_deflate':'OutOfMemory; MemberBegin/End, no StreamBegin',
 'no_xml_buffer':'OutOfMemory after successful required expansion; encryption maps to Encrypted',
 'backend_create_failure':'OutOfMemory after expansion; encryption maps to Encrypted',
 'zero_before_eof':'scheduled read returns -1, no yield; IoError or parser-specific failure mapping',
 'chapter_extra_output':'read stops at declared length; finish false',
 'chapter_wrong_offset':'-1 without consuming output or yielding',
}
