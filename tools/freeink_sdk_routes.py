"""The composed FreeInk SDK route and patch order for the one firmware build."""
from pathlib import Path

# The one firmware build: regional gray SDK plus the DEV USB bridge. Its key
# names the build's paths, receipts and resource budgets.
FIRMWARE_BUILD = "regional-dev-usb"
READER_VARIANTS = (FIRMWARE_BUILD,)
SDK_DEPENDENCIES = (
    ("BoardConfig", "libs/hardware/BoardConfig"),
    ("EInkDisplay", "libs/display/FreeInkDisplay"),
    ("InputManager", "libs/hardware/InputManager"),
    ("BatteryMonitor", "libs/hardware/BatteryMonitor"),
    ("XteinkDetect", "libs/hardware/XteinkDetect"),
    ("PowerManager", "libs/hardware/PowerManager"),
    ("FreeInkFont", "libs/font/FreeInkFont"),
    # The keyboard model update mode's Wi-Fi password page ports
    # (rust/device-ui/src/update/keyboard.rs).
    ("FreeInkUI", "libs/ui/FreeInkUI"),
    ("FreeInkBook", "libs/book/FreeInkBook"),
)
SDK_LIBRARIES = tuple(path for _, path in SDK_DEPENDENCIES)


def reader_sdk(variant):
    if variant not in READER_VARIANTS:
        raise ValueError(f"Unknown SDK variant: {variant}")
    return f".build/freeink-reader-sdk-{variant}"


def reader_sdk_provenance(variant):
    return reader_sdk(variant) + ".provenance.json"


def reader_patch_order(variant):
    if variant not in READER_VARIANTS:
        raise ValueError(f"Unknown SDK variant: {variant}")
    return ("regional", "producer")


def sdk_lib_deps(variant):
    root = reader_sdk(variant)
    return "\n".join(f"{name}=symlink://{root}/{path}" for name, path in SDK_DEPENDENCIES)


def require_sdk_dependency_routes(root, variant, dependencies):
    """Require every declared library's actual compiler routes to use this build's one SDK."""
    root = Path(root)
    sdk = (root / reader_sdk(variant)).resolve()
    vendor = (root / "vendor/freeink-sdk").resolve()
    build = (root / ".build").resolve()
    seen = set()
    for dependency in dependencies:
        if dependency.is_relative_to(vendor):
            raise ValueError(f"Pinned SDK dependency bypasses composed reader copy: {dependency}")
        if dependency.is_relative_to(build):
            relative = dependency.relative_to(build)
            if relative.parts and relative.parts[0].startswith("freeink-reader-sdk-") and not dependency.is_relative_to(sdk):
                raise ValueError(f"Compiler dependency uses another reader SDK lane: {dependency}")
        if dependency.is_relative_to(sdk):
            relative = dependency.relative_to(sdk).as_posix()
            matches = [library for library in SDK_LIBRARIES
                       if relative == library or relative.startswith(library + "/")]
            if len(matches) != 1:
                raise ValueError(f"Compiler dependency uses an undeclared SDK library route: {dependency}")
            seen.add(matches[0])
    if seen != set(SDK_LIBRARIES):
        missing = ", ".join(sorted(set(SDK_LIBRARIES) - seen))
        raise ValueError(f"Missing actual compiler dependency routes for reader SDK libraries: {missing}")
