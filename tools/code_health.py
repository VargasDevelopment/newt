#!/usr/bin/env python3
"""Read-only code-health checks: limits against the base commit, advisory
tool/test line counts and optional verified C3 resource evidence."""
import sys

sys.dont_write_bytecode = True
sys.pycache_prefix = None

import argparse
import json
import os
from pathlib import Path, PurePosixPath
import subprocess

ROOT = Path(__file__).resolve().parents[1]
BASELINE = "code-health-baseline.json"
POLICY = {"line_thresholds": {"test": 700, "tool": 500}}
CODE_SUFFIXES = {".c", ".cc", ".cpp", ".h", ".hpp", ".inc", ".py", ".swift"}


def exact_keys(value, keys, label):
    if not isinstance(value, dict) or set(value) != set(keys):
        raise ValueError(f"{label}: unknown or missing fields")


def unique_keys(pairs):
    # A repeated key would let the reviewed text differ from the parsed value.
    if len({key for key, _ in pairs}) != len(pairs):
        raise ValueError("Duplicate key in code-health baseline")
    return dict(pairs)


def read_baseline(root, revision=None):
    if revision:  # A committed base for adoption: its reviewed values, unvalidated.
        if revision.startswith("-"):
            raise ValueError("Adoption base must be a revision, not an option")
        text = subprocess.run(["git", "show", f"{revision}:{BASELINE}"], cwd=root, check=True,
                              capture_output=True, text=True).stdout
        return json.loads(text, object_pairs_hook=unique_keys)
    baseline = json.loads((root / BASELINE).read_text(), object_pairs_hook=unique_keys)
    exact_keys(baseline, ("version", "policy", "large_files", "resources", "floors", "records"), "baseline")
    if baseline["version"] != 2 or isinstance(baseline["version"], bool):
        raise ValueError("Unsupported code-health baseline version")
    if baseline["policy"] != POLICY:
        raise ValueError("Code-health policy drift requires a reviewed policy and baseline change")
    if not isinstance(baseline["large_files"], dict):
        raise ValueError("large_files must be a path -> line ceiling map")
    for path, ceiling in baseline["large_files"].items():
        category = classify(path)
        if (not category or ".." in PurePosixPath(path).parts
                or isinstance(ceiling, bool) or not isinstance(ceiling, int)
                or ceiling <= POLICY["line_thresholds"][category]):
            raise ValueError(f"Invalid oversized-file baseline: {path}")
    from code_health_resources import validate_budgets
    validate_budgets(baseline)
    return baseline


def base_failures(root, baseline):
    """Limits moved since each origin/main commit HEAD builds on without a fresh record."""
    from code_health_flash_policy import changes
    from release_tools import destination_bases
    try:
        bases = destination_bases(root, "HEAD")
    except ValueError as error:
        raise ValueError(f"The limit check against the base commit cannot find it: {error}") from error
    return [failure for base in bases for failure in changes(read_baseline(root, base), baseline)]


def classify(path):
    if PurePosixPath(path).suffix.lower() not in CODE_SUFFIXES:
        return None
    return {"tests": "test", "tools": "tool"}.get(PurePosixPath(path).parts[0])


def discover(root):
    result = subprocess.run(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
                            cwd=root, check=True, capture_output=True)
    files = {}
    for path in sorted(set(result.stdout.decode().split("\0")) - {""}):
        if not classify(path):
            continue
        candidate = root / path
        if candidate.is_symlink() or not candidate.resolve().is_relative_to(root.resolve()):
            raise ValueError(f"Authored code must be a regular in-repository file: {path}")
        if not candidate.exists():  # Tracked deletions are improvements.
            continue
        if not candidate.is_file():
            raise ValueError(f"Authored code is not a regular file: {path}")
        files[path] = candidate.read_text()
    return files


def length_warnings(root, baseline):
    """Files over their line guide and their reviewed size. Length is a review
    signal, not a gate: a reviewer judges the growth (docs/CODE_HEALTH.md)."""
    warnings = []
    for path, content in discover(root).items():
        lines, guide = len(content.splitlines()), POLICY["line_thresholds"][classify(path)]
        reviewed = baseline["large_files"].get(path, 0)
        if lines > max(guide, reviewed):
            size = f", reviewed size {reviewed}" if reviewed else ""
            warnings.append((path, f"{lines} lines, over the {guide}-line guide{size}; "
                                   "split it where a cohesive boundary makes the design simpler"))
    return warnings


def print_warning(path, message):
    # On GitHub Actions this workflow command annotates the file in the pull request.
    prefix = f"::warning file={path}::" if os.environ.get("GITHUB_ACTIONS") == "true" else "WARN "
    print(f"{prefix}{path}: {message}")


def verify_existing_inputs(root):
    subprocess.run([sys.executable, "-E", "-B", str(root / "tools/firmware-build.py"), "--check-config"], check=True, cwd=root)
    for generator in ("make-builtin-app.py", "make-reader-fixture.py", "make-stage-backdrop.py",
                      "pack-reader-atlas.py"):
        subprocess.run([sys.executable, "-E", "-B", str(root / "tools" / generator), "--check"], check=True, cwd=root)


def main():
    from code_health_flash_policy import VARIANTS
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--variant", choices=VARIANTS, help="also require the one build's fresh verified C3 evidence")
    parser.add_argument("--adopt", metavar="BASE", help="with --variant: bind ceilings raised since BASE to this build")
    args = parser.parse_args()
    if args.adopt and not args.variant:
        parser.error("--adopt requires --variant")
    baseline = read_baseline(ROOT)
    for path, message in length_warnings(ROOT, baseline):
        print_warning(path, message)
    failures = base_failures(ROOT, baseline)
    verify_existing_inputs(ROOT)
    if args.variant:
        from code_health_resources import check_resources
        base = read_baseline(ROOT, args.adopt) if args.adopt else None
        failures.extend(check_resources(ROOT, baseline, args.variant, base))
    if failures:
        raise ValueError("Code-health checks failed:\n" + "\n".join(failures))
    print("PASS: code-health limits" + (f" and {args.variant} resources" if args.variant else ""))
    print("ADVISORY: function complexity and duplicated decisions require full review workflow judgment; no parser score enforced")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, TypeError, KeyError, subprocess.CalledProcessError) as error:
        print(f"Code-health stopped: {error}", file=sys.stderr)
        sys.exit(1)
