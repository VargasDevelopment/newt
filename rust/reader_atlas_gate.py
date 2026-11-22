"""Use the same focused atlas recipe on the aggregate's canonical source snapshot."""
import importlib.util
import uuid


def check(gate, snapshot, positive_only):
    path = snapshot / 'rust/tests/reader_atlas/gate.py'
    spec = importlib.util.spec_from_file_location('reader_atlas_focused', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.run_gate(
        gate.ROOT, gate.SCRATCH / ('reader-atlas-' + uuid.uuid4().hex),
        source_root=snapshot / 'rust', positive_only=positive_only,
    )
