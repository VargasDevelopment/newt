"""Library on the real firmware built for the host, with only CC0 books.

The preview (tools/rust_preview.py) runs rust/firmware over C fakes; each
call is a new card. Fields come from the firmware's own DEV USB replies:
STATUS screen (4 reading, 6 Library, 0 Home, 3 sleeping), DIAG
library_count/library_selected, and the open book's actual_character and
current_book (its identity). These are host facts, not hardware evidence.
"""
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from gate_tier import negative_controls
from host_common import png_from_pbm
import preview_driver
import rust_preview

READING, LIBRARY, HOME, SLEEPING = "4", "6", "0", "3"


def firmware_of(binary: Path) -> Path:
    if binary.is_file():
        match = re.search(r'--firmware "([^"]+)"', binary.read_text(errors="ignore"))
        if match:
            return Path(match.group(1))
    return rust_preview.FIRMWARE


def write_opf_epub(path: Path, opf: bytes) -> None:
    with zipfile.ZipFile(preview_driver.SAMPLE) as src, zipfile.ZipFile(path, "w", zipfile.ZIP_STORED) as dst:
        for info in src.infolist():
            dst.writestr(info, opf if info.filename == "OEBPS/content.opf" else src.read(info))


def opf_entity_expansion_refusal(firmware: Path) -> None:
    """Entity-expansion OPFs take the exact existing ParseError refusal path and
    notice text on the real firmware, while a plain OPF >16 KiB still opens."""
    base_opf = zipfile.ZipFile(preview_driver.SAMPLE).read("OEBPS/content.opf")

    def run_single_book(opf: bytes):
        with tempfile.TemporaryDirectory(prefix="newt-opf-limits-") as td:
            directory = Path(td)
            preview_driver.first_boots(firmware, directory, [preview_driver.SKIP_QUICK_START])
            books = directory / "card/Newt/Books"
            books.mkdir(parents=True, exist_ok=True)
            write_opf_epub(books / "Synthetic Book.epub", opf)
            preview_driver.fresh(directory)
            preview_driver.play(firmware, directory, [preview_driver.CONFIRM, preview_driver.CONFIRM])
            frame = (directory / "frame.pbm").read_bytes()
            png_from_pbm(frame)
            fields = dict(re.findall(r"([a-z_]+)=(\d+)", (directory / "receipt.txt").read_text()))
            return frame, fields

    decl, sep, rest = base_opf.partition(b"<package")
    assert sep == b"<package"
    items = "".join(
        f'<item id="extra-{i:03d}" href="one.xhtml" media-type="application/xhtml+xml"/>'
        for i in range(300)
    ).encode()
    large_plain = base_opf.replace(b"</manifest>", items + b"</manifest>", 1)
    assert len(large_plain) > 22_000
    _, plain_fields = run_single_book(large_plain)
    assert plain_fields["screen"] == READING and plain_fields["reader_open"] == "1", plain_fields

    malformed_frame, malformed_fields = run_single_book(b"<package><")
    assert malformed_fields["screen"] == LIBRARY and malformed_fields["reader_open"] == "0", malformed_fields

    unit = b"x" * 1024
    nested_dtd = b"<!DOCTYPE package [<!ENTITY a '" + unit + b"'><!ENTITY b '" + b"&a;" * 32 + b"'>]>"
    flat_dtd = b"<!DOCTYPE package [<!ENTITY a '" + unit + b"'>]>"
    assert b"The Small Lantern" in rest and b'<item id="one"' in rest
    bombs = (
        decl + nested_dtd + sep + rest.replace(b"The Small Lantern", b"&b;" * 32, 1),
        decl + flat_dtd + sep + rest.replace(b"The Small Lantern", b"&a;" * 1024, 1),
        decl + nested_dtd + sep + rest.replace(b'<item id="one"', b'<item id="one" title="' + b"&b;" * 32 + b'"', 1),
    )
    for bomb in bombs:
        assert b"&a;" in bomb or b"&b;" in bomb
        frame, fields = run_single_book(bomb)
        assert fields["screen"] == LIBRARY and fields["reader_open"] == "0", fields
        assert frame == malformed_frame, "entity-expansion OPF must take the exact ParseError refusal screen and notice"


def previewer(binary):
    def preview(screen="library", actions=None, fixture="healthy", orientation="portrait"):
        command = [str(binary), "--screen", screen, "--library-fixture", fixture,
                   "--reader-orientation", orientation]
        if actions is not None:
            command += ["--actions", actions]
        result = subprocess.run(command, check=True, capture_output=True)
        png_from_pbm(result.stdout)
        fields = dict(re.findall(r"([a-z_]+)=(\d+)", result.stderr.decode()))
        return result.stdout, fields
    return preview


