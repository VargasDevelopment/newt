"""Bounded file-backed delivery fixture. No network, SD card, or device I/O.

Artifact publication and recipient intent are separate. A device commits one
record containing both its receipt and verified package, so a restart observes
an old or new installation, never a new receipt with old bytes. The fixture is
single-process; it does not claim cross-process queue/commit serialization.
"""
from __future__ import annotations

import base64
from dataclasses import dataclass
import json
import os
from pathlib import Path
import re
from typing import Callable

from newt_package import decode_package, package_digest

PACKAGE_BYTES = 615
MAX_REVISION = (1 << 63) - 1
_DIGEST = re.compile(r"[0-9a-f]{64}\Z")
_ALIAS = re.compile(r"[a-z][a-z0-9-]{0,31}\Z")


class DeliveryError(ValueError):
    """A fixture record or requested delivery cannot be trusted."""


class InjectedFailure(RuntimeError):
    """A deliberate interruption for restart tests."""


@dataclass(frozen=True)
class Desired:
    revision: int
    digest: str


@dataclass(frozen=True)
class Installed:
    revision: int
    digest: str
    payload: bytes


@dataclass(frozen=True)
class DeliveryResult:
    status: str
    revision: int | None = None
    digest: str | None = None


def _alias(value: str) -> str:
    if not isinstance(value, str) or not _ALIAS.fullmatch(value):
        raise DeliveryError("invalid fixture device alias")
    return value


def _digest(value: str) -> str:
    if not isinstance(value, str) or not _DIGEST.fullmatch(value):
        raise DeliveryError("invalid artifact digest")
    return value


def _revision(value: object) -> int:
    if type(value) is not int or not 1 <= value <= MAX_REVISION:
        raise DeliveryError("invalid desired revision")
    return value


def _verify(payload: bytes, digest: str | None = None) -> str:
    if not isinstance(payload, bytes) or len(payload) != PACKAGE_BYTES:
        raise DeliveryError("unexpected package size")
    decode_package(payload)
    actual = package_digest(payload)
    if digest is not None and actual != _digest(digest):
        raise DeliveryError("artifact digest mismatch")
    return actual


def _read(path: Path, limit: int) -> bytes:
    with path.open("rb") as stream:
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise DeliveryError("fixture record exceeds its limit")
    return data


def _pairs(pairs: list[tuple[str, object]]) -> dict:
    result: dict = {}
    for key, value in pairs:
        if key in result:
            raise DeliveryError("duplicate fixture record field")
        result[key] = value
    return result


def _json(data: bytes) -> dict:
    try:
        value = json.loads(data, object_pairs_hook=_pairs)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise DeliveryError("invalid fixture record") from exc
    if type(value) is not dict:
        raise DeliveryError("invalid fixture record shape")
    return value


def _encoded(value: dict) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode("ascii")


