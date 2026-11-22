"""Source-only corpus definitions. Root owns execution and transport comparison.

Every record compares entire OriginalCpp/Rust stdout, including destination tails,
source calls, reader position, and arena diagnostics. `lines` supplies independent
whole-output expectations; `payload` supplies concatenated successful F bytes.
Do not count bytes from negative-return calls toward payload.
"""
def hx(b):
    return b.hex() or "-"

# Independent literal pairs, not extracted from the implementation or SDK table.
NAMED = [
    (b"aelig", "æ"), (b"agrave", "à"), (b"auml", "ä"), (b"bull", "•"),
    (b"ccedil", "ç"), (b"cent", "¢"), (b"copy", "©"), (b"dagger", "†"),
    (b"Dagger", "‡"), (b"deg", "°"), (b"divide", "÷"), (b"eacute", "é"),
    (b"egrave", "è"), (b"euro", "€"), (b"frac12", "½"), (b"frac14", "¼"),
    (b"hellip", "…"), (b"laquo", "«"), (b"ldquo", "“"), (b"lsquo", "‘"),
    (b"mdash", "—"), (b"middot", "·"), (b"minus", "−"), (b"nbsp", "\u00a0"),
    (b"ndash", "–"), (b"ntilde", "ñ"), (b"oelig", "œ"), (b"ouml", "ö"),
    (b"para", "¶"), (b"permil", "‰"), (b"plusmn", "±"), (b"pound", "£"),
    (b"prime", "′"), (b"Prime", "″"), (b"raquo", "»"), (b"rdquo", "”"),
    (b"reg", "®"), (b"rsquo", "’"), (b"sect", "§"), (b"shy", "\u00ad"),
    (b"sup2", "²"), (b"sup3", "³"), (b"szlig", "ß"), (b"times", "×"),
    (b"trade", "™"), (b"uuml", "ü"), (b"yen", "¥"),
]

def records():
    for name, replacement in NAMED:
        for chunk, cap in [(1, 1), (2, 2), (511, 7), (512, 32)]:
            yield {"input": f"F {hx(b'&'+name+b';')} {chunk} -1 -1 0 " + ",".join(["0"]+[str(cap)]*20),
                   "payload": replacement.encode()}
    repairs = [
        (b"&amp;&lt;&gt;&quot;&apos;&#12;&#xzz;&#;", b"&amp;&lt;&gt;&quot;&apos;&#12;&#xzz;&#;"),
        (b"&unknown;&;", "��".encode()),
        (b"a& b&&x", b"a&amp; b&amp;&amp;x"),
        (b"&1234567890;", "�".encode()),
        (b"&12345678901;", b"&amp;12345678901;"),
        (b"&unfinished", b"&amp;unfinished"),
        (b"&a\x00z", b"&amp;a\x00z"),
        (b"&\xff;", b"&amp;\xff;"),
        (b"&COPY;&copy;", "�©".encode()),
        (b"x"*511+b"&mdash;Z", b"x"*511+"—Z".encode()),
        (b"x"*510+b"&yen;&", b"x"*510+"¥&amp;".encode()),
    ]
    for raw, expected in repairs:
        for chunk in [1, 3, 127, 511, 512, 1024]:
            yield {"input": f"F {hx(raw)} {chunk} -1 -1 0 " + ",".join(["0"]+["3"]*200), "payload": expected}
    exact = [
        ("F 6162 1 1 -7 0 4,4,4", ["F -1 61a5a5a5 2 1", "F 1 62a5a5a5 3 2", "F 0 a5a5a5a5 3 2"]),
        ("F 6162 0 -1 -1 0 2,0,2", ["F -1 a5a5 1 0", "F 0 - 1 0", "F -1 a5a5 2 0"]),
        ("F 266d646173683b58 512 -1 -1 0 1,0,r,4", ["F 1 e2 1 8", "F 0 - 1 8", "RESET", "F 0 a5a5a5a5 1 8"]),
        ("F 2661 1 -1 -1 0 1,r,9", ["F 1 26 2 2", "RESET", "F 0 a5a5a5a5a5a5a5a5a5 2 2"]),
        ("P 2 null 78797a2366 1", ["P null 6600 2 2 4"]),
        ("P 1 null 2366 1", ["P null null 0 0 2"]),
        ("P 1 null 2366 0", ["P 00 null 1 1 0"]),
        ("P 0 null null 1", ["P null null 0 0 0"]),
        ("P 32 612f62 2e2e2f63253266642530307823253331 1", ["P 612f632f6400 25333100 10 10 0"]),
        ("D 6162632f64 3", ["D 0 a5a5a5"]),
        ("D 612f62 3", ["D 1 6100a5"]),
        ("D 2f61 1", ["D 1 00"]),
        ("L 613a623a63003a64 63", ["L 63 1"]),
        ("H 782009 -", ["H 1"]),
        ("H - -", ["H 0"]),
        ("H 780b79 79", ["H 0"]),
        ("A local 6b 613a6b 6669727374 6b 7365636f6e64", ["A 6669727374"]),
        ("A exact 6b 613a6b 6669727374 6b 7365636f6e64", ["A 7365636f6e64"]),
        ("T a200961200d t a5a t e a62", ["T 5 1 null 200961200d00", "T 5 1 61 200961000d00", "T 6 1 null 200961000d5a00", "T 6 1 61 200961000d5a00", "T 6 0 null 200961000d5a00", "T 6 0 null 200961000d5a00"]),
    ]
    for line, lines in exact:
        yield {"input": line, "lines": lines+["END"]}
    for base, href in [(b"a//b/", b"../c"), (b"a/./b", b"../../../../x"), (None,b"//a///./b/../c"), (b"x",b"%2e%2e/%23%25%zz#raw%20"), (b"x\x00/z",b"%00ignored#f"), (b"x"*511,b""), (b"x"*512,b"a"), (None,b"x"*512), (None,b"x"*511), (None,b"%"), (None,b"%a")]:
        for cap in [0,1,2,8,511,512,1024]:
            for fragment in [0,1]:
                yield {"input": f"P {cap} {'null' if base is None else hx(base)} {hx(href)} {fragment}"}
    for active in ["local", "exact"]:
        for name in [b"EncryptionMethod", b"x:EncryptionMethod", b"unrelated"]:
            for algorithms in [[],[b"http://www.idpf.org/2008/embedding"],[b"http://ns.adobe.com/pdf/enc#RC"],[b"bad"],[b"http://www.idpf.org/2008/embedding",b"bad"],[b"bad",b"http://ns.adobe.com/pdf/enc#RC"]]:
                yield {"input": " ".join(["E",active,hx(name)]+[item for alg in algorithms for item in [hx(b"x:Algorithm"),hx(alg)]])}
    for length in [0,254,255,256,511]:
        yield {"input": f"T a{hx(b'x'*length)} t e a79 b a207a20 t t n"}

# Each must be rejected (nonzero process exit), independently on both transports.
INVALID = ["", "X", "F - 1 -1 -1 0 1,", "F - 1 -1 1 0 1", "F 0 1 -1 -1 0 1",
           "P 1 null - 2", "D - -1", "A local 61 62", "H null", "T a0", "L zz -", "L - - extra"]
