"""Read-only native observations, bound to a complete ordered request corpus."""
import importlib.util
import json
from pathlib import Path
from types import SimpleNamespace
import zlib


def expected_module():
    path = Path(__file__).resolve().parents[2] / 'expected.py'
    spec = importlib.util.spec_from_file_location('leaf_expected', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class Frozen:
    def __init__(self, path, requests, provenance=None, literal_answers=None):
        self.path = Path(path)
        expected = expected_module()
        if literal_answers is None:
            self.outputs = expected.load(self.path, requests)
        else:
            answers = expected.load_literals(self.path, requests, literal_answers)
            self.outputs = [dict(returncode=0, stdout=answer.encode().hex(), stderr='')
                            for answer in answers]
        raw = self.path.read_bytes()
        document = json.loads(zlib.decompress(raw) if self.path.suffix == '.zlib' else raw)
        if provenance is not None and document.get('source_provenance') != provenance:
            raise ValueError('Frozen native source provenance changed: ' + str(path))
        self.responses = {}
        for request, output in zip(requests, self.outputs):
            key = (request.get('mode', 0), request['input'])
            if key in self.responses and self.responses[key] != output:
                raise ValueError('Conflicting frozen responses')
            self.responses[key] = output

    def execute(self, command, cwd, wire):
        return self.text(wire)

    def text(self, wire, mode=0):
        output = self.responses[(mode, wire)]
        if output['returncode'] != 0 or output['stderr']:
            raise ValueError('Frozen successful response contains a runtime failure')
        return bytes.fromhex(output['stdout']).decode('utf-8')


def rejection_child(child, command, *, input, requests, fixture, **kwargs):
    # Always authenticate all malformed inputs, including when checking only Rust.
    wires = [wire.encode() if isinstance(wire, str) else wire for wire in requests]
    outputs = expected_module().load(fixture, [dict(input_hex=wire.hex()) for wire in wires])
    if Path(command[0]).name.startswith('expected'):
        output = outputs[wires.index(input)]
        return SimpleNamespace(returncode=output['returncode'],
                               stdout=bytes.fromhex(output['stdout']),
                               stderr=bytes.fromhex(output['stderr']))
    return child(command, input=input, **kwargs)