def _sync_directory(path: Path) -> None:
    descriptor = os.open(path, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def _stage(path: Path, data: bytes) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    staged = path.with_name(path.name + ".pending")
    with staged.open("wb") as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    return staged


def _commit(staged: Path, path: Path) -> None:
    os.replace(staged, path)
    _sync_directory(path.parent)


def _atomic_write(path: Path, data: bytes) -> None:
    _commit(_stage(path, data), path)


class ArtifactStore:
    """Content-addressed share artifacts, without recipient intent."""

    def __init__(self, root: Path):
        self._root = root

    def add(self, payload: bytes) -> str:
        digest = _verify(payload)
        path = self._root / (digest + ".newt")
        if path.exists():
            if self.get(digest) != payload:
                raise DeliveryError("existing artifact differs")
        else:
            _atomic_write(path, payload)
        return digest

    def get(self, digest: str) -> bytes:
        payload = _read(self._root / (_digest(digest) + ".newt"), PACKAGE_BYTES)
        _verify(payload, digest)
        return payload


class ControlPlane:
    """Desired revision and acknowledgement for each fixture recipient."""

    def __init__(self, root: Path):
        self._root = root

    def _path(self, alias: str) -> Path:
        return self._root / (_alias(alias) + ".json")

    def _record(self, alias: str) -> dict | None:
        path = self._path(alias)
        if not path.exists():
            return None
        record = _json(_read(path, 512))
        if set(record) != {"revision", "digest", "acknowledged"}:
            raise DeliveryError("invalid desired record fields")
        _revision(record["revision"])
        _digest(record["digest"])
        if type(record["acknowledged"]) is not bool:
            raise DeliveryError("invalid desired acknowledgement")
        return record

    def desired(self, alias: str) -> Desired | None:
        record = self._record(alias)
        return Desired(record["revision"], record["digest"]) if record else None

    def queue(self, alias: str, digest: str) -> Desired:
        digest = _digest(digest)
        previous = self._record(alias)
        if previous and previous["digest"] == digest:
            return Desired(previous["revision"], digest)
        revision = previous["revision"] + 1 if previous else 1
        _revision(revision)
        _atomic_write(self._path(alias), _encoded({
            "revision": revision, "digest": digest, "acknowledged": False,
        }))
        return Desired(revision, digest)

    def acknowledge(self, alias: str, desired: Desired) -> bool:
        previous = self._record(alias)
        if not previous or (previous["revision"], previous["digest"]) != (
            desired.revision, desired.digest,
        ):
            return False
        if not previous["acknowledged"]:
            previous["acknowledged"] = True
            _atomic_write(self._path(alias), _encoded(previous))
        return True

    def acknowledged(self, alias: str) -> bool:
        record = self._record(alias)
        return bool(record and record["acknowledged"])


class FixtureDeviceStorage:
    """Verified installed record; storage paths never appear in app commands."""

    def __init__(self, root: Path, alias: str):
        self.alias = _alias(alias)
        self._path = root / self.alias / "installed.json"

    def _decode(self, data: bytes) -> Installed:
        record = _json(data)
        if set(record) != {"revision", "digest", "payload"}:
            raise DeliveryError("invalid installed record fields")
        revision = _revision(record["revision"])
        digest = _digest(record["digest"])
        try:
            payload = base64.b64decode(record["payload"], validate=True)
        except (ValueError, TypeError) as exc:
            raise DeliveryError("invalid installed payload") from exc
        _verify(payload, digest)
        return Installed(revision, digest, payload)

    def installed(self) -> Installed | None:
        if not self._path.exists():
            return None
        return self._decode(_read(self._path, 1200))

    def deliver(
        self, artifacts: ArtifactStore, control: ControlPlane,
        failure_stage: str | None = None,
        hook: Callable[[str], None] | None = None,
    ) -> DeliveryResult:
        stages = {"download", "validation", "staging", "before_commit", "after_commit"}
        if failure_stage is not None and failure_stage not in stages:
            raise DeliveryError("unknown failure stage")

        def checkpoint(stage: str) -> None:
            if hook:
                hook(stage)
            if failure_stage == stage:
                raise InjectedFailure(stage)

        desired = control.desired(self.alias)
        current = self.installed()
        if desired is None:
            return DeliveryResult("idle")
        if current:
            if desired.revision < current.revision:
                raise DeliveryError("desired revision would roll back installed state")
            if desired.revision == current.revision:
                if desired.digest != current.digest:
                    raise DeliveryError("same revision identifies different artifacts")
                control.acknowledge(self.alias, desired)
                return DeliveryResult("already_installed", desired.revision, desired.digest)
        checkpoint("download")
        payload = artifacts.get(desired.digest)
        checkpoint("validation")
        _verify(payload, desired.digest)
        record = _encoded({"revision": desired.revision, "digest": desired.digest,
                           "payload": base64.b64encode(payload).decode("ascii")})
        staged = _stage(self._path, record)
        checkpoint("staging")
        # Verify bytes actually staged, not only the downloaded buffer.
        if self._decode(_read(staged, 1200)) != Installed(
            desired.revision, desired.digest, payload,
        ):
            raise DeliveryError("staged installation differs")
        checkpoint("before_commit")
        if control.desired(self.alias) != desired:
            staged.unlink(missing_ok=True)
            return DeliveryResult("superseded", desired.revision, desired.digest)
        _commit(staged, self._path)
        checkpoint("after_commit")  # Committed bytes survive a lost acknowledgement.
        control.acknowledge(self.alias, desired)
        return DeliveryResult("installed", desired.revision, desired.digest)


@dataclass(frozen=True)
class DeliveryFixture:
    artifacts: ArtifactStore
    control: ControlPlane
    _devices_root: Path

    def device(self, alias: str) -> FixtureDeviceStorage:
        return FixtureDeviceStorage(self._devices_root, alias)


def fixture(root: Path) -> DeliveryFixture:
    """Internal simulation root, chosen by tooling, never an SD mount path."""
    root = Path(root)
    return DeliveryFixture(ArtifactStore(root / "artifacts"),
                           ControlPlane(root / "control"), root / "devices")
