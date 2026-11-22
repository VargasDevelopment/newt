"""Fresh type-preserving decoder mutations; only clean semantic mismatches count."""
import json
from pathlib import Path
import signal
import sys
from expected_stream import FrozenStreams
from compare import fixtures
from stream_compare import (compare_stream, compare_script, script_cases,
                            ParityMismatch, stop_for_signal)


def main():
    for signum in (signal.SIGINT, signal.SIGTERM):
        signal.signal(signum, stop_for_signal)
    request = json.loads(Path(sys.argv[1]).read_text())
    frozen = FrozenStreams(request['expected'])
    request['oracles'] = {name: frozen.for_namespace(name) for name in ('32', '64', 'native')}
    root = Path(__file__).resolve().parent
    directory = Path(request['output']) / 'mutation-streams'
    directory.mkdir()
    corpus = {name: (packed, expected, wrapped) for name, packed, expected, wrapped in fixtures()}
    scripts = {name: (size, calls) for name, size, calls in script_cases()}
    controls = json.loads((root / 'mutations.json').read_text())
    source = Path(request['source']).read_text()
    rows = []
    for control in controls:
        assert source.count(control['before']) == 1, control['id']
        rust = Path(request['mutants'][control['id']])
        witness = None
        for mode in (32, 64):
            oracle = request['oracles'][str(mode)]
            try:
                if control['witness'] in scripts:
                    size, calls = scripts[control['witness']]
                    compare_script(oracle, rust, mode, control['witness'], size, calls,
                                   request['deadline'], directory)
                else:
                    packed, expected, wrapped = corpus[control['witness']]
                    for ring in (False, True):
                        for chunks in [(1,), (2, 3, 1, 29, 4, 257), (65536,)]:
                            compare_stream(oracle, rust, mode, control['witness'], packed,
                                           expected, wrapped, chunks, ring, request['deadline'], directory)
            except ParityMismatch as difference:
                witness = difference.witness
                break
        assert witness is not None, f'semantic mutant survived: {control["id"]}'
        rows.append({'id': control['id'], 'freshly_compiled': True,
                     'clean_exit_and_complete_protocol': True, 'difference': witness})
        print('DETECTED', control['id'], json.dumps(witness), flush=True)
    (Path(request['output']) / 'controls.json').write_text(json.dumps(rows, indent=2) + '\n')


if __name__ == '__main__':
    main()
