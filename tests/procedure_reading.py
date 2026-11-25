"""What a Python file, read statically, imports and loads: the reading that
procedure_closure_test.py applies to each fingerprint's procedure, with what
it runs read by procedure_launches.py.

The reading fails closed. Each import, load and run must reduce to constant
module names or paths, or it fails at its file and line; only an exact
ALLOWED entry stands in for one it cannot reduce. A module found nowhere in
the project must be in the standard library or EXTERNAL."""
import ast
import functools
from itertools import product
import os
from pathlib import Path, PurePath
import sys

ROOT = Path(__file__).resolve().parents[1]
TOOLS = ROOT / 'tools'
PIO = "the pinned toolchain venv's python runs PlatformIO's pio entry point, outside the project"
# Each load or run the rule cannot reduce, by file, line and call: the project
# files it loads or runs, and why that is all it does. Keep this small.
ALLOWED = {
    ('tests/host_objects_test.py', 331, "compile(source, host_common.__file__, 'exec')"): (
        ['tools/host_common.py'], "a control runs host_common.py's own source with one call moved"),
    ('tools/firmware-build.py', 101, "[str(interpreter), '-I', '-B', str(executable), 'run', '-e', 'newt_x3', '--jobs', '2']"): (
        [], f'build_command: {PIO}'),
    ('tools/firmware-build.py', 186, "subprocess.run([*command, '-t', 'clean'], cwd=ROOT, env=env, check=True)"): (
        [], f'the clean runs build_command: {PIO}'),
    ('tools/firmware-build.py', 208, 'subprocess.run(command, cwd=ROOT, env=env, check=True)'): (
        [], f'the build runs build_command and --project-conf options: {PIO}'),
    ('tools/rust_app_archive.py', 11, "[sys.executable, '-B', str(root / 'tools/rust-toolchain.py'), '--verify']"): (
        ['tools/rust-toolchain.py'], 'shared bench/daily helper runs only the checkout\'s pinned verifier'),
    ('tools/rust_app_archive.py', 11, "subprocess.run([sys.executable, '-B', str(root / 'tools/rust-toolchain.py'), '--verify'], cwd=root, check=True)"): (
        ['tools/rust-toolchain.py'], 'same exact cross-module verifier call; its root is the caller\'s checkout'),
    ('tools/rust_app_archive.py', 38, "subprocess.run([str(toolchain / 'cargo'), 'clean', '--offline', '--release', '--manifest-path', str(manifest), '--target', TARGET, '--target-dir', str(target)], cwd=root, env=environment, check=True)"): (
        [], 'Cargo from the identity-verified local Rust toolchain cleans its fixed project-local target'),
    ('tools/rust_app_archive.py', 41, "subprocess.run([str(toolchain / 'cargo'), 'build', '--offline', '--locked', '--release', '--manifest-path', str(manifest), '--target', TARGET, '--target-dir', str(target), '-p', package, '--lib', '--jobs', '2', *(['--features', features] if features else [])], cwd=root, env=environment, check=True)"): (
        [], 'identity-verified local Cargo builds the fixed workspace offline/locked; daily features are fixed'),
    ('tools/rust-toolchain.py', 65, "subprocess.run([str(base / 'toolchain/bin/rustc'), '--version', '--verbose'], capture_output=True, text=True, check=True)"): (
        [], 'the verifier probes rustc only after checking every installed byte against the approved lock'),
    ('tools/rust-toolchain.py', 67, "subprocess.run([str(base / 'toolchain/bin/cargo'), '--version'], capture_output=True, text=True, check=True)"): (
        [], 'the verifier probes Cargo only after checking every installed byte against the approved lock'),
    ('tools/rust-toolchain.py', 101, "subprocess.run(['sh', str(directory / 'install.sh'), f'--prefix={prefix}', '--sysconfdir=' + str(prefix / 'etc'), '--disable-ldconfig'], check=True)"): (
        [], 'installer script extracted only from checksum-verified official archives; --verify never installs'),
    ('tools/rust-toolchain.py', 103, "subprocess.run([str(BASE / 'toolchain/bin/rustc'), '--version', '--verbose'], capture_output=True, text=True, check=True)"): (
        [], 'the approved installation probes its own pinned rustc; no project Python is loaded'),
}
# Modules outside the project that covered code imports or runs, and why.
EXTERNAL = {'commitizen': 'the pinned release tool release_tools.py runs from the release venv',
            'serial': "pyserial 3.5 from PlatformIO's toolchain; dev-usb.py checks its version and place"}
