"""Clean export: exact deterministic snapshot, byte-level leak scan, exact exceptions.

Every leak string here is assembled at runtime, so this file never flags itself.
"""
from contextlib import redirect_stderr, redirect_stdout
import gzip
import hashlib
import io
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile
import zlib

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import clean_export

AT = "@"
HOME = "/" + "home" + "/" + "alice" + "/"
MAC = ":".join(["00", "1b", "63", "84", "45", "e6"])
LEAKS = {
    "home-path": HOME + "src/newt",
    "email": "alice" + AT + "mail" + ".org",
    "mac": MAC,
    "udev-serial": 'ATTRS{' + 'serial}=="' + "1A2B3C4D" + '"',
    "private-ipv4": ".".join(["192", "168", "7", "20"]),
    "local-host": "alice-desk" + "." + "local",
    "secret": "gh" + "p_" + "A1b2" * 9,
}
GENERIC = "\n".join([
    "/" + "home" + "/<user>/x", "user" + AT + "example" + ".com", "x" + AT + "host" + ".invalid",
    ":".join(["02", "12", "23", "34", "45", "56"]), "00" + ":00" * 5,
    'ATTRS{' + 'serial}=="MAIN_MAC"', 'ATTRS{' + 'serial}=="' + "' + mac + '" + '"',
    "version 10.2.3", "threading" + "." + "local()", "python" + AT + "3.12",
])


def value_id(text):
    """The allowlist's key for one exact matched value."""
    return "sha256:" + hashlib.sha256(text.encode()).hexdigest()


def git(repo, *args):
    return subprocess.run(["git", "-C", str(repo), "-c", "user.name=t", "-c", "user.email=t" + AT + "t.invalid", *args],
                          check=True, capture_output=True, text=True).stdout.strip()


def png_with_ztxt(text):
    def chunk(kind, body):
        return len(body).to_bytes(4, "big") + kind + body + zlib.crc32(kind + body).to_bytes(4, "big")
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", bytes(13))
            + chunk(b"zTXt", b"Author\0\0" + zlib.compress(text.encode())) + chunk(b"IEND", b""))


def epub_with(text, member="OEBPS/chapter.xhtml"):
    # Fixed timestamps keep raw-byte line numbers deterministic: a clock-time DOS byte can be a newline.
    def entry(name):
        info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
        info.compress_type = zipfile.ZIP_DEFLATED
        info.external_attr = 0o600 << 16  # what writestr(name, ...) sets
        return info
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, "w", zipfile.ZIP_DEFLATED) as archive:
        archive.writestr(entry("mimetype"), "application/epub+zip")
        archive.writestr(entry(member), "<p>\n" + text + "</p>")
    return stream.getvalue()


