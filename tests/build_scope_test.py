"""Ensure normal config edits cannot acquire tools outside the approved bundle."""
import importlib.util
from contextlib import ExitStack, redirect_stderr
import hashlib
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
spec = importlib.util.spec_from_file_location("firmware_build", ROOT / "tools/firmware-build.py")
build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)
import daily_firmware


def verify_relocated_driver() -> None:
    with tempfile.TemporaryDirectory() as directory:
        venv = Path(directory) / "penv"
        subprocess.run([sys.executable, "-m", "venv", "--without-pip", str(venv)], check=True)
        interpreter = venv / "bin/python"
        site = Path(subprocess.check_output(
            [str(interpreter), "-I", "-B", "-c", "import sysconfig; print(sysconfig.get_path('purelib'))"], text=True).strip())
        (site / "local_build_driver.py").write_text("ORIGIN = 'isolated-venv'\n")
        ambient = Path(directory) / "ambient"
        ambient.mkdir()
        (ambient / "local_build_driver.py").write_text("ORIGIN = 'ambient-unapproved'\n")
        pio = venv / "bin/pio"
        pio.write_text("#!/unavailable/old-checkout/penv/bin/python\n"
                       "import local_build_driver, sys\n"
                       "assert sys.argv[1:] == ['run', '-e', 'newt_x3', '--jobs', '2']\n"
                       "print(local_build_driver.ORIGIN)\n")
        command = build.build_command(pio)
        env = dict(os.environ, PYTHONPATH=str(ambient))
        assert subprocess.check_output(command, text=True, env=env).strip() == "isolated-venv"
        interpreter.unlink()
        try:
            build.build_command(pio)
        except RuntimeError:
            pass
        else:
            raise AssertionError("Missing local interpreter accepted")


# The source identity the driver captures before compiling, and its commit date.
REVISION, COMMIT_DATE = "0123456789ab" + "0" * 28, "2026-10-27"


def run_native_main(driver, project, arguments):
    calls = []

    def run(command, **kwargs):
        calls.append(command)
        if str(project / 'tools/crt_bundle.py') in command:
            bundle = project / '.build/regional-dev-usb/crt/x509_crt_bundle'
            bundle.parent.mkdir(parents=True, exist_ok=True)
            bundle.write_bytes(b'fixed certificate fixture')
        if command[:2] == ["git", "show"]:
            # About's date is the captured revision's, never HEAD's or the clock's.
            assert command == ["git", "show", "-s", "--format=%cs", REVISION], command
            return subprocess.CompletedProcess(command, 0, stdout=COMMIT_DATE + "\n")
        output = "0123456789ab\n" if command[:2] == ["git", "rev-parse"] else ""
        return subprocess.CompletedProcess(command, 0, stdout=output)

    rejected = False
    with ExitStack() as context:
        context.enter_context(patch.object(driver, "ROOT", project))
        context.enter_context(patch.object(driver, "TOOLCHAIN", project / ".toolchain"))
        context.enter_context(patch.object(driver, "require_toolchain", return_value=project / "mock-pio"))
        context.enter_context(patch.object(driver, "build_command", return_value=["mock-pio"]))
        context.enter_context(patch.object(daily_firmware, 'CRT_BUNDLE_SHA256',
                                          hashlib.sha256(b'fixed certificate fixture').hexdigest()))
        context.enter_context(patch.object(driver, "source_identity", return_value={
            "firmware_version": "0.1.0-alpha.0", "git_revision": REVISION, "dirty": False}))
        context.enter_context(patch.object(driver.subprocess, "run", side_effect=run))
        context.enter_context(patch.object(sys, "argv", ["firmware-build.py", *arguments]))
        try:
            driver.main()
        except (RuntimeError, OSError):
            rejected = True
    return rejected, calls


# The one build's path key, authored here rather than taken from the driver.
BUILD = "regional-dev-usb"