# Each load function, and the argument (position, keyword) naming what it loads.
LOADS = {'import_module': (0, 'name'), '__import__': (0, 'name'), 'run_module': (0, 'mod_name'),
         'spec_from_file_location': (1, 'location'), 'run_path': (0, 'path_name'),
         'SourceFileLoader': (1, 'path'), 'SourcelessFileLoader': (1, 'path'), 'load_source': (1, 'pathname'),
         'exec': (0, 'source'), 'eval': (0, 'source'), 'compile': (0, 'source')}
BY_NAME, CODE = ('import_module', '__import__', 'run_module'), ('exec', 'eval', 'compile')
SYS_PATH = {'sys.path.insert': 1, 'sys.path.append': 0, 'site.addsitedir': 0}
FUNCTIONS = {'str': str, 'os.fspath': os.fspath, 'Path': Path, 'pathlib.Path': Path, 'os.path.join': os.path.join,
             'os.path.dirname': os.path.dirname, 'os.path.abspath': os.path.abspath,
             'os.path.realpath': os.path.realpath}
PATH_METHODS, PATH_ATTRIBUTES = ('resolve', 'absolute', 'with_name', 'with_suffix', 'joinpath'), (
    'parent', 'name', 'stem', 'suffix')
SCOPES = (ast.FunctionDef, ast.AsyncFunctionDef, ast.Lambda, ast.ClassDef, ast.ListComp, ast.SetComp,
          ast.DictComp, ast.GeneratorExp)
# Controls overlay sources here: a resolved path and the text read in its place.
SOURCES = {}


class Unresolved(Exception):
    """An expression the rule cannot reduce to constants."""


def relative(path):
    return path.relative_to(ROOT).as_posix() if path.is_relative_to(ROOT) else str(path)


def binds(node):
    """The names a node binds in its scope."""
    if isinstance(node, ast.Name) and not isinstance(node.ctx, ast.Load):
        return [node.id]
    if isinstance(node, ast.arg):
        return [node.arg]
    if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
        return [node.name]
    if isinstance(node, (ast.Import, ast.ImportFrom)):
        return [(alias.asname or alias.name).partition('.')[0] for alias in node.names]
    if isinstance(node, (ast.ExceptHandler, ast.MatchAs, ast.MatchStar)) and node.name:
        return [node.name]
    return [node.rest] if isinstance(node, ast.MatchMapping) and node.rest else []


def given(call, position, keyword):
    """The argument a call passes at a position or by keyword."""
    for item in call.keywords:
        if item.arg == keyword:
            return item.value
    if len(call.args) > position and not any(isinstance(item, ast.Starred) for item in call.args[:position + 1]):
        return call.args[position]
    raise Unresolved(f'no {keyword} it can read')


def passed(function, call, name):
    """The expression a call of a function passes for one parameter."""
    arguments = function.args
    names = [argument.arg for argument in (*arguments.posonlyargs, *arguments.args)]
    if any(isinstance(item, ast.Starred) for item in call.args) or any(item.arg is None for item in call.keywords):
        raise Unresolved(f'a call of {function.name} unpacks its arguments')
    if name in names and names.index(name) < len(call.args):
        return call.args[names.index(name)]
    defaults = {**dict(zip(names[len(names) - len(arguments.defaults):], arguments.defaults)),
                **{item.arg: value for item, value in zip(arguments.kwonlyargs, arguments.kw_defaults) if value}}
    found = next((item.value for item in call.keywords if item.arg == name), defaults.get(name))
    if found is None:
        raise Unresolved(f'a call of {function.name} passes no {name}')
    return found


def combined(operator, left, right):
    if isinstance(operator, ast.Add) and isinstance(left, str) and isinstance(right, str):
        return left + right
    if isinstance(operator, ast.Div) and isinstance(left, PurePath) and isinstance(right, (str, PurePath)):
        return left / right
    raise TypeError(f'{type(left).__name__} and {type(right).__name__}')


