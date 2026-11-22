"""XML input helpers and entity filtering, distinct from the full XML parser."""
import importlib.util
import json


def check(gate, snapshot, positive_only):
    spec = importlib.util.spec_from_file_location('xml_input_suite', snapshot / 'rust/tests/xml_input/suite.py')
    suite = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(suite)
    return suite.run(gate, snapshot, positive_only)


def verify_target(gate, artifact):
    lock = json.loads((gate.ROOT / 'rust/tests/inflate/toolchains.json').read_text())
    entry = lock['executables']['nm']
    nm = gate.ROOT / entry['path']
    if gate.digest(nm) != entry['sha256']:
        raise RuntimeError('XML target nm changed')
    output = gate.run([nm, '-S', '--defined-only', artifact], env=gate.environment()).stdout
    names = ['newt_xml_filter_borrowed', 'newt_xml_path_borrowed',
             'newt_xml_capture_borrowed', 'newt_xml_names_borrowed']
    for name in names:
        rows = [line.split() for line in output.splitlines() if line.split() and line.split()[-1] == name]
        if not any(len(f) == 4 and f[-2] == 'T' and int(f[1], 16) > 0 for f in rows):
            raise RuntimeError(f'XML nonempty borrowed target code missing: {name}')
    return names
