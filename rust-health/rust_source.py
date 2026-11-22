# SPDX-License-Identifier: MIT
"""Rust source views for rust_health: comments, strings and #[cfg(test)] code.

Views keep the source's length and newlines, so offsets and line numbers from
one view hold in every other. This is a lexer, not a parser: it knows comments,
string and char literals, bracket nesting and module declarations.
"""
from __future__ import annotations

import os
import re
from collections.abc import Iterable
from pathlib import Path

# Each alternative starts with a fixed character set, so the long numeric data
# files are skipped quickly. Prefixed strings need no word-boundary test: an
# identifier directly followed by a quote is not valid Rust.
_SPECIAL = re.compile(r'''/[/*]|[bc]r?\#*"|r\#*"|"|'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]{1,6}\}|.)|[^\\'\n])\'''')
_STRING_TAIL = re.compile(r'(?:[^"\\]|\\.)*"', re.S)
_BLOCK = re.compile(r'/\*|\*/')
_DELIMS = re.compile(r'[()\[\]{};,]')


def blank(text: str) -> str:
    return '\n'.join(' ' * len(line) for line in text.split('\n'))


def _block_end(text: str, start: int) -> int:
    depth = 0
    for match in _BLOCK.finditer(text, start):
        depth += 1 if match.group() == '/*' else -1
        if depth == 0:
            return match.end()
    return len(text)


def lex(text: str) -> tuple[str, str]:
    """Split source into a code view and a comment view; string literals are blank in both."""
    code: list[str] = []
    comments: list[str] = []
    position = 0
    while match := _SPECIAL.search(text, position):
        start, token = match.start(), match.group()
        is_comment = token in ('//', '/*')
        if token == '//':
            end = text.find('\n', start)
            end = len(text) if end < 0 else end
        elif token == '/*':
            end = _block_end(text, start)
        elif token.startswith("'"):
            end = match.end()
        elif 'r' in token:
            close = text.find('"' + '#' * token.count('#'), match.end())
            end = len(text) if close < 0 else close + 1 + token.count('#')
        else:
            tail = _STRING_TAIL.match(text, match.end())
            end = tail.end() if tail else len(text)
        between, span = text[position:start], text[start:end]
        code += [between, blank(span)]
        comments += [blank(between), span if is_comment else blank(span)]
        position = end
    code.append(text[position:])
    comments.append(blank(text[position:]))
    return ''.join(code), ''.join(comments)


# #[cfg(test)] items

_CFG = re.compile(r'#\s*\[\s*cfg\s*\(')
_ATTRIBUTE = re.compile(r'\s*#\s*!?\s*\[')
_ITEM = re.compile(r'\s*(?:pub\s*(?:\([^)]*\))?\s+)?(?:(?:unsafe|async|const|extern)\s+)*'
                   r'(?:mod|fn|impl|struct|enum|union|use|const|static|trait|type|extern|macro_rules)\b')


def _matching(code: str, opening: int) -> int:
    depth = 0
    for match in _DELIMS.finditer(code, opening):
        if match.group() in '([{':
            depth += 1
        elif match.group() in ')]}':
            depth -= 1
            if depth == 0:
                return match.start()
    return len(code) - 1


def _test_only(predicate: str) -> bool:
    predicate = re.sub(r'\s+', '', predicate)
    if predicate == 'test':
        return True
    # all(.., test, ..) is test-only; split on commas outside nested predicates.
    return (predicate.startswith('all(') and predicate.endswith(')')
            and 'test' in re.split(r',(?![^()]*\))', predicate[4:-1]))


def _item_end(code: str, start: int) -> int:
    """End of the item, field, arm or statement an attribute applies to."""
    while attribute := _ATTRIBUTE.match(code, start):
        start = _matching(code, attribute.end() - 1) + 1
    # Items ignore commas (generics, where clauses); fields and arms end at one.
    item = _ITEM.match(code, start) is not None
    depth = 0
    for match in _DELIMS.finditer(code, start):
        character = match.group()
        if character in '([{':
            depth += 1
        elif character in ')]}':
            depth -= 1
            if depth < 0:
                return match.start()
            if depth == 0 and character == '}':
                return match.end()
        elif depth == 0 and (character == ';' or (character == ',' and not item)):
            return match.end()
    return len(code)


def test_spans(code: str) -> list[tuple[int, int]]:
    spans = []
    for match in _CFG.finditer(code):
        close = _matching(code, match.end() - 1)
        if _test_only(code[match.end():close]):
            spans.append((match.start(), _item_end(code, code.index(']', close) + 1)))
    return spans


def without(code: str, spans: list[tuple[int, int]]) -> str:
    parts, position = [], 0
    for start, end in sorted(spans):
        if end > position:
            start = max(start, position)
            parts += [code[position:start], blank(code[start:end])]
            position = end
    return ''.join([*parts, code[position:]])


# Module declarations, so files declared only by #[cfg(test)] code can be skipped

_MOD = re.compile(r'\bmod\s+(\w+)\s*;')
_PATH_ATTRIBUTE = re.compile(r'#\s*\[\s*path\s*=')
_STRING_VALUE = re.compile(r'\s*"((?:[^"\\]|\\.)*)"')


def _module_file(declaring: Path, name: str, explicit: str | None) -> Path | None:
    if explicit is not None:
        return Path(os.path.normpath(declaring.parent / explicit))
    # Try both module layouts: a #[path]-loaded file owns its directory like mod.rs.
    owner = declaring.parent / declaring.stem
    bases = [declaring.parent, owner] if declaring.name in ('lib.rs', 'main.rs', 'mod.rs') else [owner, declaring.parent]
    for base in bases:
        for candidate in (base / f'{name}.rs', base / name / 'mod.rs'):
            if candidate.is_file():
                return candidate
    return None


def module_declarations(path: Path, text: str, code: str, spans: list[tuple[int, int]]) -> Iterable[tuple[Path, bool]]:
    for match in _MOD.finditer(code):
        # Attributes and visibility sit between the previous item's end and `mod`.
        prefix = max(code.rfind(end, 0, match.start()) for end in ';{}') + 1
        explicit = None
        if attribute := _PATH_ATTRIBUTE.search(code, prefix, match.start()):
            if value := _STRING_VALUE.match(text, attribute.end()):
                explicit = value.group(1)
        if target := _module_file(path, match.group(1), explicit):
            yield target, any(start <= match.start() < end for start, end in spans)
