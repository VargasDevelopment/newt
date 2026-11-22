"""Independent standard-library/hand vectors and public contract facts."""
import io
import zipfile

STATUS = {'empty': (0, 0), 'directory-end-equality': (0, 0),
    'not-zip': (2, 0), 'short-not-zip': (2, 0), 'false-last-eocd-signature': (2, 0),
    'names-oom': (7, 0), 'metadata-oom': (7, 0), 'name-one-short': (7, 0),
    'name-zero': (6, 0), 'name-1025': (6, 0), 'bad-central': (2, 0),
    'central-truncated': (5, 0), 'eocd-field-10-65535': (6, 0),
    'eocd-field-12-4294967295': (6, 0), 'eocd-field-16-4294967295': (6, 0),
    'eocd-field-16-4294967294': (5, 0)}


MANDATORY_CASES = frozenset(['bad-central', 'bad-local', 'central-truncated', 'deflate-all', 'deflate-hand-stored-block', 'deflate-invalid-block', 'deflate-no-scratch', 'deflate-short-1', 'deflate-short-1023', 'deflate-short-2047', 'deflate-short-2048', 'deflate-short-7', 'deflate-truncated-block', 'directory-end-equality', 'empty', 'eocd-field-10-65535', 'eocd-field-12-4294967295', 'eocd-field-16-4294967294', 'eocd-field-16-4294967295', 'eocd-overlap-4073', 'eocd-overlap-4074', 'eocd-overlap-4075', 'eocd-overlap-65535', 'false-last-eocd-signature', 'independent-raw-failure', 'independent-raw-short', 'independent-stored-abc', 'io-call-0--7', 'io-call-0-0', 'io-call-1--7', 'io-call-1-0', 'io-call-10--7', 'io-call-10-0', 'io-call-11--7', 'io-call-11-0', 'io-call-2--7', 'io-call-2-0', 'io-call-3--7', 'io-call-3-0', 'io-call-4--7', 'io-call-4-0', 'io-call-5--7', 'io-call-5-0', 'io-call-6--7', 'io-call-6-0', 'io-call-7--7', 'io-call-7-0', 'io-call-8--7', 'io-call-8-0', 'io-call-9--7', 'io-call-9-0', 'local-name-offset-past-eof', 'metadata-oom', 'name-1024', 'name-1025', 'name-exact-capacity', 'name-one-short', 'name-zero', 'names-oom', 'not-zip', 'raw-empty', 'raw-stored-short', 'raw-truncated', 'reopen-io-failure', 'self-extracting-prefix', 'short-not-zip', 'stored-mixed-duplicate-nul', 'stored-size-disagreement', 'two-complete-eocds', 'unsupported-method-at-open'])

def fixtures(corpus):
    cases = list(corpus.cases())
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, 'w') as archive:
        info = zipfile.ZipInfo('a', (1980, 1, 1, 0, 0, 0))
        info.compress_type = zipfile.ZIP_STORED
        archive.writestr(info, b'abc')
    encoded = buffer.getvalue().hex()
    cases += [
        {'label': 'independent-stored-abc', 'input': encoded + ' 2147483647 -1 -1 2 1 0 1,2,4 -1'},
        {'label': 'independent-raw-short', 'input': '616263 2 -1 -1 0 0 0 1,4,4 3'},
        {'label': 'independent-raw-failure', 'input': '616263 2 0 -7 0 0 0 1 3'},
        {'label': 'reopen-io-failure', 'input': encoded + ' 2147483647 3 -7 2 1 0 1 -1', 'reopen': True},
    ]
    if len(cases) != 70 or {case['label'] for case in cases} != MANDATORY_CASES:
        raise RuntimeError('complete ZIP corpus of70 unique named cases required')
    if not set(STATUS).issubset({case['label'] for case in cases}):
        raise RuntimeError('independent ZIP status cases missing')
    return cases


def check(case, semantics):
    name = case['label']
    if name in STATUS:
        status, count = STATUS[name]
        assert semantics[0] == f'CAT {status} {count}', (name, semantics[0])
    if name == 'independent-stored-abc':
        assert semantics[:7] == ['CAT 0 1', 'ENTRY 0 6100 3826002220 3 3 0 0',
            'FIND 0 0 0', 'OPEN 0 0', 'OUT 0 1 1 1 61', 'OUT 0 2 2 3 6263',
            'OUT 0 4 0 3 a5a5a5a5']
    if name == 'independent-raw-short':
        assert semantics[:5] == ['RAW 3', 'OPEN 0 0', 'OUT 0 1 1 1 61',
            'OUT 0 4 2 3 6263a5a5', 'OUT 0 4 0 3 a5a5a5a5']
    if name == 'independent-raw-failure':
        row = semantics[2].split()
        assert row[:3] == ['OUT', '0', '1'] and int(row[3]) < 0 and row[4:] == ['0', 'a5']
    if name == 'reopen-io-failure':
        assert semantics[:4] == ['CAT 0 1', 'ENTRY 0 6100 3826002220 3 3 0 0',
                                'FIND 0 0 0', 'REOPEN 1 0']
