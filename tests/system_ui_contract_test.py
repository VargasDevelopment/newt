"""Focused admission negatives; current old Library cannot claim Frosted acceptance."""
import hashlib
from pathlib import Path
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
from rust_preview import build, cli
from reading_acceptance import load
BINARY = Path(sys.argv.pop()) if len(sys.argv) == 2 else None
import system_ui_contract as ui


class UiContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.binary = BINARY or cli(build())
        cls.home = subprocess.run([str(cls.binary), '--screen', 'home'], check=True, capture_output=True).stdout
    def frames(self):
        # Positive schema fixture only; it is never called an approved renderer.
        reference = subprocess.run([str(self.binary), '--screen', 'library', '--library-fixture', 'four', '--actions', 'next'], check=True, capture_output=True).stdout
        result = {name: reference for name in ui.CASES}
        result['home'] = self.home
        for name, actions in ui.LIBRARY_NAVIGATION.items():
            frame=bytearray(reference);offset=len(frame)-528*792//8
            for active, (left, top, right, bottom) in zip(actions,ui.NAVIGATION_CELLS):
                if active: continue
                for y in range(top,bottom):
                    start=offset+y*66+left//8;end=offset+y*66+right//8
                    frame[start:end]=bytes(end-start)
            result[name]=bytes(frame)
        frame = bytearray(result['focus']); frame[len(frame)-528*792//8+300*66+20] ^= 1
        result['focus'] = bytes(frame)
        return result
    def test_schema_and_focused_pixel_negative_controls(self):
        frames = self.frames(); ui.validate_frames(frames)
        for name in ('loading', 'unavailable', 'focus3', 'shelf43'):
            bad = dict(frames); frame = bytearray(bad[name]); frame[20 + 20 * 66] ^= 1; bad[name] = bytes(frame)
            with self.assertRaises(ValueError): ui.validate_frames(bad)
        bad = dict(frames); bad['focus'] = bad['library']
        with self.assertRaises(ValueError): ui.validate_frames(bad)
        bad=dict(frames);frame=bytearray(frames['focus']);offset=len(frame)-528*792//8
        frame[offset+130*66:offset+604*66]=frames['library'][offset+130*66:offset+604*66]
        bad['focus']=bytes(frame)  # Correct navigation rail cannot substitute for visible book focus.
        with self.assertRaisesRegex(ValueError,'Focused Library entry'):
            ui.validate_frames(bad)
        bad = dict(frames); bad.pop('sparse')
        with self.assertRaises(ValueError): ui.validate_frames(bad)
        bad = dict(frames); frame = bytearray(bad['library']); frame[-66 + 40] ^= 1; bad['library'] = bytes(frame)
        with self.assertRaises(ValueError): ui.validate_frames(bad)
    def test_every_navigation_cell_rejects_wrong_actionability(self):
        frames=self.frames()
        for name,actions in ui.LIBRARY_NAVIGATION.items():
            for active,(left,top,right,bottom) in zip(actions,ui.NAVIGATION_CELLS):
                bad=dict(frames);frame=bytearray(bad[name]);offset=len(frame)-528*792//8
                if active:
                    for y in range(top,bottom):
                        start=offset+y*66+left//8;end=offset+y*66+right//8
                        frame[start:end]=bytes(end-start)
                else:
                    frame[offset+top*66+left//8] |= 0x80
                bad[name]=bytes(frame)
                with self.subTest(state=name,active=active):
                    with self.assertRaisesRegex(ValueError,'noncanonical physical button footer'):
                        ui.validate_frames(bad)
    def test_goldens_require_explicit_review_and_never_autobless_drift(self):
        frames = self.frames()
        manifest = {'schema': 1, 'review': '', 'frames': {name: hashlib.sha256(frame).hexdigest() for name, frame in frames.items()}}
        with self.assertRaises(ValueError): ui.verify_reviewed(frames, manifest)
        manifest['review'] = 'synthetic unit fixture only'
        ui.verify_reviewed(frames, manifest)
        manifest['frames']['library'] = '0' * 64
        with self.assertRaises(ValueError): ui.verify_reviewed(frames, manifest)
    def test_all_22_actual_reviewed_frames_are_required(self):
        # Losing a renderer feature must fail its golden, never reduce coverage.
        ui.verify_reviewed(ui.render_frames(self.binary), load(ROOT / 'artifacts/library-ui-review.json'))
    def reading(self, actions):
        """A Library book's actual spine, after checking the press ends on
        its open reading page (DEV STATUS screen 4, DIAG reader_open)."""
        result = subprocess.run([str(self.binary), '--screen', 'library', '--actions', actions],
                                check=True, capture_output=True)
        self.assertIn(b'STATUS screen=4 ', result.stderr)
        self.assertIn(b' reader_open=1 ', result.stderr)
        return result.stderr.split(b' actual_spine=')[1].split()[0]
    def test_external_chapters_compose_between_preview_actions(self):
        # Library's book, its menu, Chapters, a chapter: back on the page.
        self.reading('activate,activate,next,activate,activate')
    def test_queued_press_after_entering_chapters_moves_focus(self):
        # Next in Chapters moves focus, so its chapter opens another spine. The
        # preview presses only once the screen settles, so this no longer
        # covers a press queued before the repaint (issue #357).
        self.assertNotEqual(self.reading('activate,activate,next,activate,activate'),
                            self.reading('activate,activate,next,activate,next,activate'))

if __name__ == '__main__': unittest.main()
