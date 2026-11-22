"""Source-only corpus definition; no generation or execution at import time."""
import struct
import zlib


def enc(data):
    return data.hex() if data else "_"


def text(data):
    return "T " + enc(data)


def sheet(data, method=0, comp=None, size=None, offset=0xffffffff,
          chunk=999999, fail=-1, ret=-1, decode=1, cap=65537):
    return f"S {enc(data)} {method} {len(data) if comp is None else comp} {len(data) if size is None else size} {offset} {chunk} {fail} {ret} {decode} {cap}"


def local(payload, method=0):
    return struct.pack("<IHHHHHIIIHH", 0x04034b50, 20, 0, method, 0, 0,
                       0, len(payload), len(payload), 0, 0) + payload


def corpus():
    cases = []

    def add(name, commands, expected=()):
        cases.append({"name": name, "input": "\n".join(commands) + "\n",
                      "expected_lines": list(expected)})

    def inline(name, value, expected=()):
        add(name, ["I " + enc(value)], expected)

    baseline = "DECL 0 0 -1 -1 0 -1 -1 -1 -1 -1 -1 -1"
    inline("defaults", b"", [baseline])
    inline("all-fields", b"font-size:1.25rem;font-size:small;font-weight:600junk;font-style:oblique;text-align:end;text-indent:-4em;margin:2px 3pt 11em 1.019em;display:none;text-decoration-line:underline;vertical-align:sub",
           ["DECL 87 1 1 1 2 0 101 12 1000 1 1 2"])
    inline("percent-truncate", b"font-size:199.999%", ["DECL 199 0 -1 -1 0 -1 -1 -1 -1 -1 -1 -1"])
    inline("fraction-limit", b"font-size:1.0000em", [baseline])
    inline("weight-first-byte", b"font-weight:600garbage", ["DECL 0 0 1 -1 0 -1 -1 -1 -1 -1 -1 -1"])
    inline("negative-zero", b"margin-left:-20em", ["DECL 0 0 -1 -1 0 -1 0 -1 -1 -1 -1 -1"])
    inline("stale-root", b"font-size:2rem;font-size:medium", ["DECL 100 1 -1 -1 0 -1 -1 -1 -1 -1 -1 -1"])
    inline("reset-root", b"font-size:2rem;font-size:3em", ["DECL 300 0 -1 -1 0 -1 -1 -1 -1 -1 -1 -1"])
    # Literal field positions and values independently encode single-property outcomes.
    for name, css, field, value in [
        ("align-left", b"text-align:start", 4, 1),
        ("align-right", b"text-align:right", 4, 2),
        ("align-center", b"text-align:center", 4, 3),
        ("align-justify", b"text-align:justify", 4, 4),
        ("style-italic", b"font-style:italic", 3, 1),
        ("style-normal", b"font-style:normal", 3, 0),
        ("weight-lighter", b"font-weight:lighter", 2, 0),
        ("weight-bolder", b"font-weight:bolder", 2, 1),
        ("decoration-none", b"text-decoration-line:none", 10, 0),
        ("decoration-underline", b"text-decoration:underline", 10, 1),
        ("decoration-list-ignored", b"text-decoration:underline overline", 10, -1),
        ("vertical-super", b"vertical-align:super", 11, 1),
        ("vertical-baseline", b"vertical-align:baseline", 11, 0),
        ("display-block-ignored", b"display:block", 9, -1),
        ("display-none", b"display:none", 9, 1),
        ("indent-clamp", b"text-indent:1001%", 5, 1000),
        ("margin-left-clamp", b"margin-left:1001%", 6, 1000),
        ("margin-top-zero", b"margin-top:0", 7, 0),
        ("margin-bottom-unit", b"margin-bottom:16px", 8, 100),
        ("uppercase-property-ignored", b"FONT-SIZE:2em", 0, 0),
        ("negative-zero-size", b"font-size:-1em", 0, 0),
    ]:
        fields = [0, 0, -1, -1, 0, -1, -1, -1, -1, -1, -1, -1]
        fields[field] = value
        inline(name, css, ["DECL " + " ".join(map(str, fields))])
    for i, value in enumerate([b"xx-small", b"x-small", b"small", b"smaller", b"medium", b"large", b"larger", b"x-large", b"xx-large", b"0", b"+0", b"-0em", b".5em", b"1.em", b"1.234em", b"1.2345em", b"16px", b"12pt", b"2rem", b"400%", b"401%", b"1EM", b"1 em", b"auto", b"inherit", b"1em!important"]):
        inline(f"length-{i}", b"font-size:" + value + b";margin-left:" + value)
    for byte in [9,10,12,13,32,11,0,0xa0,0xc2]:
        inline(f"space-{byte}", bytes([byte])+b"font-size:2em"+bytes([byte]))
    css = b"/* top */ @media x{p{display:none}} @import 'a'; div > p.a, .a, p, *, #id, p:hover, [attr], p.a.b {font-size:2em;font-weight:bold} p{font-size:3em}"
    add("rightmost", ["B 512", text(css), "F", "C 70 61 0"], ["DECL 200 0 1 -1 0 -1 -1 -1 -1 -1 -1 -1"])
    add("star-rejected", ["B 512", text(b"*{display:none}"), "C 70 - 0"], [baseline])
    add("eight-classes", ["B 512", text(b".ninth{font-style:italic}.eighth{font-weight:bold}"), "C 70 " + enc(b"a b c d e f g eighth ninth") + " 0"], ["DECL 0 0 1 -1 0 -1 -1 -1 -1 -1 -1 -1"])
    add("specificity", ["B 512", text(b"p.a{font-size:2em}.a{font-size:3em}p{font-size:4em}"), "C 70 61 0"], ["DECL 200 0 -1 -1 0 -1 -1 -1 -1 -1 -1 -1"])
    add("document-order", ["B 512", text(b"p{font-size:2em}p{font-size:3em}"), "C 70 - 0"], ["DECL 300 0 -1 -1 0 -1 -1 -1 -1 -1 -1 -1"])
    add("inline-order", ["B 512", text(b"p.a{font-size:2em}"), "I " + enc(b"font-size:50%"), "C 70 61 1"], ["DECL 50 0 -1 -1 0 -1 -1 -1 -1 -1 -1 -1"])
    add("apply-sentinel", ["I " + enc(b"font-size:2rem;display:none"), "A " + enc(b"font-size:0;display:block"), "A " + enc(b"font-size:small")])
    for i in range(len(css)+1):
        add(f"truncate-{i}", ["B 512", text(css[:i]), "F", "C 70 61 0"])
    for css in [b"/*",b"/**",b"/**/p{",b"p/*x*/{font-size:2em}",b"p{font-size:/*x*/2em}",b"p{bad; font-size:2em}",b"p{font-size:2em}garbage",b"@x }p{font-size:2em}",b"@x{{}}p{font-size:2em}",b"p+q{display:none}",b"p > {display:none}",b"p{font-size:2em\0;font-weight:bold}"]:
        add("lexical-"+enc(css), ["B 512", text(css), "F", "C 70 - 0"])
    add("inert", ["B 511", text(b"p{font-size:2em}"), "F"], ["BEGIN 0", "SHEET 0 2166136261 0"])
    add("rule-cap", ["B 512", text(b"p{font-size:2em}"*512+b"p{font-size:3em}"), "F", "C 70 - 0"], ["DECL 200 0 -1 -1 0 -1 -1 -1 -1 -1 -1 -1"])
    add("ignored-hash", ["B 512", text(b"@ignored;"), "F"])
    add("rebegin", ["B 512", text(css), "B 0", "F", "B 512", "F"])
    payload = b"p{font-size:2rem;margin:1em 2em 3em 4em}"
    for chunk in [1,2,7,999999]:
        add(f"raw-{chunk}", ["B 512", sheet(payload,chunk=chunk), "F"])
        add(f"stored-{chunk}", ["B 512", sheet(local(payload),size=len(payload),comp=len(payload),offset=0,chunk=chunk), "F"])
    compressor=zlib.compressobj(wbits=-15)
    packed=compressor.compress(payload)+compressor.flush()
    for chunk in [1,2,17,999999]:
        add(f"deflate-{chunk}", ["B 512", sheet(local(packed,8),method=8,comp=len(packed),size=len(payload),offset=0,chunk=chunk), "F"])
    # Done before declared output is an accepted partial sheet, not IoError.
    add("early-deflate-done", ["B 512",sheet(local(packed,8),method=8,comp=len(packed),size=len(payload)+5,offset=0),"F"])
    for method, size, offset, decode, cap in [(0,len(payload),0xffffffff,0,0),(99,1,0xffffffff,0,0),(99,65537,0,0,0),(0,1,0,0,0),(0,1,0xffffffff,0,2),(8,len(payload),0,0,65537),(8,len(payload),0,1,0)]:
        add(f"precedence-{method}-{size}-{offset}-{decode}-{cap}", ["B 512",sheet(local(packed,8),method=method,size=size,offset=offset,decode=decode,cap=cap),"F"])
    for fail in [0,1,3,10,35]:
        for ret in [-1,0]:
            add(f"io-{fail}-{ret}",["B 512",sheet(local(payload),size=len(payload),comp=len(payload),offset=0,chunk=2,fail=fail,ret=ret),"F"])
    for cap in [0,len(payload),len(payload)+1]:
        add("text-oom-exact" if cap == len(payload) else f"text-oom-{cap}",["B 512",sheet(payload,cap=cap),"F"])
    add("raw-truncated",["B 512",sheet(payload[:-1],size=len(payload),comp=len(payload)),"F"],["STATUS 1","SHEET 0 2166136261 0"])
    add("invalid-deflate",["B 512",sheet(local(b"\xff",8),method=8,comp=1,size=20,offset=0),"F"],["STATUS 1"])
    for size in [0,65535,65536,65537]:
        add(f"sheet-cap-{size}",["B 512",sheet(b" "*size),"F"])
    add("skip-wrap",["B 512"]+[sheet(b"",size=65537)]*65536+["F"],["SHEET 0 2166136261 0"])
    add("inert-still-reads",["B 0",sheet(payload),"F"],["STATUS 0","SHEET 0 2166136261 0"])
    return cases


def rejection_corpus():
    return ["\n", "B -1\n", "B 1025\n", "T -\n", "T 0\n", "T zz\n", "F x\n", "I _ x\n", "C - - 2\n", "S _ 0 0 0 4294967295 1 -1 1 1 1\n", "S _ 0 0 0 4294967295 1 -1 -1 2 1\n"]
