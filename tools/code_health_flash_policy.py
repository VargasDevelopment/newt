"""Current resource limits, their latest records and the base-commit check; never writes a baseline.

The baseline holds each ceiling and floor with only its latest record. Git keeps
the history: `changes` compares a baseline with the one at its base commit, so a
limit moves only with a fresh record (docs/CODE_HEALTH.md "Memory policy").
Records are review metadata, not cryptographic approval or execution proof.
"""
import re

from freeink_sdk_routes import FIRMWARE_BUILD

# The one firmware build.
VARIANTS = (FIRMWARE_BUILD,)
APP_PARTITION_CAPACITY = 0x770000
# Owner-set stability floors in bytes; null means not yet derived from measured need.
FLOORS = ("reading_heap_min_free_bytes", "reading_heap_exception_min_free_bytes", "loop_stack_min_free_bytes")
RECORD = ("source_revision", "before", "after", "reason", "reviewer")


def usable_flash(capacity):
    # Integer ceil of ten percent, then ceil to one flash-sector boundary.
    reserve = ((capacity + 9) // 10 + 4095) // 4096 * 4096
    return capacity - reserve


def text(value):
    return isinstance(value, str) and bool(value.strip())


def size(value):
    """Bytes, or null for a floor not yet derived."""
    return value is None or (type(value) is int and value >= 0)


def ceilings(budgets):
    """Flash, RAM, the lent range if any and every selected stack ceiling as one flat map."""
    lent = {"wifi_lent_bytes": budgets["wifi_lent_bytes"]} if "wifi_lent_bytes" in budgets else {}
    return {"flash_bytes": budgets["flash_bytes"], "ram_bytes": budgets["ram_bytes"], **lent, **budgets["stack_bytes"]}


def limits(baseline):
    """Every ceiling and floor by name; stack keys hold '::', so names never collide."""
    return {**ceilings(baseline["resources"][FIRMWARE_BUILD]), **baseline["floors"]}


def validate_records(baseline):
    """Each limit carries exactly one record that ends at its current value."""
    floors, records = baseline.get("floors"), baseline.get("records")
    if not isinstance(floors, dict) or set(floors) != set(FLOORS) or not all(map(size, floors.values())):
        raise ValueError("Invalid memory floors")
    values = limits(baseline)
    if not isinstance(records, dict) or set(records) != set(values):
        raise ValueError("Each ceiling and floor needs exactly one record")
    for name, record in records.items():
        approval = isinstance(record, dict) and "owner_approval" in record
        if (not isinstance(record, dict) or set(record) != set(RECORD) | ({"owner_approval"} if approval else set())
                or not re.fullmatch("[0-9a-f]{40}", str(record["source_revision"]))
                or not text(record["reason"]) or not text(record["reviewer"])
                or (approval and not text(record["owner_approval"])) or not size(record["before"])):
            raise ValueError(f"Invalid limit record: {name}")
        if record["after"] != values[name]:
            raise ValueError(f"{name}: its record ends at {record['after']}, not the current {values[name]}")


# The pre-NEWT-69 stack keys that the record format renamed to their real paths.
V1_STACK_PREFIXES = {"firmware/main.cpp::": "rust/firmware/native/entry.cpp::",
                     "src/xml_budget.cpp::": "rust/target-expat/native/xml_budget.cpp::"}


def v1_limits(base):
    """A pre-NEWT-69 baseline's live limits under the current names."""
    floors = base["flash_policy"]["floors"]
    budgets = base["resources"][FIRMWARE_BUILD]
    stacks = {}
    for key, value in budgets["stack_bytes"].items():
        prefix = next((old for old in V1_STACK_PREFIXES if key.startswith(old)), None)
        stacks[V1_STACK_PREFIXES[prefix] + key[len(prefix):] if prefix else key] = value
    return {**ceilings({**budgets, "stack_bytes": stacks}),
            **(floors["updates"][-1]["after"] if floors["updates"] else floors["anchor"])}


def changes(base, head):
    """Failures for limits that moved since BASE without a fresh matching record."""
    # A pre-NEWT-69 base has no records to compare, so it may only meet the
    # migration itself: every live value unchanged. A branch still built on one
    # must move onto main before it changes a limit.
    if base.get("version") == 1:
        return [] if v1_limits(base) == limits(head) else [
            "Limits differ from a pre-record base; update your branch onto main before changing limits"]
    old, new, failures = limits(base), limits(head), []
    for name in sorted(old.keys() | new.keys()):
        if name not in new:
            failures.append(f"{name}: a limit cannot be removed")
            continue
        record, before = head["records"][name], old.get(name)
        if name in old and new[name] == before:
            if record != base["records"][name]:
                failures.append(f"{name}: its record changed but its value did not")
        elif record["before"] != before:
            failures.append(f"{name}: changed from {before} to {new[name]}, so its record must run from {before}")
        elif name in FLOORS and "owner_approval" not in record:
            failures.append(f"{name}: a floor moves only with a record naming the owner's approval")
    return failures


def adoption_failures(base, baseline, variant, report):
    """At adoption the fresh verified build must measure every ceiling raised or added since BASE.

    Later commits measure different source, so only adoption binds a record to
    its measurement."""
    old = ceilings(base["resources"][variant])
    measured = ceilings({**report, "stack_bytes": {key: frame["bytes"] for key, frame in report["stack_bytes"].items()}})
    return [f"{variant} {key}: raised ceiling {ceiling} is not the adopted measurement {measured.get(key)}"
            for key, ceiling in ceilings(baseline["resources"][variant]).items()
            if ceiling > old.get(key, -1) and ceiling != measured.get(key)]
