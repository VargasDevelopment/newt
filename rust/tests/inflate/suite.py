"""Linear paired streams replaying pinned native call transcripts."""
import json
from pathlib import Path
import signal
import sys
from expected_stream import FrozenStreams
from compare import fixtures
from stream_compare import compare_stream, compare_script, script_cases, stop_for_signal

for signum in (signal.SIGINT, signal.SIGTERM):
    signal.signal(signum, stop_for_signal)
request = json.loads(Path(sys.argv[1]).read_text())
frozen = FrozenStreams(request['expected'])
request['oracles'] = {name: frozen.for_namespace(name) for name in ('32', '64', 'native')}
directory = Path(request['output']) / 'streams'
directory.mkdir()
selected = list(fixtures())
if request['quick']:
    selected = selected[:5]
block_types = {str(n): 0 for n in range(4)}
for _, packed, expected, wrapped in selected:
    if expected is not None:
        block_types[str((packed[2 if wrapped else 0] >> 1) & 3)] += 1
if not request['quick']:
    assert all(block_types[str(n)] for n in (0, 1, 2))
rows = []
for mode in (32, 64):
    for name, size, calls in script_cases():
        rows.append(compare_script(request['oracles'][str(mode)], request['rust'], mode,
            name, size, calls, request['deadline'], directory))
    for name, packed, expected, wrapped in selected:
        for ring in (False, True):
            for chunks in ([(1,)] if len(packed) <= 8192 else []) + [(2, 3, 1, 29, 4, 257), (65536,)]:
                rows.append(compare_stream(request['oracles'][str(mode)], request['rust'], mode,
                    name, packed, expected, wrapped, chunks, ring, request['deadline'], directory))
        print(f'PASS {mode} {name}', flush=True)
for name, packed, expected, wrapped in selected:
    rows.append(compare_stream(request['oracles']['native'], request['rust'], 64,
        'native-' + name, packed, expected, wrapped, (65536,), False, request['deadline'], directory))
(Path(request['output']) / 'positive.json').write_text(json.dumps({
    'complete_positive_corpus': not request['quick'], 'first_block_types_for_valid_fixtures': block_types,
    'byte_chunk_policy': 'packed <=8192; larger streams use heterogeneous and full chunks', 'streams': rows,
    'calls': sum(row['calls'] for row in rows)}, indent=2) + '\n')
