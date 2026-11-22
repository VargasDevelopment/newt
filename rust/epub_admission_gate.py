"""Canonical aggregate adapter; snapshot sources, actual approved local tools."""
import importlib.util
import json
from pathlib import Path
import tempfile


def check(gate, snapshot, positive_only):
    recipe_path = snapshot / 'rust/tests/epub_admission/recipe.py'
    spec = importlib.util.spec_from_file_location('canonical_epub_recipe', recipe_path)
    recipe = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(recipe)
    receipt_root = Path(tempfile.mkdtemp(prefix='epub-admission-', dir=gate.SCRATCH))
    output = receipt_root / 'run'
    args = ['--output', str(output), '--allowance', '540']
    if positive_only:
        args.append('--positive-only')
    recipe.main(args, tool_root=gate.ROOT)
    result = json.loads((output / 'result.json').read_text())
    positives = json.loads((output / 'positive-receipts.json').read_text())
    controls = json.loads((output / 'mutation-receipts.json').read_text()) if not positive_only else []
    comparisons = sum('cpp' in row and row.get('domain', 'shared semantic domain')
                      == 'shared semantic domain' for row in positives)
    return dict(result, receipts=str(output), comparisons=comparisons,
                positive_cases=len(positives), distinct_resource_domain_cases=len(positives)-comparisons,
                semantic_controls=controls)
