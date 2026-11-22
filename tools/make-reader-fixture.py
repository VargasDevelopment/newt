#!/usr/bin/env python3
"""Generate Newt's original public-domain EPUB fixture, using only stdlib.

Fixed timestamps/order and ZIP_STORED yield identical firmware and host bytes.
This is synthetic reading material, never a user's book or device contents.
"""
from io import BytesIO
from pathlib import Path
import argparse
import hashlib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
# The test fixture and the built-in sample the firmware embeds.
OUTPUTS = (ROOT / "tests/fixtures/the-small-lantern.epub", ROOT / "rust/firmware/sample.epub")

CHAPTERS = (
    ("Morning at the Pond", (
        "Compass in hand, Mira followed the narrow path to the pond. The air was cool enough to make every breath feel new. A reed bent across the water and drew a circle that widened until it met the bank. She had come with a notebook, a sandwich, and no important appointment. That was the plan: to notice one small thing before the rest of the day began.",
        "On the nearest stone sat a newt, dark as wet bark. It did not hurry away when Mira crouched beside it. She watched its tiny toes find a comfortable place. Behind them, the first bus climbed the hill with a soft growl. The pond kept its own quieter schedule. A leaf turned slowly near the reeds, and a bright insect skated across the surface without leaving a trail.",
        "Mira opened her notebook and wrote a question: what makes a place feel familiar? The pond had changed since yesterday. There was a new branch in the water and a fresh print in the mud. Yet the leaning willow, the cracked stone, and the patient shape of the shore were still there. Perhaps familiar things leave enough room for a little surprise. She liked that answer and underlined it once.",
        "A breeze moved through the willow. Its leaves made a sound like a handful of paper being gently sorted. Mira thought of the books waiting on her shelf, each with a scrap of paper holding a place. There was comfort in knowing that a story could wait while life went on. A good bookmark was a promise that nothing would be lost just because someone needed to put the book down.",
        "She divided her sandwich into two halves and put one back in its wrapper for later. A robin landed near the path, inspected the empty space beside her boot, and decided she was not useful company. It hopped away with great purpose. Mira laughed. In the notebook she added a second answer: familiar places make room for being ordinary. Nobody at the pond expected her to do anything impressive.",
        "Before she left, Mira gave the newt a small nod. It had moved only a little, toward a patch of warmer stone. The bus returned downhill. A door somewhere in the village clicked shut. She folded the notebook around her pencil and followed the path home. The circle in the pond had disappeared, but she could still picture it spreading patiently from the reed to the shore.",
    )),
    ("A Light for the Walk Home", (
        "In the evening, the village lost its electricity for an hour. Mira found a little lantern in the cupboard and checked that it still worked. Its circle of light was modest, but enough to find a coat, a key, and the folded notebook. Outside, the houses were soft shapes against the sky. Without the usual windows shining, she could see how many stars had been there all along.",
        "At the corner, her neighbor Tomas was carrying a basket of apples. He offered her one and asked where she was going. Back to the pond, she said, just for a short walk. Tomas nodded as though this were a perfectly sensible evening errand. He took the other path toward his sister's house. Their two lights separated slowly, each making a small moving island on the road.",
        "The pond at night was not the pond of the morning. The willow had become a single dark shape. The water held the sky instead of the bank. Mira sat on the same cracked stone and left the lantern beside her feet. Somewhere among the reeds, an animal made a brief rustling sound. She did not need to know what it was to enjoy the pause that followed.",
        "She read her two answers again. The handwriting leaned a little because she had written with the notebook on her knee. A page did not have to be beautiful to be worth keeping. Still, she added a careful little drawing of the reed and its circle. Tomorrow she might remember a different detail. That was why a notebook needed more than one page, and why a story needed room to continue.",
        "Lights began returning to the village one house at a time. From the hill, they looked like seeds taking root in the dark. Mira closed the notebook and stood. The lantern was small enough to carry without thinking about it, and bright enough to show the next few steps. She did not need the whole road at once. The next safe place for her foot was quite enough.",
        "At home she put the notebook beside her current book. A scrap of paper marked the sentence where she had stopped the night before. She read a few pages, then another, while the kettle warmed. When it was time to sleep, she put the bookmark back and turned off the light. The pond, the path, and the unfinished story would be there in the morning. Lantern.",
    )),
)


def make_epub() -> bytes:
    entries = [
        ("mimetype", "application/epub+zip"),
        ("META-INF/container.xml", '<?xml version="1.0" encoding="UTF-8"?>\n<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>\n'),
        ("OEBPS/content.opf", '<?xml version="1.0" encoding="UTF-8"?>\n<package version="3.0" unique-identifier="book-id" xmlns="http://www.idpf.org/2007/opf"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="book-id">urn:newt:fixture:small-lantern:1</dc:identifier><dc:title>The Small Lantern</dc:title><dc:creator>Newt</dc:creator><dc:language>en</dc:language><meta property="dcterms:modified">2026-10-01T00:00:00Z</meta></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="one"/><itemref idref="two"/></spine></package>\n'),
        ("OEBPS/nav.xhtml", '<?xml version="1.0" encoding="UTF-8"?>\n<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="en"><head><title>Contents</title></head><body><nav epub:type="toc"><h1>Contents</h1><ol><li><a href="one.xhtml">Morning at the Pond</a></li><li><a href="two.xhtml">A Light for the Walk Home</a></li></ol></nav></body></html>\n'),
    ]
    for filename, (title, paragraphs) in zip(("one.xhtml", "two.xhtml"), CHAPTERS):
        body = "".join(f"<p>{text}</p>\n" for text in paragraphs)
        entries.append((f"OEBPS/{filename}", '<?xml version="1.0" encoding="UTF-8"?>\n<html xmlns="http://www.w3.org/1999/xhtml" lang="en"><head>' + f"<title>{title}</title></head><body><h1>{title}</h1>\n{body}</body></html>\n"))
    output = BytesIO()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_STORED) as archive:
        for name, content in entries:
            info = zipfile.ZipInfo(name, date_time=(2026, 10, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_STORED
            info.create_system = 0
            info.external_attr = 0
            archive.writestr(info, content.encode("utf-8"))
    return output.getvalue()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Verify committed bytes without modifying files")
    args = parser.parse_args()
    payload = make_epub()
    for path in OUTPUTS:
        if args.check:
            if not path.is_file() or path.read_bytes() != payload:
                raise SystemExit(f"Fixture differs: regenerate {path.relative_to(ROOT)}")
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(payload)
    print(f"Synthetic EPUB: {len(payload)} B, SHA-256 {hashlib.sha256(payload).hexdigest()}")


if __name__ == "__main__":
    main()