def project_module(name, directories, files):
    """A module's project file, or None for one in the standard library or EXTERNAL."""
    top = name.partition('.')[0] if isinstance(name, str) else ''
    if not top:
        raise Unresolved(f'{name!r} is not an absolute module name')
    return located(top, tuple(directories), tuple(sorted(files)))


@functools.lru_cache(maxsize=None)
def located(top, directories, files):
    """project_module for one top-level name, given the overlaid files' paths."""
    for directory in directories:
        if (directory / (top + '.py')).resolve() in files or (directory / (top + '.py')).is_file():
            return (directory / (top + '.py')).resolve()
        if (directory / top).is_dir():
            raise Unresolved(f'{top} is a project package')
    if top in sys.stdlib_module_names or top in EXTERNAL:
        return None
    raise Unresolved(f'no module {top} in the project, the standard library or EXTERNAL')


class Reader:
    """Reduces a file's expressions to every constant each can take: strings,
    integers and paths. A name must have one binding in its scope: a constant
    expression, a loop over constants, a project module's own such name, or a
    parameter, taken from each call of its function in this file, which must
    use that function only by calling it."""

    def __init__(self, path, tree, files, directories):
        self.path, self.files, self.directories, self.parents, self.bound, self.reads = (
            path, files, directories, {}, {}, {})
        self.declared, self.called, self.active = set(), set(), set()
        # Every node, in ast.walk's order, walked once.
        self.nodes = [tree]
        for node in self.nodes:
            for child in ast.iter_child_nodes(node):
                self.parents[child] = node
                self.nodes.append(child)
        for node in self.nodes:
            if isinstance(node, (ast.Global, ast.Nonlocal)):
                self.declared.update(node.names)
            if isinstance(node, ast.Name) and isinstance(node.ctx, ast.Load):
                self.reads.setdefault(node.id, []).append(node)
            for name in binds(node):
                self.bound.setdefault((self.scope(node), name), []).append(node)

    def scope(self, node):
        node = self.parents.get(node)
        while node is not None and not isinstance(node, SCOPES):
            node = self.parents.get(node)
        return node

    def bindings(self, node):
        """Every node binding a name in the scope where node reads it."""
        scope, nearest = self.scope(node), True
        while True:
            found = None if isinstance(scope, ast.ClassDef) and not nearest else self.bound.get((scope, node.id))
            if found is not None:
                return found
            if scope is None:
                raise Unresolved(f'{node.id} is not bound in this file')
            scope, nearest = self.scope(scope), False

    def binding(self, node):
        """The one node binding a name where node reads it."""
        found = self.bindings(node)
        if len(found) != 1 or node.id in self.declared:
            raise Unresolved(f'{node.id} is bound more than once')
        return found[0]

    def values(self, node):
        try:
            return self.reduce(node)
        except (TypeError, ValueError, IndexError) as error:
            raise Unresolved(f'{ast.unparse(node)}: {error}') from None

    def reduce(self, node):
        if isinstance(node, ast.Constant) and isinstance(node.value, (str, int)):
            return {node.value}
        if isinstance(node, ast.JoinedStr):
            return {''.join(parts) for parts in product(*map(self.values, node.values))}
        if isinstance(node, ast.FormattedValue) and node.conversion in (-1, 115) and node.format_spec is None:
            return set(map(str, self.values(node.value)))
        if isinstance(node, ast.BinOp) and isinstance(node.op, (ast.Add, ast.Div)):
            return {combined(node.op, *pair) for pair in product(self.values(node.left), self.values(node.right))}
        if isinstance(node, ast.Name):
            return {str(self.path)} if node.id == '__file__' else self.named(node)
        if isinstance(node, ast.Attribute) and node.attr in PATH_ATTRIBUTES:
            return {getattr(PurePath(value), node.attr) for value in self.values(node.value)}
        if isinstance(node, ast.Subscript) and isinstance(node.value, ast.Attribute) and node.value.attr == 'parents':
            return {PurePath(value).parents[index] for value in self.values(node.value.value)
                    for index in self.values(node.slice)}
        if (isinstance(node, ast.Call) and not node.keywords
                and not any(isinstance(item, ast.Starred) for item in node.args)):
            function, arguments = ast.unparse(node.func), list(product(*map(self.values, node.args)))
            if function in FUNCTIONS:
                return {FUNCTIONS[function](*items) for items in arguments}
            if isinstance(node.func, ast.Attribute) and node.func.attr in PATH_METHODS:
                return {getattr(Path(value), node.func.attr)(*items)
                        for value in self.values(node.func.value) for items in arguments}
        raise Unresolved(f'{ast.unparse(node)} is not a constant')

    def named(self, node):
        return self.bound_to(self.binding(node), node.id)

    def exported(self, name):
        """The values of a name this file binds once at module level."""
        found = self.bound.get((None, name), [])
        if len(found) != 1 or name in self.declared:
            raise Unresolved(f'{relative(self.path)} does not bind {name} once')
        return self.bound_to(found[0], name)

    def bound_to(self, binding, name):
        if binding in self.active:
            raise Unresolved(f'{name} depends on itself')
        self.active.add(binding)
        try:
            parent = self.parents.get(binding)
            if isinstance(binding, ast.arg):
                return self.parameter(self.parents[parent], binding.arg)
            if isinstance(binding, ast.ImportFrom) and not binding.level:
                source = project_module(binding.module, self.directories, self.files)
                if source is None:
                    raise Unresolved(f'{name} is from outside the project')
                original = next(alias.name for alias in binding.names if (alias.asname or alias.name) == name)
                return module_reader(source, tuple(sorted(self.files.items()))).exported(original)
            if isinstance(parent, ast.Assign) and parent.targets == [binding]:
                return self.values(parent.value)
            if isinstance(parent, ast.AnnAssign) and parent.value is not None:
                return self.values(parent.value)
            if (isinstance(parent, (ast.For, ast.comprehension)) and parent.target is binding
                    and isinstance(parent.iter, (ast.Tuple, ast.List, ast.Set))):
                return set().union(*map(self.values, parent.iter.elts))
            raise Unresolved(f'{name} is not bound to constants')
        finally:
            self.active.discard(binding)

    def parameter(self, function, name):
        """Every value one parameter takes at each call of its function in this file."""
        if not isinstance(function, (ast.FunctionDef, ast.AsyncFunctionDef)):
            raise Unresolved(f'{name} is a lambda parameter')
        calls = []
        for read in self.reads.get(function.name, ()):
            if self.binding(read) is function:
                if not (isinstance(self.parents.get(read), ast.Call) and self.parents[read].func is read):
                    raise Unresolved(f'{function.name} is used other than by a call')
                calls.append(self.parents[read])
        if not calls:
            raise Unresolved(f'{function.name} has no call in this file')
        found = set().union(*(self.values(passed(function, call, name)) for call in calls))
        self.called.add(function.name)
        return found


