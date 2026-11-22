"""Renderer-derived Library admission; no self-updating goldens or design verdicts."""
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess

from host_common import ROOT, png_from_pbm

# Actual Library renderer matrix; final admission rejects the old list renderer.
# Changed pixels require explicit review, never automatic baseline acceptance.
CASES = {'home': ['--screen', 'home']}
for name, fixture, actions in (
    ('library', 'four', ''), ('focus', 'four', 'next'),
    ('focus2', 'four', 'next,next'), ('focus3', 'four', 'next,next,next'),
    ('empty', 'empty', ''), ('unavailable', 'unavailable', ''),
    ('loading', 'loading', ''), ('no-covers', 'no-covers', ''),
    ('cover-fault', 'cover-fault', ''), ('long-title', 'long-title', ''),
    ('shelf43', 'shelf', ','.join(['next'] * 42)),
    ('shelf61', 'shelf', ','.join(['next'] * 60)),
    ('sparse', 'sparse', 'next,next,next,next')):
    CASES[name] = ['--screen', 'library', '--library-fixture', fixture]
    if actions:
        CASES[name] += ['--actions', actions]

LIBRARY_CASES = set(CASES) - {'home'}
CASES.update({'reader': ['--screen', 'epub'],
              'reader-menu': ['--screen', 'epub', '--actions', 'activate'],
              'reader-settings': ['--screen', 'epub', '--actions', 'activate,next,next,activate'],
              'device': ['--screen', 'device'], 'prepared-focus': None, 'open-error': None})
LIBRARY_CASES.update(('prepared-focus', 'open-error'))
# Authored fixture actions, in physical Previous/Next order. This pins the
# navigation contract as well as the canonical appearance of active cells.
LIBRARY_NAVIGATION = {
    'library': (False, True), 'focus': (True, True),
    'focus2': (True, True), 'focus3': (True, False),
    'empty': (False, False), 'unavailable': (False, False), 'loading': (False, False),
    'no-covers': (False, True), 'cover-fault': (False, True), 'long-title': (False, False),
    'shelf43': (True, True), 'shelf61': (True, False), 'sparse': (True, False),
    'prepared-focus': (True, True), 'open-error': (True, True),
}
NAVIGATION_CELLS = ((280, 748, 384, 792), (384, 748, 488, 792))



def crop(pbm, box):
    png_from_pbm(pbm)  # Validate geometry and exact packed byte count first.
    _, geometry, data = pbm.split(b'\n', 2)
    if geometry != b'528 792':
        raise ValueError('System UI must retain portrait geometry')
    x0, y0, x1, y1 = box
    return bytes(bool(data[(y * 528 + x) // 8] & (0x80 >> (x % 8)))
                 for y in range(y0, y1) for x in range(x0, x1))


def validate_frames(frames):
    if set(frames) != set(CASES):
        raise ValueError('Missing or unknown Library renderer states')
    reference = crop(frames['library'], (0, 0, 528, 64))
    for name in LIBRARY_CASES:
        frame = frames[name]
        if crop(frame, (0, 0, 528, 64)) != reference:
            raise ValueError(f'{name}: noncanonical wordmark/header pixels')
    # Count, selected title and footer changes cannot stand in for tile focus.
    if crop(frames['library'], (28, 130, 500, 604)) == crop(frames['focus'], (28, 130, 500, 604)):
        raise ValueError('Focused Library entry is not visibly distinct')
    if set(LIBRARY_NAVIGATION) != LIBRARY_CASES:
        raise ValueError('Every Library fixture must declare its navigation actions')
    # Active cells retain the exact shared glyphs/geometry; inactive cells are
    # white paper. Blank boxes and hidden actionable buttons both fail.
    for index, box in enumerate(NAVIGATION_CELLS):
        rail = crop(frames['focus'], box)
        for name in LIBRARY_CASES:
            expected = rail if LIBRARY_NAVIGATION[name][index] else bytes(len(rail))
            if crop(frames[name], box) != expected:
                raise ValueError(f'{name}: noncanonical physical button footer')


def render_frames(binary):
    frames = {}
    for name, arguments in CASES.items():
        if arguments is None:
            continue
        result = subprocess.run([str(binary), *arguments], check=True, capture_output=True)
        frames[name] = result.stdout
    # Reuse the UI child's original CC0 books and exact firmware sidecar adapter.
    # A baseline without these reviewed integration tools cannot claim readiness.
    spec = importlib.util.spec_from_file_location('library_visual_fixtures', ROOT / 'tools/library-redesign-preview.py')
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    directory = ROOT / '.build/reading-ui-fixtures'
    card, covers = directory / 'card', directory / 'covers'
    books = card / 'Newt/Books'
    helper.authored_books(books)
    helper.prepare_authored_covers(books, covers)
    # The preview's prepared-library mode scans the card as the firmware does.
    for name, damaged in (('prepared-focus', False), ('open-error', True)):
        arguments = [str(binary), 'prepared-library', str(card), str(covers), '2', *(['damaged'] if damaged else [])]
        frames[name] = subprocess.run(arguments, check=True, capture_output=True).stdout
    return frames


def verify_reviewed(frames, manifest):
    """Human-approved hashes pin rendered pixels; deliberately reviewed edits only."""
    validate_frames(frames)
    if (type(manifest) is not dict or set(manifest) != {'schema', 'review', 'frames'} or
            type(manifest['schema']) is not int or manifest['schema'] != 1 or
            type(manifest['review']) is not str or not manifest['review'].strip() or
            type(manifest['frames']) is not dict or set(manifest['frames']) != set(CASES)):
        raise ValueError('Missing explicit reviewed UI state matrix')
    for name, frame in frames.items():
        expected = manifest['frames'][name]
        if type(expected) is not str or not re.fullmatch('[0-9a-f]{64}', expected) or expected != hashlib.sha256(frame).hexdigest():
            raise ValueError(f'{name}: rendered pixels differ from reviewed UI; inspect changes')


def export_for_review(binary, output):
    """Export the preview's actual pixels; never approve or overwrite the manifest."""
    directory = Path(output)
    directory.mkdir(parents=True, exist_ok=True)
    frames = render_frames(binary)
    for name, frame in frames.items():
        (directory / (name + '.pbm')).write_bytes(frame)
        (directory / (name + '.png')).write_bytes(png_from_pbm(frame))
    (directory / 'candidate.json').write_text(json.dumps({
        'schema': 1, 'review': '',
        'frames': {name: hashlib.sha256(frame).hexdigest() for name, frame in frames.items()}}, indent=2) + '\n')
    return frames