class CleanExportTests(unittest.TestCase):
    def setUp(self):
        self.scratch = Path(tempfile.mkdtemp(prefix="clean-export-"))
        self.addCleanup(subprocess.run, ["rm", "-rf", str(self.scratch)], check=True)
        self.repo = self.scratch / "repo"
        self.repo.mkdir()
        git(self.repo, "init", "-q")
        self.write("README.md", "Newt\n")
        self.write("tools/run.sh", "#!/bin/sh\n")
        (self.repo / "tools/run.sh").chmod(0o755)
        self.write(".gitmodules", '[submodule "vendor/sdk"]\n\tpath = vendor/sdk\n\turl = https://example.invalid/sdk.git\n')
        self.pin = "1" * 40
        (self.repo / "vendor/sdk").mkdir(parents=True)
        git(self.repo, "update-index", "--add", "--cacheinfo", f"160000,{self.pin},vendor/sdk")
        self.runs = 0

    def write(self, name, content):
        path = self.repo / name
        path.parent.mkdir(parents=True, exist_ok=True)
        (path.write_bytes if isinstance(content, bytes) else path.write_text)(content)

    def commit(self, files=None):
        for name, content in (files or {}).items():
            self.write(name, content)
        git(self.repo, "add", "-A")
        git(self.repo, "commit", "-q", "-m", "fixture")
        return git(self.repo, "rev-parse", "HEAD")

    def run_tool(self, rev="HEAD", terms=None, extra=()):
        self.runs += 1
        out = self.scratch / f"out{self.runs}"
        args = ["--repo", str(self.repo), "--rev", rev, "--out", str(out), *extra]
        if terms is not None:
            terms_file = self.scratch / "terms.txt"
            terms_file.write_text("\n".join(terms) + "\n")
            args += ["--terms", str(terms_file)]
        stdout, stderr = io.StringIO(), io.StringIO()
        with redirect_stdout(stdout), redirect_stderr(stderr):
            code = clean_export.main(args)
        return code, stdout.getvalue() + stderr.getvalue(), out

    def hits(self, output):
        return [line for line in output.splitlines() if ": " in line and not line.startswith(("export", "root commit", "  restore", "scan"))]

    def test_export_is_exact_deterministic_and_records_the_pinned_submodule(self):
        self.commit({"tools/clean-export-omit.txt": "# comment\nnotes/\tprivate working notes\n", "notes/a.md": "x\n"})
        first, output, out = self.run_tool()
        second, again, out2 = self.run_tool()
        self.assertEqual((first, second), (0, 0), output)
        self.assertEqual(output.splitlines()[0], again.splitlines()[0])
        self.assertIn(f"submodule vendor/sdk https://example.invalid/sdk.git {self.pin}", output)
        self.assertIn(f"root commit trailer: Newt-Snapshot: {git(self.repo, 'rev-parse', 'HEAD')} sha256:", output)
        names = sorted(p.relative_to(out).as_posix() for p in out.rglob("*") if p.is_file())
        self.assertEqual(names, [".gitmodules", "README.md", "tools/clean-export-omit.txt", "tools/run.sh"])
        self.assertEqual((out / "tools/run.sh").stat().st_mode & 0o777, 0o755)
        self.assertEqual([(out / n).read_bytes() for n in names], [(out2 / n).read_bytes() for n in names])
        self.commit({"README.md": "Newt changed\n"})
        self.assertNotEqual(self.run_tool()[1].splitlines()[0].split()[-1], output.splitlines()[0].split()[-1])

    def test_commit_holds_every_exported_entry_even_ignored_ones_and_the_pin(self):
        # Tracked paths the exported .gitignore matches: `git add -A` skips them (64 on main, 2026-11-05).
        self.write("firmware/app.bin", b"\x00image")
        git(self.repo, "add", "-f", "firmware/app.bin")
        self.commit({".gitignore": "*.bin\nbuild/\n", "build/keep.txt": "kept\n"})
        git(self.repo, "add", "-f", "build/keep.txt")
        git(self.repo, "commit", "-q", "-m", "ignored but tracked")
        source = git(self.repo, "rev-parse", "HEAD")
        code, output, out = self.run_tool(extra=["--commit", "Newt <newt" + AT + "example.invalid>"])
        self.assertEqual(code, 0, output)
        root = git(out, "rev-parse", "HEAD")
        self.assertEqual(git(out, "ls-tree", "-r", root), git(self.repo, "ls-tree", "-r", source))
        self.assertEqual((git(out, "rev-list", "--count", "HEAD"), git(out, "status", "--porcelain")), ("1", ""))
        self.assertIn(f"Newt-Snapshot: {source} sha256:", git(out, "log", "-1", "--format=%B"))
        self.assertEqual(git(out, "log", "-1", "--format=%an <%ae>|%cn <%ce>"),
                         "Newt <newt" + AT + "example.invalid>|Newt <newt" + AT + "example.invalid>")

    def test_lists_must_be_exact_with_reasons_and_stale_entries_fail(self):
        for omit in ("gone.md\tno longer tracked\n", "README.md\n"):
            with self.subTest(omit=omit):
                self.commit({"tools/clean-export-omit.txt": omit})
                code, output, _ = self.run_tool()
                self.assertEqual(code, 2, output)
        for allow, error in ((f"README.md\tbogus\t{value_id(MAC)}\treason\n", "unknown pattern"),
                             ("README.md\tmac\treason\n", "expected 4 tab-separated fields"),
                             ("README.md\tmac\tsha256:abc\treason\n", "the value must be sha256")):
            with self.subTest(allow=allow):
                self.commit({"tools/clean-export-omit.txt": "", "tools/clean-export-allow.txt": allow})
                self.assertIn(error, self.run_tool()[1])

    def test_every_generic_pattern_is_found_in_text_and_inside_containers(self):
        files = {f"leak/{rule}.txt": f"line one\n{text}\n" for rule, text in LEAKS.items()}
        files.update({
            "nested/log.gz": gzip.compress(f"a\nb\n{LEAKS['home-path']}\n".encode(), mtime=0),
            "nested/book.epub": epub_with(LEAKS["email"]),
            "nested/cover.png": png_with_ztxt(LEAKS["home-path"]),
            "nested/bundle.zlib": zlib.compress(LEAKS["secret"].encode(), 9),
            "nested/font.bin": b"\0\x01" + LEAKS["email"].encode("utf-16-be"),
            "clean/generic.txt": GENERIC + "\n",
        })
        self.commit(files)
        code, output, _ = self.run_tool()
        self.assertEqual(code, 1)
        expected = {f"leak/{rule}.txt:2: {rule}" for rule in LEAKS} | {
            "nested/log.gz!gunzip:3: home-path", "nested/book.epub!OEBPS/chapter.xhtml:2: email",
            "nested/cover.png!zTXt:1: home-path", "nested/bundle.zlib!zlib:1: secret",
            "nested/font.bin!utf16:1: email"}
        self.assertEqual(set(self.hits(output)), expected)
        for text in LEAKS.values():
            self.assertNotIn(text, output)

    def test_content_the_scan_cannot_read_in_full_fails_it(self):
        leak = f"a\n{LEAKS['home-path']}\n".encode()
        nested = leak
        for _ in range(4):
            nested = gzip.compress(nested, mtime=0)
        self.commit({"deep.gz": nested, "big.gz": gzip.compress(b"x" * 4096, mtime=0),
                     "two.gz": gzip.compress(b"clean\n", mtime=0) + gzip.compress(leak, mtime=0),
                     "tail.gz": gzip.compress(b"clean\n", mtime=0) + b"appended",
                     "cut.gz": gzip.compress(leak, mtime=0)[:-12]})
        with patch.object(clean_export, "MAX_INFLATED", 1024):
            code, output, _ = self.run_tool()
        self.assertEqual(code, 1)
        self.assertEqual(set(self.hits(output)), {
            "deep.gz!gunzip!gunzip!gunzip: unscanned: containers nest deeper than 3",
            "big.gz: unscanned: inflates past the 1024-byte budget",
            "two.gz!gunzip:3: home-path",
            "tail.gz: unscanned: data after the last gzip member",
            "cut.gz: unscanned: truncated compressed stream"})

    def test_generic_rules_scan_every_exported_path_and_member_name(self):
        host = LEAKS["local-host"]
        self.commit({f"assets/{host}": b"", "nested/book.epub": epub_with("clean", member=f"OEBPS/{host}")})
        code, output, _ = self.run_tool()
        self.assertEqual((code, set(self.hits(output))), (1, {
            "#3:0: local-host", "nested/book.epub!member #2:0: local-host",
            "nested/book.epub:1: local-host"}))  # zip headers carry member names in the raw bytes too

    def test_utf16_text_is_decoded_before_any_rule_or_term_matches(self):
        term = "Zo\u00eb Brindle"
        self.commit({"le.bin": b"\xff\xfe" + f"x {term} y\n".encode("utf-16-le"),
                     "be.bin": f"{term}".encode("utf-16-be"),
                     "mail.bin": b"\xfe\xff" + LEAKS["email"].encode("utf-16-be")})
        code, output, _ = self.run_tool(terms=[term])
        self.assertEqual((code, set(self.hits(output))), (1, {
            "le.bin!utf16:1: term #1", "be.bin!utf16:1: term #1", "mail.bin!utf16:1: email"}))

    def test_private_terms_are_whole_word_case_sensitive_and_never_echoed(self):
        self.commit({"docs/a.md": "Zanzibar Quokka read here.\nzanzibar quokka\nZanzibar Quokkas\n",
                     "assets/zanzibar-cover.png": b"\x89PNG"})
        code, output, _ = self.run_tool(terms=["Zanzibar Quokka", "", "zanzibar"])
        self.assertEqual(code, 1)
        self.assertEqual(sorted(self.hits(output)), ["#3:0: term #3", "docs/a.md:1: term #1", "docs/a.md:2: term #3"])
        self.assertNotIn("anzibar", output)

    def test_terms_file_exceptions_cover_one_term_in_one_file_and_go_stale(self):
        owner, other = "Zanzibar Quokka", "Pemba Kestrel"
        self.commit({"LICENSE": f"Copyright {owner}\n{other}\n", "docs/a.md": f"{owner}\n"})
        code, output, _ = self.run_tool(terms=[owner, other, f"allow\tLICENSE\t{owner}",
                                               f"allow\tdocs/gone.md\t{owner}", "allow\tLICENSE\tUnlisted Ibis"])
        self.assertEqual(code, 1)
        self.assertEqual(sorted(self.hits(output)), ["LICENSE:2: term #2", "docs/a.md:1: term #1",
                                                     "terms file line 4: allows nothing",
                                                     "terms file line 5: allows nothing"])
        for private in ("anzibar", "Kestrel", "Ibis"):
            self.assertNotIn(private, output)
        self.assertEqual(self.run_tool(terms=[owner, f"allow\tLICENSE\t{owner}", f"allow\tdocs/a.md\t{owner}"])[0], 0)
        code, output, _ = self.run_tool(terms=[owner, f"allow\tLICENSE {owner}"])
        self.assertEqual(code, 2)
        self.assertNotIn("anzibar", output)

    def test_flagged_names_and_submodule_credentials_never_reach_the_output(self):
        term, token, pin = "Zanzibar Quokka", "s3cr3t" + "-value", "2" * 40
        (self.repo / f"vendor/{term}").mkdir()
        git(self.repo, "update-index", "--add", "--cacheinfo", f"160000,{pin},vendor/{term}")
        self.commit({".gitmodules": '[submodule "vendor/sdk"]\n\tpath = vendor/sdk\n'
                                    f'\turl = https://reader:{token}' + AT + 'example.invalid/sdk.git\n'
                                    f'[submodule "q"]\n\tpath = vendor/{term}\n\turl = https://{LEAKS["local-host"]}/q.git\n',
                     f"notes/{term}.md": "clean\n", "book.epub": epub_with("clean", member=f"OEBPS/{term}.xhtml")})
        code, output, _ = self.run_tool(terms=[term])
        self.assertEqual(code, 1)
        for private in ("anzibar", token, "reader", LEAKS["local-host"]):
            self.assertNotIn(private, output)
        self.assertIn(f"submodule #1 <flagged url> {pin}", output)
        self.assertIn(f"submodule vendor/sdk https://example.invalid/sdk.git {self.pin}", output)
        self.assertLessEqual({"#4:0: term #1", "book.epub!member #2:0: term #1", ".gitmodules:5: term #1",
                              ".gitmodules:6: local-host",
                              "submodule #1:0: term #1", "submodule #1!url:0: local-host"}, set(self.hits(output)))

    def test_allowlist_is_exact_per_file_pattern_and_value_and_stale_entries_fail(self):
        other = MAC.replace("e6", "e7")
        allow = f"leak/a.txt\tmac\t{value_id(MAC)}\tsynthetic fixture\n"
        self.commit({"leak/a.txt": MAC + "\n" + LEAKS["email"] + "\n" + other + "\n", "leak/b.txt": MAC + "\n",
                     "tools/clean-export-allow.txt": allow})
        code, output, _ = self.run_tool()
        self.assertEqual((code, set(self.hits(output))),
                         (1, {"leak/a.txt:2: email", "leak/a.txt:3: mac", "leak/b.txt:1: mac"}))
        self.commit({"leak/a.txt": "clean\n", "leak/b.txt": "clean\n"})
        code, output, _ = self.run_tool()
        self.assertEqual(code, 1)
        self.assertIn("tools/clean-export-allow.txt:1: allows nothing", output)

    def test_tool_lists_and_this_test_never_flag_themselves(self):
        names = ["tools/clean_export.py", "tests/clean_export_test.py", "tools/clean-export-omit.txt",
                 "tools/clean-export-allow.txt", "tools/clean-export-emails.txt"]
        exported = [(name, (ROOT / name).read_bytes()) for name in names]
        lists = clean_export.list_rows((ROOT / clean_export.EMAIL_LIST).read_text(), 2, "emails")
        emails = {address.lower().encode() for _number, address in lists}
        self.assertEqual(clean_export.scan(exported, [], [], [], emails), ([], [], set()))


if __name__ == "__main__":
    unittest.main()