def verify_generated_config_boundary(driver=build) -> int:
    cases = [(level, target) for level in ("build-root", "variant", "config-file")
             for target in ("outside", "inside", "broken")]
    cases += [("project-root", "inside"), ("root-parent", "inside"),
              ("hardlink", "outside"), ("hardlink", "inside"),
              ("build-root", "nonregular"), ("variant", "nonregular"), ("config-file", "nonregular"),
              ("valid", "new"), ("valid", "existing")]
    count = 0
    variant = BUILD
    for level, target_kind in cases:
        with tempfile.TemporaryDirectory() as directory:
            sandbox = Path(directory).resolve()
            project = sandbox / "project"
            project.mkdir()
            (project / daily_firmware.SOURCE_CONFIGURATION).write_bytes(
                (ROOT / daily_firmware.SOURCE_CONFIGURATION).read_bytes())
            generated = project / ".build" / variant / "platformio.ini"
            target = (sandbox if target_kind == "outside" else project) / "alias-target"
            protected = None
            if level in ("build-root", "variant", "config-file"):
                alias = {"build-root": project / ".build", "variant": generated.parent,
                         "config-file": generated}[level]
                alias.parent.mkdir(parents=True, exist_ok=True)
                if target_kind == "nonregular":
                    if level == "config-file":
                        alias.mkdir()
                    else:
                        alias.write_text("regular file cannot be a configuration directory\n")
                elif target_kind != "broken":
                    if level == "config-file":
                        protected = target
                    else:
                        target.mkdir()
                        protected = target / variant / "platformio.ini" if level == "build-root" else target / "platformio.ini"
                        protected.parent.mkdir(parents=True, exist_ok=True)
                    protected.write_bytes(b"existing configuration must survive rejected aliases\n")
                if target_kind != "nonregular":
                    alias.symlink_to(target, target_is_directory=level != "config-file")
            elif level == "hardlink":
                protected = target
                protected.write_bytes(b"existing configuration must survive rejected aliases\n")
                generated.parent.mkdir(parents=True)
                generated.hardlink_to(target)
            elif level == "project-root":
                alias = sandbox / "root-alias"
                alias.symlink_to(project, target_is_directory=True)
                project = alias
            elif level == "root-parent":
                alias = sandbox / "parent-alias"
                alias.symlink_to(sandbox, target_is_directory=True)
                project = alias / "project"
            elif target_kind == "existing":
                generated.parent.mkdir(parents=True)
                generated.write_text("old generated configuration\n")
            rejected, calls = run_native_main(driver, project, [])
            label = f"{variant}: {level}/{target_kind}"
            if level == "valid":
                assert not rejected, label
                with patch.object(daily_firmware, 'CRT_BUNDLE_SHA256',
                                  hashlib.sha256(b'fixed certificate fixture').hexdigest()):
                    daily_firmware.require_generated_configuration(project, generated)
                assert f'-DNEWT_DEV_BUILD_DATE=\\"{COMMIT_DATE}\\"' in generated.read_text(), label
                assert ["mock-pio", "--project-conf", str(generated)] in calls, label
            else:
                if protected is not None:
                    assert protected.read_bytes() == b"existing configuration must survive rejected aliases\n", label
                assert rejected, f"Generated configuration alias accepted: {label}"
                assert not any(command[0] == "mock-pio" for command in calls), label
                if target_kind == "broken":
                    assert not target.exists(), label
            count += 1
    return count


