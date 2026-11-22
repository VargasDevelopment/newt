"""Verify frozen inputs and reproduce the approved Home without overwriting it."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from png_tools import read_png

ROOT = Path(__file__).resolve().parent
manifest = json.loads((ROOT / 'freeze-manifest.json').read_text())
for item in manifest['files']:
    data = (ROOT / item['path']).read_bytes()
    assert hashlib.sha256(data).hexdigest() == item['sha256'], item['path']
scene = json.loads((ROOT / 'selected-scene.json').read_text())[0]
assert scene['approval']['status'] == 'owner-selected-for-implementation'
assert scene['card_treatment'] == {'radius': 4, 'stroke_width': 1, 'stroke_tone': 85}
canonical = ROOT / 'screens' / (scene['name'] + '.png')
w, h, approved = read_png(canonical)
assert (w, h) == (528, 792) and set(approved) == {0, 85, 170, 255}
build = ROOT.parents[2] / '.build'
build.mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='frosted-render-', dir=build) as temporary:
    stage = Path(temporary)
    env = dict(os.environ, CLANG_MODULE_CACHE_PATH=str(stage / 'clang-module-cache'),
               SWIFT_MODULECACHE_PATH=str(stage / 'swift-module-cache'))
    shutil.copytree(ROOT / 'assets', stage / 'assets')
    for name in ['render.swift', 'selected-scene.json']:
        shutil.copyfile(ROOT / name, stage / name)
    (stage / 'screens').mkdir()
    subprocess.run(['/usr/bin/swift', 'render.swift', 'selected-scene.json'], cwd=stage,
                   env=env, check=True)
    rw, rh, reproduced = read_png(stage / 'screens' / canonical.name)
    assert (rw, rh) == (w, h) and reproduced == approved, 'Approved pixels changed'
    assert json.loads((stage / 'text-bounds-selected.json').read_text()) == []
    metrics = json.loads((stage / 'column-metrics.json').read_text())
    assert len(metrics) == 1 and metrics[0]['left'] == 50
    assert metrics[0]['right'] <= 304 and metrics[0]['limit'] == 254
print('PASS: frozen inputs intact; approved Home reproduced pixel-for-pixel; no text overflow.')