class Reading:
    """What one Python file, read statically, imports or loads, and each
    import or load the rule cannot resolve, at its location. Launching reads
    what it runs."""

    def __init__(self, path, tree, files, directories=None):
        self.path, self.tree, self.files, self.directories = path, tree, files, directories or [path.parent, TOOLS]
        self.reader, self.imports, self.runs = Reader(path, tree, files, self.directories), set(), set()
        self.failures, self.run_failures, self.allowed, self.external = [], [], set(), set()
        self.nested = []
        nodes = self.reader.nodes
        self.references = {node.attr for node in nodes if isinstance(node, ast.Attribute)} | {
            alias.name for node in nodes if isinstance(node, ast.ImportFrom) for alias in node.names}
        aliases = {alias.asname: alias.name for node in nodes if isinstance(node, ast.ImportFrom)
                   for alias in node.names if alias.asname}
        modules, loads, added = [], [], set()
        for node in nodes:
            function = ast.unparse(node.func) if isinstance(node, ast.Call) else ''
            name = aliases.get(function, function).rpartition('.')[2]
            if isinstance(node, ast.Import):
                modules += [(node, alias.name) for alias in node.names]
            elif isinstance(node, ast.ImportFrom):
                modules.append((node, '.' * node.level + (node.module or '')))
            elif function in SYS_PATH:
                self.attempt(node, lambda node=node: map(self.directory, self.reader.values(
                    given(node, SYS_PATH[function], 'path'))), added, self.failures)
            elif isinstance(node, (ast.Assign, ast.AugAssign, ast.AnnAssign)) and any(
                    ast.unparse(target).startswith('sys.path') for target in getattr(node, 'targets', [])
                    + [getattr(node, 'target', None)] if target is not None):
                self.failures.append(f'{relative(path)}:{node.lineno}: cannot resolve a change to sys.path')
            elif name in LOADS and (name not in CODE or isinstance(node.func, ast.Name)) and not (
                    name in ('exec', 'eval') and node.args and isinstance(node.args[0], ast.Call)
                    and ast.unparse(node.args[0].func) == 'compile'):
                loads.append((node, name))
            used = node.id if isinstance(node, ast.Name) else node.attr if isinstance(node, ast.Attribute) else ''
            used, call = aliases.get(used, used), self.reader.parents.get(node)
            if used in LOADS and (used not in CODE or isinstance(node, ast.Name)) and isinstance(
                    node.ctx, ast.Load) and not (isinstance(call, ast.Call) and call.func is node):
                self.failures.append(f'{relative(path)}:{node.lineno}: cannot resolve {used} used but not called')
        self.directories[:0] = sorted(added)
        for node, module in modules:
            self.attempt(node, lambda module=module: [self.module(module)], self.imports, self.failures)
        for node, name in loads:
            self.attempt(node, lambda node=node, name=name: self.load(node, name), self.imports, self.failures)

    def attempt(self, node, action, into, failures):
        key = (relative(self.path), node.lineno, ast.unparse(node))
        if key in ALLOWED:
            self.allowed.add(key)
            into.update((ROOT / name).resolve() for name in ALLOWED[key][0])
            return 'allowed'
        try:
            into.update(item for item in action() if item is not None)
        except Unresolved as error:
            failures.append(f'{key[0]}:{key[1]}: cannot resolve {key[2]}: {error}')
            return 'failed'
        return 'resolved'

    def exists(self, path):
        return path.resolve() in self.files or path.is_file()

    def directory(self, value):
        """A directory added to sys.path: a project directory, or None for one outside."""
        place = (ROOT / value).resolve()
        if not place.is_relative_to(ROOT):
            return None
        if not place.is_dir():
            raise Unresolved(f'no project directory {value}')
        return place

    def module(self, name):
        found = project_module(name, self.directories, self.files)
        if found is None and name.partition('.')[0] in EXTERNAL:
            self.external.add(name.partition('.')[0])
        return found

    def file(self, value):
        """The project file a loaded or run path names."""
        place = Path(value)
        candidates = [place] if place.is_absolute() else [ROOT / place, *(base / place for base in self.directories)]
        for candidate in candidates:
            if self.exists(candidate):
                if not candidate.resolve().is_relative_to(ROOT):
                    raise Unresolved(f'{value} is outside the project')
                return candidate.resolve()
        raise Unresolved(f'no file {value}')

    def code(self, values, line):
        """Python source a call runs in this file's context: what it imports or loads."""
        for value in values:
            try:
                nested = type(self)(self.path, ast.parse(value), self.files, self.directories)
            except (SyntaxError, TypeError) as error:
                raise Unresolved(f'its code does not parse: {error}') from None
            if nested.failures:
                raise Unresolved(f'its code at line {line}: ' + '; '.join(nested.failures))
            self.nested.append(nested)
            self.external |= nested.external
            yield from nested.imports

    def load(self, node, name):
        values = self.reader.values(given(node, *LOADS[name]))
        if name in BY_NAME:
            return [self.module(value) for value in values]
        return self.code(values, node.lineno) if name in CODE else [self.file(value) for value in values]


def parsed(path, files):
    return tree_of(path, files[path] if path in files else path.read_text())


@functools.lru_cache(maxsize=None)
def tree_of(path, source):
    """One file's tree, parsed once for each text. No reading changes a tree."""
    return ast.parse(source, str(path))


@functools.lru_cache(maxsize=None)
def module_reader(path, overlay):
    """A project module's names, as a file that imports from it reads them."""
    return Reader(path, parsed(path, dict(overlay)), dict(overlay), [path.parent, TOOLS])