def verify_native_directory_boundary(driver=build) -> int:
    mutable_toolchain = (".toolchain", ".toolchain/platformio", ".toolchain/cache",
                         ".toolchain/cache/platformio", ".toolchain/cache/pip",
                         ".toolchain/cache/uv", ".toolchain/cache/xdg", ".toolchain/tmp")
    count = 0
    variant, prefix = BUILD, ".pio/" + BUILD
    directories = [".pio", prefix, prefix + "/build", prefix + "/build/newt_x3",
                   prefix + "/libdeps", prefix + "/libdeps/newt_x3", *mutable_toolchain,
                   '.build/rust-app', '.build/rust-app/target', '.build/rust-app/ld',
                   '.toolchain/rust', '.toolchain/rust/cargo-home']
    cases = [(directory, kind) for directory in directories
             for kind in ("inside", "outside", "broken", "nonregular")]
    cases += [("root", "inside"), ("root-parent", "inside"), ("valid", "new"), ("valid", "existing")]
    for clean in (False, True):
        for directory, kind in cases:
            with tempfile.TemporaryDirectory() as temporary:
                sandbox = Path(temporary).resolve()
                project = sandbox / "project"
                project.mkdir()
                (project / daily_firmware.SOURCE_CONFIGURATION).write_bytes(
                    (ROOT / daily_firmware.SOURCE_CONFIGURATION).read_bytes())
                protected = None
                if directory == "root":
                    alias = sandbox / "root-alias"
                    alias.symlink_to(project, target_is_directory=True)
                    project = alias
                elif directory == "root-parent":
                    alias = sandbox / "parent-alias"
                    alias.symlink_to(sandbox, target_is_directory=True)
                    project = alias / "project"
                elif directory != "valid":
                    alias = project / directory
                    alias.parent.mkdir(parents=True, exist_ok=True)
                    target = (sandbox if kind == "outside" else project) / "alias-target"
                    if kind == "nonregular":
                        alias.write_bytes(b"preserve namespace file\n")
                        protected = alias
                    else:
                        if kind != "broken":
                            target.mkdir()
                            protected = target / "preserve"
                            protected.write_bytes(b"preserve namespace file\n")
                        alias.symlink_to(target, target_is_directory=True)
                elif kind == "existing":
                    for namespace in directories:
                        (project / namespace).mkdir(parents=True, exist_ok=True)
                    # These are lawful source/dependency file and library links,
                    # below namespaces the native driver owns, not namespace aliases.
                    vendor = project / "vendor/freeink-sdk/BoardConfig"
                    vendor.mkdir(parents=True)
                    library = project / prefix / "libdeps/newt_x3/BoardConfig"
                    library.symlink_to(vendor, target_is_directory=True)
                    implementation = project / "pinned-implementation"
                    implementation.write_bytes(b"read-only pinned implementation\n")
                    for name in ("platformio/penv/bin/python", "platformio/packages/compiler/bin/compiler"):
                        link = project / ".toolchain" / name
                        link.parent.mkdir(parents=True, exist_ok=True)
                        link.symlink_to(implementation)
                arguments = ["--clean"] if clean else []
                rejected, calls = run_native_main(driver, project, arguments)
                label = f"{variant} clean={clean}: {directory}/{kind}"
                if directory == "valid":
                    assert not rejected, label
                    native = [command for command in calls if command[0] == "mock-pio"]
                    assert len(native) == (2 if clean else 1), label
                    if clean:
                        assert native[0][-2:] == ["-t", "clean"], label
                    for namespace in directories:
                        assert (project / namespace).is_dir(), label
                    if kind == "existing":
                        assert library.is_symlink() and library.resolve() == vendor, label
                        assert implementation.read_bytes() == b"read-only pinned implementation\n", label
                else:
                    assert rejected, f"Native directory alias accepted: {label}"
                    assert not calls, f"Native child dispatched before directory rejection: {label}"
                    if protected is not None:
                        assert protected.read_bytes() == b"preserve namespace file\n", label
                    if kind == "broken":
                        assert not target.exists(), label
                count += 1
    return count


def verify_retired_variant_flags(driver=build) -> None:
    """There is one build; the old variant flags stop before any build step."""
    for arguments in (["--dev-usb"], ["--regional-home"], ["--clean", "--health-report", "--regional-home", "--dev-usb"]):
        error = io.StringIO()
        with (patch.object(sys, "argv", ["firmware-build.py", *arguments]), patch.object(driver.subprocess, "run") as run,
              redirect_stderr(error)):
            try:
                driver.main()
            except SystemExit as stop:
                assert stop.code == 2 and "there is one firmware build" in error.getvalue(), arguments
            else:
                raise AssertionError(f"Retired variant flags accepted: {arguments}")
        assert not run.called, arguments


def main() -> None:
    verify_relocated_driver()
    verify_retired_variant_flags()
    generated_cases = verify_generated_config_boundary()
    namespace_cases = verify_native_directory_boundary()
    overrides = {"PLATFORMIO_PACKAGES_DIR": "/outside-project/unapproved-packages",
                 "PLATFORMIO_BUILD_FLAGS": "!echo unapproved-command",
                 "PLATFORMIO_LIB_EXTRA_DIRS": "/outside-project/unapproved-libraries",
                 "PLATFORMIO_CORE_DIR": "/outside-project/unapproved-core",
                 "SCONSFLAGS": "--file=/outside-project/unapproved-SConstruct",
                 "SCONS_LIB_DIR": "/outside-project/unapproved-SCons",
                 "UNREVIEWED_FUTURE_BUILD_OVERRIDE": "unapproved"}
    with patch.dict(os.environ, overrides):
        env = build.build_environment()
        assert env["PLATFORMIO_CORE_DIR"] == str(ROOT / ".toolchain/platformio")
        assert env["SCONSFLAGS"] == "--no-site-dir"
        assert not (set(overrides) - {"PLATFORMIO_CORE_DIR", "SCONSFLAGS"}) & env.keys()
        if "HOME" in os.environ:
            assert env["HOME"] == os.environ["HOME"]
    print(f"PASS: build scope/environment preflight; {generated_cases} config and {namespace_cases} native-directory boundary cases")


if __name__ == "__main__":
    main()
