"""Frozen, per-stream rolling call digests; Rust still executes each call."""
import importlib.util
import hashlib
import json
from pathlib import Path
import re
from compare import fixtures
from stream_compare import script_cases

HEX64 = re.compile(r'[0-9a-f]{64}')


def requests():
    rows = []
    selected = list(fixtures())
    controls = json.loads((Path(__file__).parent / 'mutations.json').read_text())
    script_names = {name for name, _, _ in script_cases()}
    witnesses = {c['witness'] for c in controls} - script_names
    for namespace in ('32', '64', 'native'):
        mode = 64 if namespace == 'native' else int(namespace)
        if namespace != 'native':
            for name, size, calls in script_cases():
                rows.append(dict(namespace=namespace, id=f'script-{mode}-{name}', kind='script',
                    name=name, size=size, calls=[[[*p[:3], p[3].hex()], [*e[:4], e[4].hex()]] for p, e in calls]))
        for name, packed, expected, wrapped in selected:
            rings = (False,) if namespace == 'native' else (False, True)
            for ring in rings:
                chunks = [(65536,)] if namespace == 'native' else ([(1,)] if len(packed) <= 8192 or name in witnesses else []) + [(2, 3, 1, 29, 4, 257), (65536,)]
                for schedule in chunks:
                    label = 'native-' + name if namespace == 'native' else name
                    rows.append(dict(namespace=namespace, id=f'{mode}-{label}-{int(ring)}-' + ','.join(map(str, schedule)),
                        kind='stream', name=label, packed=packed.hex(), expected=None if expected is None else expected.hex(),
                        wrapped=wrapped, chunks=list(schedule), ring=ring))
    return rows


class FrozenStreams:
    def __init__(self, path):
        here = Path(__file__).parent
        spec = importlib.util.spec_from_file_location('stream_expected', here.parents[1] / 'expected.py')
        expected = importlib.util.module_from_spec(spec); spec.loader.exec_module(expected)
        corpus = requests()
        document = expected.load_document(path, corpus)
        if type(document.get('stream_format')) is not int or document['stream_format'] != 2:
            raise ValueError('unsupported frozen stream representation')
        if document['source_provenance'] != json.loads((here / 'provenance.json').read_text()):
            raise ValueError('frozen inflater source provenance changed')
        self.rows = {}
        for request, output in zip(corpus, document['outputs']):
            if (not isinstance(output, list) or len(output) != 3
                    or type(output[0]) is not int or output[0] <= 0
                    or not isinstance(output[1], str) or not HEX64.fullmatch(output[1])
                    or not isinstance(output[2], str) or not HEX64.fullmatch(output[2])):
                raise ValueError('invalid frozen stream digest row')
            self.rows[(request['namespace'], request['id'])] = tuple(output)

    def for_namespace(self, namespace):
        return Namespace(self.rows, namespace)


class Namespace:
    def __init__(self, rows, namespace):
        self.rows = rows
        self.namespace = namespace

    def stream_digest(self, header, case):
        return StreamDigest(self.rows[(self.namespace, case)], header)


class StreamDigest:
    """Rolling SHA-256 over stream header, every CALL request, and every RESULT line."""
    def __init__(self, expected, header):
        self.expected_calls, self.expected_request_sha256, self.expected_response_sha256 = expected
        self.calls = 0
        self.req_hash = hashlib.sha256(header)
        self.resp_hash = hashlib.sha256()

    def record(self, request, raw_response):
        self.calls += 1
        self.req_hash.update(hashlib.sha256(request).digest())
        self.resp_hash.update(raw_response)

    def matches(self):
        return (self.calls == self.expected_calls
                and self.req_hash.hexdigest() == self.expected_request_sha256
                and self.resp_hash.hexdigest() == self.expected_response_sha256)

    def summary(self):
        return {
            'expected': {
                'calls': self.expected_calls,
                'request_sha256': self.expected_request_sha256,
                'response_sha256': self.expected_response_sha256,
            },
            'actual': {
                'calls': self.calls,
                'request_sha256': self.req_hash.hexdigest(),
                'response_sha256': self.resp_hash.hexdigest(),
            },
        }
