"""One bounded card app, encoded for Newt's trusted renderer.

The artifact contains no device or delivery state. These routines use only the
standard library; they do not run app code or read paths from an app.
"""

import hashlib
import json
import re


MAGIC = b"NEWTAPP1"
PACKAGE_BYTES = 615
MAX_SOURCE_BYTES = 8192
MAX_CARDS = 3
ACTIONS = {"continue-reading": 1, "device-info": 2, "show-note": 3}
_ACTION_NAMES = {value: key for key, value in ACTIONS.items()}
_APP_KEYS = {"id", "version", "title", "subtitle", "note", "cards"}
_CARD_KEYS = {"title", "detail", "action"}
_SLUG = re.compile(r"[a-z][a-z0-9-]{0,31}\Z")
_VERSION = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\Z")


def _text(value, limit, name, *, empty=False):
    if not isinstance(value, str):
        raise ValueError(f"{name} must be a string")
    if len(value) > limit or (not empty and not value):
        raise ValueError(f"{name} must contain {'0' if empty else '1'}–{limit} characters")
    if any(ord(char) < 32 or ord(char) > 126 for char in value):
        raise ValueError(f"{name} must use printable ASCII")
    return value


def _object(value, keys, name):
    if not isinstance(value, dict) or set(value) != keys:
        raise ValueError(f"{name} must have exactly these keys: {', '.join(sorted(keys))}")


def _validate(app):
    _object(app, _APP_KEYS, "app")
    identifier = _text(app["id"], 32, "id")
    if not _SLUG.fullmatch(identifier):
        raise ValueError("id must be a lowercase slug beginning with a letter")
    version = _text(app["version"], 16, "version")
    if not _VERSION.fullmatch(version):
        raise ValueError("version must be numeric x.y.z without leading zeroes")
    title = _text(app["title"], 32, "title")
    subtitle = _text(app["subtitle"], 64, "subtitle", empty=True)
    note = _text(app["note"], 160, "note", empty=True)
    cards = app["cards"]
    if not isinstance(cards, list) or not 1 <= len(cards) <= MAX_CARDS:
        raise ValueError("cards must be a list containing 1–3 cards")
    normalized_cards = []
    for index, card in enumerate(cards):
        _object(card, _CARD_KEYS, f"card {index + 1}")
        card_title = _text(card["title"], 32, "card title")
        detail = _text(card["detail"], 64, "card detail")
        action = _text(card["action"], 32, "card action")
        if action not in ACTIONS:
            raise ValueError("unknown card action")
        normalized_cards.append({"title": card_title, "detail": detail, "action": action})
    return {"id": identifier, "version": version, "title": title,
            "subtitle": subtitle, "note": note, "cards": normalized_cards}


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def _bytes(data):
    if not isinstance(data, bytes):
        raise ValueError("input must be bytes")
    return data


def validate_source(data: bytes) -> dict:
    """Validate the agent-authored JSON and return its concrete app data."""
    _bytes(data)
    if len(data) > MAX_SOURCE_BYTES:
        raise ValueError("app source exceeds 8 KiB")
    try:
        app = json.loads(data.decode("utf-8"), object_pairs_hook=_unique_object)
    except (UnicodeDecodeError, json.JSONDecodeError, RecursionError) as error:
        raise ValueError("invalid app JSON") from error
    return _validate(app)


def _field(value, width):
    encoded = value.encode("ascii")
    return encoded + bytes(width - len(encoded))


def encode_app(app: dict) -> bytes:
    """Produce the single canonical representation of a valid app."""
    app = _validate(app)
    result = bytearray(MAGIC)
    for key, width in (("id", 33), ("version", 17), ("title", 33),
                       ("subtitle", 65), ("note", 161)):
        result.extend(_field(app[key], width))
    result.append(len(app["cards"]))
    for card in app["cards"]:
        result.extend(_field(card["title"], 33))
        result.extend(_field(card["detail"], 65))
        result.append(ACTIONS[card["action"]])
    result.extend(bytes((MAX_CARDS - len(app["cards"])) * 99))
    return bytes(result)


def decode_package(data: bytes) -> dict:
    """Reject malformed, noncanonical or unsupported package bytes."""
    _bytes(data)
    if len(data) != PACKAGE_BYTES or data[:8] != MAGIC:
        raise ValueError("expected a 615-byte NEWTAPP1 package")
    offset = 8

    def field(width):
        nonlocal offset
        raw = data[offset:offset + width]
        offset += width
        terminator = raw.find(b"\0")
        if terminator < 0 or any(raw[terminator:]):
            raise ValueError("string must have a NUL terminator and zero padding")
        try:
            return raw[:terminator].decode("ascii")
        except UnicodeDecodeError as error:
            raise ValueError("string must use printable ASCII") from error

    app = {}
    for key, width in (("id", 33), ("version", 17), ("title", 33),
                       ("subtitle", 65), ("note", 161)):
        app[key] = field(width)
    card_count = data[offset]
    offset += 1
    if not 1 <= card_count <= MAX_CARDS:
        raise ValueError("package must contain 1–3 cards")
    cards = []
    for _ in range(card_count):
        title = field(33)
        detail = field(65)
        action = _ACTION_NAMES.get(data[offset])
        offset += 1
        if action is None:
            raise ValueError("unknown card action")
        cards.append({"title": title, "detail": detail, "action": action})
    if any(data[offset:]):
        raise ValueError("unused card records must be zero")
    app["cards"] = cards
    return _validate(app)


def package_digest(data: bytes) -> str:
    """Content identity of the artifact, independent of any recipient queue."""
    return hashlib.sha256(_bytes(data)).hexdigest()