def selection_problem(binary):
    """Library opens the selected book: the second starts at its first
    character after the first was read, and the first keeps its own place."""
    preview = previewer(binary)
    _, first = preview(actions="activate,next")
    _, second = preview(actions="activate,next,back,activate,next,activate")
    # One world: read the first, open the second, then return to the first.
    _, again = preview(actions="activate,next,back,activate,next,activate,back,activate,previous,activate")
    if second["current_book"] == first["current_book"] or second["actual_character"] != "0":
        return "the second book did not open at its start"
    if again["current_book"] != first["current_book"] or again["actual_character"] != first["actual_character"]:
        return "the first book did not resume its own place"


def main():
    binary = Path(sys.argv[1]) if len(sys.argv) == 2 else rust_preview.cli(rust_preview.build())
    preview = previewer(binary)
    # Flags are fixture data, applied before any press whatever their order.
    late = subprocess.run([str(binary), "--actions", "next", "--reader-orientation", "portrait",
                           "--library-fixture", "four", "--screen", "library"], check=True, capture_output=True)
    early = subprocess.run([str(binary), "--library-fixture", "four", "--screen", "library", "--actions", "next",
                            "--reader-orientation", "portrait"], check=True, capture_output=True)
    assert (late.stdout, late.stderr) == (early.stdout, early.stderr)

    initial, fields = preview()
    assert fields["screen"] == LIBRARY and fields["library_count"] == "2" and fields["library_selected"] == "0"
    focused, fields = preview(actions="next")
    assert fields["library_selected"] == "1" and focused != initial
    assert preview(actions="previous")[0] == initial
    assert preview(actions="next,next")[0] == focused
    assert preview(actions="next,previous")[0] == initial
    assert preview(screen="home", actions="activate")[0] == initial
    problem = selection_problem(binary)
    assert problem is None, problem
    for orientation in ("portrait", "landscape-ccw"):
        for focus in ("", "next,"):
            page, fields = preview(actions=focus + "activate", orientation=orientation)
            assert fields["screen"] == READING and fields["reader_open"] == "1"
            assert fields["actual_character"] == "0"
            assert page.split(b"\n", 2)[1] == (b"528 792" if orientation == "portrait" else b"792 528")
            turned, fields = preview(actions=focus + "activate,next", orientation=orientation)
            assert turned != page and int(fields["actual_character"]) > 0
            home_page, home_fields = preview(actions=focus + "activate,next,back", orientation=orientation)
            assert home_fields["screen"] == HOME and home_fields["reader_open"] == "0"
            assert home_page != preview(screen="home", orientation=orientation)[0]
            # Home's Read resumes the book Library opened, at its own place.
            resumed, resumed_fields = preview(actions=focus + "activate,next,back,read", orientation=orientation)
            assert resumed == turned and resumed_fields["actual_character"] == fields["actual_character"]
            assert resumed_fields["current_book"] == fields["current_book"]
            sleeping, sleep_fields = preview(actions=focus + "activate,next,sleep", orientation=orientation)
            assert sleeping != turned
            awake, wake_fields = preview(actions=focus + "activate,next,sleep,wake", orientation=orientation)
            assert awake == turned and wake_fields["actual_character"] == fields["actual_character"]
    home, fields = preview(actions="back")
    assert fields["screen"] == HOME
    states = {initial}
    for fixture in ("empty", "unavailable"):
        card, fields = preview(fixture=fixture)
        assert fields["library_count"] == "0" and card not in states
        states.add(card)
        assert preview(fixture=fixture, actions="next,previous")[0] == card
        assert preview(fixture=fixture, actions="back")[1]["screen"] == HOME
    for arguments in (("--library-fixture", "private"), ("--library-fixture",)):
        rejected = subprocess.run([str(binary), *arguments], capture_output=True)
        assert rejected.returncode == 2 and not rejected.stdout
    four, fields = preview(fixture="four")
    no_covers = preview(fixture="no-covers")[0]
    cover_fault = preview(fixture="cover-fault")[0]
    assert fields["library_count"] == "4" and four != no_covers and four != cover_fault
    assert cover_fault != no_covers
    for fixture, count in (("four", 4), ("sparse", 5)):
        focused, fields = preview(fixture=fixture, actions=",".join(["next"] * (count + 3)))
        assert int(fields["library_selected"]) == count - 1
        assert preview(fixture=fixture, actions=",".join(["next"] * count + ["previous"] * count))[0] \
            == preview(fixture=fixture)[0]
    assert preview(fixture="shelf")[1]["library_count"] == "61"
    loading, fields = preview(fixture="loading")
    assert loading not in states and loading != initial
    assert preview(fixture="long-title")[1]["library_count"] == "1"
    opf_entity_expansion_refusal(firmware_of(binary))
    if negative_controls(__file__):
        # The control opens the first book whatever Library selected.
        control = rust_preview.build(edits=[("reader_image_work.rs",
            "self.open_candidate_image(home, ui, logical, &mut events, index)",
            "self.open_candidate_image(home, ui, logical, &mut events, { let _ = index; 0 })")])
        assert selection_problem(rust_preview.cli(control)) is not None, \
            "the selection check passes when Library ignores it"
    print("PASS: Library grid, focus, the selected book's open and resume, sleep/wake, and empty/fault/loading cards"
          " on the firmware host build")


if __name__ == "__main__":
    main()
