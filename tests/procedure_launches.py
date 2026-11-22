"""What a Python file, read statically, runs: the procedure_reading.py
reading, with each process the file starts.

A run is a call of a process launcher, however the file names it: by import,
from-import, assignment, getattr or __import__, through a project module's or
a standard-library module's attribute, or through a project function that
passes its own parameter to one. Its command comes from the launcher's
argument, positional or by keyword, with executable= and shell=. The program
must reduce to an interpreter, whose script, -m module or -c code must reduce
in turn, or to a program in PROGRAMS. These fail too:
- an unknown launcher, or a call named like one whose function the reading
  cannot place;
- a call of a computed function, or of a name that a global or nonlocal
  statement lets another scope rebind;
- a launcher used other than by a call, or aliased in a class;
- Popen options passed by position;
- any other list naming an interpreter that does not reduce.
Only an exact ALLOWED entry stands in for a run the rule cannot reduce."""
import ast
import builtins
import functools
import importlib
from pathlib import Path, PurePath
import re
import shlex
import sys
import types

from procedure_reading import SOURCES, Reading, Unresolved, given, parsed, passed, project_module, relative

# Programs covered code runs, by file name, and why none runs project Python.
PROGRAMS = {'git': 'reads or archives the repository and the FreeInk source',
            'tar': 'unpacks the FreeInk archive apply-freeink-patch.py stages',
            'patch': 'applies the reviewed FreeInk patch to that staged copy',
            'riscv32-esp-elf-size': "the pinned toolchain's size reads the firmware ELF",
            'riscv32-esp-elf-objdump': "the pinned toolchain's objdump reads the firmware ELF",
            'riscv32-esp-elf-nm': "the pinned toolchain's nm reads the firmware ELF's symbols",
            'riscv32-esp-elf-readelf': "the pinned toolchain's readelf reads the firmware ELF's DWARF type sizes"}
# Each process launcher, and where its command is: an argv list or a shell
# string at (position, keyword); or the program's position, then its argv:
# 'v' a list next, 'l' the remaining arguments ('e' less the environment).
# Each argv holds argv[0] first, except create_subprocess_exec's ('').
LAUNCHERS = {**{f'subprocess.{name}': ('argv', 0, 'args')
                for name in ('run', 'Popen', 'call', 'check_call', 'check_output')},
             **{f'subprocess.{name}': ('shell', 0, 'cmd') for name in ('getoutput', 'getstatusoutput')},
             'os.system': ('shell', 0, 'command'), 'os.popen': ('shell', 0, 'cmd'), 'pty.spawn': ('argv', 0, 'argv'),
             **{f'asyncio.{module}create_subprocess_{kind}': ('shell', 0, 'cmd') if kind == 'shell' else
                ('exec', 0, '') for module in ('', 'subprocess.') for kind in ('exec', 'shell')},
             'os.posix_spawn': ('exec', 0, 'v'), 'os.posix_spawnp': ('exec', 0, 'v'),
             **{f'os.{verb}{form}': ('exec', at, form) for verb, at in (('exec', 0), ('spawn', 1))
                for form in ('l', 'le', 'lp', 'lpe', 'v', 've', 'vp', 'vpe')}}
LAUNCHER_MODULES = {'subprocess', 'os', 'posix', 'asyncio', 'asyncio.subprocess', 'pty', '_posixsubprocess'}
LAUNCHER_NAMES = {name.rpartition('.')[2] for name in LAUNCHERS} | {
    'subprocess_exec', 'subprocess_shell', 'startfile', 'fork_exec'}
UNKNOWN = re.compile(r'subprocess\.(?!(PIPE|DEVNULL|STDOUT|CompletedProcess|CalledProcessError|TimeoutExpired|'
                     r'SubprocessError|list2cmdline)$)\w+|os\.(exec|spawn|posix_spawn|startfile)\w*|'
                     r'asyncio\.(subprocess\.)?create_subprocess\w*|.+\.subprocess_(exec|shell)|_posixsubprocess\.\w+')
# Standard-library modules whose import does something, which canonical() does not import.
EFFECTS = {'antigravity', 'this', '__hello__', '__phello__'}
INTERPRETER = re.compile(r'sys\.executable|(python|pypy)(\d+(\.\d+)*)?[dmtw]?(\.exe)?')
# Standard-library modules that, run with -m, run other code they are given.
RUNNERS = ('cProfile', 'doctest', 'pdb', 'profile', 'runpy', 'timeit', 'trace', 'unittest')
SHELL_WORDS = re.compile(r"[\w@%+=:,./' \"-]*")
MUTATIONS, GROWTH = ('insert', 'remove', 'pop', 'clear', 'sort', 'reverse', '__setitem__', '__delitem__'), (
    'append', 'extend', '__iadd__')


@functools.lru_cache(maxsize=None)
def canonical(name):
    """A dotted standard-library name as its object is known at run time: the
    LAUNCHERS entry it is, through any module that re-exports it, or else the
    name after the last module it passes through, by that module's own name."""
    top, *rest = name.split('.')
    if top not in sys.stdlib_module_names or top in EFFECTS:
        return name
    try:
        value = importlib.import_module(top)
    except ImportError:
        return name
    parts = [value.__name__]
    for part in rest:
        value = getattr(value, part, None) if isinstance(value, types.ModuleType) else None
        parts = [value.__name__] if isinstance(value, types.ModuleType) else [*parts, part]
    return RUNTIME.get(id(value), '.'.join(parts)) if value is not None else '.'.join(parts)


def runtime(name):
    """The object a LAUNCHERS name is on this host, or None."""
    value = importlib.import_module(name.partition('.')[0])
    for part in name.split('.')[1:]:
        value = getattr(value, part, None)
    return value


# Each launcher, by its object's identity, for canonical().
RUNTIME = {id(value): name for name in LAUNCHERS if (value := runtime(name)) is not None}
PROGRAM_CALLS = {canonical(name) for name in (
    'builtins.str', 'os.fspath', 'pathlib.Path', 'shutil.which', 'os.path.abspath', 'os.path.realpath')}
# Project functions whose wrapper reading is in progress, those a call cycle
# reached, and each finished reading.
WRAPPING, CYCLIC, WRAPS = [], set(), {}


def raised(error):
    raise error


class Launching(Reading):
    """A Reading that, once launched(), also holds what its file runs: the
    project files in runs, the PROGRAMS it starts, each launch site and how
    the rule read it, and each run the rule cannot resolve in run_failures."""

    def __init__(self, path, tree, files, directories=None):
        super().__init__(path, tree, files, directories)
        self.sites, self.programs, self.listing, self.wrappers = None, set(), set(), {}
        # Names a star import binds cannot be traced to a launcher or wrapper.
        self.run_failures += [f'{relative(path)}:{node.lineno}: cannot resolve {ast.unparse(node)}: a star import'
                              for node in self.reader.nodes if isinstance(node, ast.ImportFrom)
                              and any(alias.name == '*' for alias in node.names)]

    def qualified(self, node, active=frozenset()):
        """Every dotted name an expression can denote, through the bindings of
        its names in scope, getattr and __import__: a builtin as builtins.name,
        a function of a file as path::name, ? for a name the rule cannot reduce."""
        if isinstance(node, ast.Name):
            try:
                bindings = self.reader.bindings(node)
            except Unresolved:
                return {f'builtins.{node.id}'} if hasattr(builtins, node.id) else set()
            if node.id in self.reader.declared:
                return {'*'}  # global or nonlocal: other scopes may rebind it
            return set().union(*(self.denoted(binding, node.id, active) for binding in bindings))
        if isinstance(node, ast.Attribute):
            return set().union(*(self.member(base, node.attr, active) for base in self.qualified(node.value, active)))
        if isinstance(node, ast.Call):
            function = self.qualified(node.func, active)
            if 'builtins.getattr' in function and len(node.args) > 1:
                return set().union(*(self.member(base, str(name), active) for base in self.qualified(
                    node.args[0], active) for name in self.reduced(node.args[1])))
            if function & {'builtins.__import__', 'importlib.import_module'} and node.args:
                return {canonical(str(name).partition('.')[0] if 'builtins.__import__' in function else str(name))
                        for name in self.reduced(node.args[0])}
        if isinstance(node, ast.Lambda):
            return {f'{self.path}::<lambda>'}  # read in place, like any function of the file
        # Any other value, such as a call's result or a subscript, is computed: *.
        return {'*'}

    def denoted(self, binding, name, active):
        """What one binding of a name can denote: an import, a function or class, or an assigned expression."""
        if binding in active:
            return set()
        if isinstance(binding, ast.Import):
            alias = next(alias for alias in binding.names if (alias.asname or alias.name).partition('.')[0] == name)
            return {canonical(alias.name if alias.asname else name)}
        if isinstance(binding, ast.ImportFrom):
            alias = next(alias for alias in binding.names if (alias.asname or alias.name) == name)
            return {'?'} if binding.level else self.member(binding.module, alias.name, active | {binding})
        if isinstance(binding, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            return {f'{self.path}::{binding.name}'}
        parent = self.reader.parents.get(binding)
        targets = parent.targets if isinstance(parent, ast.Assign) else [getattr(parent, 'target', None)]
        if (isinstance(parent, (ast.Assign, ast.AnnAssign, ast.NamedExpr))
                and parent.value is not None and binding in targets):
            return self.qualified(parent.value, active | {binding})
        return set()

    def member(self, base, attribute, active=frozenset()):
        """What base.attribute names: for a project module, what that module
        binds the attribute to; for a standard-library one, its canonical name."""
        name = f'{base}.{attribute}'
        if '?' in base or '*' in base or '::' in base:
            return {name}
        try:
            path = project_module(base, self.directories, self.files) if '.' not in base else None
        except Unresolved:
            path = None
        if path is None:
            return {canonical(name)}
        found = read(path)
        return set().union(*(found.denoted(binding, attribute, active) for binding in found.reader.bound.get(
            (None, attribute), []))) or {name}

    def reduced(self, node):
        try:
            return self.reader.values(node)
        except Unresolved:
            return {'?'}

    def wrapper(self, name):
        """The project function a qualified name denotes, and how it launches
        what its callers pass, as wraps() reads it; (None, ()) for any other."""
        if name not in self.wrappers:
            self.wrappers[name] = self.wrapped(name)
        return self.wrappers[name]

    def wrapped(self, name):
        if '?' in name or '*' in name:
            return None, ()
        if '::' in name:
            place, _, function = name.rpartition('::')
            path = Path(place)
        else:
            module, _, function = name.rpartition('.')
            try:
                path = project_module(module, self.directories, self.files) if module and '.' not in module else None
            except Unresolved:
                path = None
        return wrapping(path, function) if path else (None, ())

    def wraps(self, name):
        """A module-level function of this file and, for each launch in it
        that runs what its callers pass, the launch's form and the parameter
        holding the command, or None where callers pass only options."""
        for function in self.reader.bound.get((None, name), []):
            specs, functional = [], isinstance(function, (ast.FunctionDef, ast.AsyncFunctionDef))
            for call in (node for node in ast.walk(function) if functional and isinstance(node, ast.Call)):
                try:
                    launches = self.launches(call)
                except Unresolved:
                    continue
                for kind, callee, parameter in launches:
                    try:
                        command = self.command_of(kind, call, callee, parameter)
                        owner, forwarded = self.passes(command), self.forwards(call)
                    except Unresolved:
                        continue
                    if owner and owner[0] is function:
                        specs.append((kind[0], owner[1]))
                    elif forwarded is function:
                        specs.append((kind[0], None))
            if specs:
                return function, tuple(specs)
        return None, ()

    def launches(self, call):
        """Each launch a call makes: its kind, and for a project function that
        passes on what its callers give, that function and the parameter."""
        names = self.qualified(call.func)
        unknown = sorted(name for name in names if name not in LAUNCHERS and ('?' in name or UNKNOWN.fullmatch(name)))
        if unknown:
            raise Unresolved(f'{unknown[0]} is a launcher the rule does not read')
        if '*' in names:
            raise Unresolved(f'it calls {ast.unparse(call.func)}, a function the rule cannot place')
        found = [(LAUNCHERS[name], None, None) for name in sorted(names & LAUNCHERS.keys())]
        for name in sorted(names - LAUNCHERS.keys()):
            function, specs = self.wrapper(name)
            found += [((form, None, None), function, parameter) for form, parameter in specs]
        leaf = call.func.attr if isinstance(call.func, ast.Attribute) else getattr(call.func, 'id', '')
        if leaf in LAUNCHER_NAMES and all(name.startswith('*') for name in names):
            raise Unresolved(f'the rule cannot tell whether {ast.unparse(call.func)} starts a process')
        return found

    def command_of(self, kind, call, callee, parameter):
        """A launch's command expression, or None for one an exec launcher spreads over its arguments."""
        if callee is None:
            return None if kind[0] == 'exec' else given(call, kind[1], kind[2])
        if parameter is None:
            return None
        command = passed(callee, call, parameter)
        if command not in self.reader.parents:
            raise Unresolved(f'{callee.name} takes its {parameter} from a default')
        return command

    def passes(self, command):
        """The module-level function, and its parameter, that a command is."""
        try:
            binding = self.reader.binding(command) if isinstance(command, ast.Name) else None
        except Unresolved:
            return None
        arguments = self.reader.parents.get(binding)
        function = self.reader.parents.get(arguments)
        if (isinstance(binding, ast.arg) and isinstance(function, (ast.FunctionDef, ast.AsyncFunctionDef))
                and self.reader.scope(function) is None and binding not in (arguments.vararg, arguments.kwarg)):
            return function, binding.arg
        return None

    def forwards(self, call):
        """The module-level function whose ** parameter a call passes on, if any; any other ** fails."""
        found = None
        for item in call.keywords:
            if item.arg is None:
                try:
                    binding = self.reader.binding(item.value) if isinstance(item.value, ast.Name) else None
                except Unresolved:
                    binding = None
                found = self.reader.parents.get(self.reader.parents.get(binding))
                if not (isinstance(found, (ast.FunctionDef, ast.AsyncFunctionDef)) and binding is found.args.kwarg
                        and self.reader.scope(found) is None):
                    raise Unresolved(f'it passes options {ast.unparse(item.value)} the rule cannot read')
        return found

    def launched(self):
        """Read once what this file runs: each launch, and each other list naming an interpreter."""
        if self.sites is not None:
            return self
        self.sites, handled = {}, set()
        for node in self.reader.nodes:
            if isinstance(node, ast.Call):
                try:
                    launches = self.launches(node)
                except Unresolved as error:
                    launches = error
                if launches:
                    self.sites[(relative(self.path), node.lineno, ast.unparse(node))] = self.attempt(
                        node, lambda node=node, launches=launches: self.start(node, launches, handled),
                        self.runs, self.run_failures)
            if isinstance(node, (ast.Name, ast.Attribute, ast.Call)):
                problem = self.misused(node)
                if problem:
                    self.attempt(node, lambda problem=problem: raised(Unresolved(problem)), self.runs,
                                 self.run_failures)
        for node in self.reader.nodes:
            if isinstance(node, (ast.List, ast.Tuple)) and node.elts and id(node) not in handled and self.python(node):
                self.attempt(node, lambda node=node: self.command(self.elements(node)[1:], node.lineno), self.runs,
                             self.run_failures)
        for nested in self.nested:
            nested.launched()
            self.runs |= nested.runs
            self.run_failures += nested.run_failures
            self.allowed, self.programs = self.allowed | nested.allowed, self.programs | nested.programs
            self.sites.update(nested.sites)
            self.external |= nested.external
        return self

    def misused(self, node):
        """Why a launcher, or a launcher module, is used other than by a call or
        an alias outside a class, if it is."""
        parent = self.reader.parents.get(node)
        aliased = isinstance(parent, (ast.Assign, ast.AnnAssign, ast.NamedExpr)) and parent.value is node and all(
            isinstance(target, ast.Name) for target in getattr(parent, 'targets', [getattr(parent, 'target', None)])
        ) and not isinstance(self.reader.scope(parent), ast.ClassDef)
        if aliased or not isinstance(getattr(node, 'ctx', ast.Load()), ast.Load) or (
                isinstance(parent, ast.Call) and parent.func is node):
            return None
        names = self.qualified(node)
        if any(name in LAUNCHERS or '?' in name or UNKNOWN.fullmatch(name) or self.wrapper(name)[1]
               for name in names):
            return 'a launcher used other than by a call'
        attribute = isinstance(parent, ast.Attribute) or isinstance(parent, ast.Call) and parent.args[:1] == [
            node] and 'builtins.getattr' in self.qualified(parent.func)
        return None if attribute or not names & LAUNCHER_MODULES else 'a launcher module used other than by attribute'

    def start(self, call, launches, handled):
        """The project files one launcher call runs. A command that is its
        function's parameter is read at each call of that function instead."""
        if isinstance(launches, Unresolved):
            raise launches
        self.forwards(call)
        files = []
        for kind, callee, parameter in launches:
            if callee is None and kind[2] == 'args' and len(call.args) > 1:
                raise Unresolved('it passes Popen options, such as executable, by position')
            command = self.command_of(kind, call, callee, parameter)
            if callee is not None and parameter is None:
                if any(item.arg in ('executable', 'shell') for item in call.keywords):
                    raise Unresolved(f'it passes {callee.name} the options of its launch')
                continue
            handled.add(id(command))
            if self.passes(command) is None:
                files += self.run(kind, call, command)
        return files

    def run(self, kind, call, command):
        """The project files one launch runs, from its program and arguments."""
        options = {item.arg: item.value for item in call.keywords if item.arg}
        shell = options.get('shell')
        if shell is not None and not isinstance(shell, ast.Constant):
            raise Unresolved('its shell= is not a constant')
        if kind[0] == 'exec':
            at, form = kind[1:]
            if len(call.args) < at + 1 + ('v' in form) or any(
                    isinstance(item, ast.Starred) for item in call.args[:at + 1 + ('v' in form)]):
                raise Unresolved('its program or argv is not where the rule reads it')
            items = (self.elements(call.args[at + 1]) if 'v' in form else self.elements(
                ast.List(call.args[at + 1:len(call.args) - ('e' in form)], ast.Load())))[bool(form):]
            program = options.get('executable', call.args[at])
        elif kind[0] == 'shell' or shell is not None and shell.value:
            if 'executable' in options:
                raise Unresolved('it names its shell with executable=')
            line = self.elements(command)[:1] if isinstance(command, (ast.List, ast.Tuple)) else [command]
            if not line:
                raise Unresolved('it runs no shell command')
            program, *items = self.words(line[0])
        else:
            items = [command] if self.string(command) else self.elements(command)
            if not items:
                raise Unresolved('it runs no program')
            program, items = options.get('executable', items[0]), items[1:]
        return self.started(program, items, call.lineno)

    def started(self, program, items, line):
        """What a program runs: an interpreter's script, -m module or -c code, or nothing for one in PROGRAMS."""
        try:
            names = self.program_names(program)
        except Unresolved:
            if not any(self.option(item) in ('-c', '-m') for item in items):
                raise
            names = {'python'}
        python = {name for name in names if INTERPRETER.fullmatch(name)}
        unknown = sorted(names - python - PROGRAMS.keys())
        if unknown:
            raise Unresolved(f'it runs {unknown[0]}, neither an interpreter nor in PROGRAMS')
        self.programs |= names - python
        return self.command(items, line) if python else []

    def program_names(self, node, active=frozenset()):
        """The file name of each program an expression can name; sys.executable for this interpreter."""
        if self.qualified(node) & {'sys.executable', 'sys._base_executable'}:
            return {'sys.executable'}
        if (isinstance(node, ast.Call) and len(node.args) == 1 and not node.keywords
                and not isinstance(node.args[0], ast.Starred)
                and self.qualified(node.func) & PROGRAM_CALLS):
            return self.program_names(node.args[0], active)
        try:
            return {PurePath(str(value)).name for value in self.reader.values(node)}
        except Unresolved:
            if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Div):
                return {PurePath(str(value)).name for value in self.reader.values(node.right)}
            binding = self.reader.binding(node) if isinstance(node, ast.Name) else None
            parent = self.reader.parents.get(binding)
            if binding not in active and isinstance(parent, ast.Assign) and parent.targets == [binding]:
                return self.program_names(parent.value, active | {binding})
            raise

    def option(self, item):
        """An item's one constant value, if it has one."""
        try:
            values = self.reader.values(item)
        except Unresolved:
            return None
        return next(iter(values)) if len(values) == 1 else None

    def string(self, node):
        """Whether a command is one string or path. A no relies on no parameter taken from calls."""
        called = set(self.reader.called)
        try:
            if all(isinstance(value, (str, PurePath)) for value in self.reader.values(node)):
                return True
        except Unresolved:
            pass
        self.reader.called = called
        return False

    def words(self, line):
        """The words of a shell command, which must be one constant string of plain words."""
        values = self.reader.values(line)
        value = next(iter(values)) if len(values) == 1 else None
        try:
            words = shlex.split(value) if isinstance(value, str) and SHELL_WORDS.fullmatch(value) else []
        except ValueError:
            words = []
        if not words:
            raise Unresolved(f'{sorted(map(str, values))} is not one command of plain words')
        return [ast.Constant(word) for word in words]

    def elements(self, node):
        """The items of a command list, each an expression; a starred part the
        rule cannot expand stays one item, which reduces to nothing."""
        if isinstance(node, (ast.List, ast.Tuple)):
            return [part for item in node.elts for part in (
                self.expanded(item) if isinstance(item, ast.Starred) else [item])]
        if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Add):
            return self.elements(node.left) + self.elements(node.right)
        if (isinstance(node, ast.Call) and ast.unparse(node.func) in ('list', 'tuple') and len(node.args) == 1
                and not node.keywords):
            return self.elements(node.args[0])
        if isinstance(node, ast.Name):
            return self.listed(node)
        raise Unresolved(f'{ast.unparse(node)} is not a command list')

    def expanded(self, item):
        try:
            return self.elements(item.value)
        except Unresolved:
            return [item]

    def listed(self, name):
        """The command list a name holds: one assignment, which only +=, append or extend grow at its end."""
        bindings = self.reader.bindings(name)
        parents = [self.reader.parents.get(binding) for binding in bindings]
        assigned = [parent for binding, parent in zip(bindings, parents) if isinstance(
            parent, (ast.Assign, ast.AnnAssign)) and parent.value is not None and getattr(
            parent, 'targets', [getattr(parent, 'target', None)]) == [binding]]
        grown = [parent for parent in parents if isinstance(parent, ast.AugAssign) and isinstance(parent.op, ast.Add)]
        if (len(assigned) != 1 or len(assigned) + len(grown) != len(bindings) or name.id in self.reader.declared
                or id(bindings) in self.listing):
            raise Unresolved(f'{name.id} is not one command list')
        for read in self.reader.reads.get(name.id, ()):
            try:
                same = self.reader.bindings(read) is bindings
            except Unresolved:
                same = False
            parent = self.reader.parents.get(read)
            if same and (isinstance(parent, ast.Subscript) and not isinstance(parent.ctx, ast.Load)
                         or isinstance(parent, ast.Attribute) and parent.attr in MUTATIONS):
                raise Unresolved(f'{name.id} changes in place')
            grown += [parent] if same and isinstance(parent, ast.Attribute) and parent.attr in GROWTH else []
        self.listing.add(id(bindings))
        try:
            return self.elements(assigned[0].value) + ([ast.Starred(name, ast.Load())] if grown else [])
        finally:
            self.listing.discard(id(bindings))

    def python(self, node):
        """Whether a list or tuple names an interpreter: first, or by a -c or -m
        option. A no relies on no parameter taken from calls."""
        if any(isinstance(item, ast.Constant) and item.value in ('-c', '-m') for item in node.elts[1:]):
            return True
        called = set(self.reader.called)
        try:
            if any(INTERPRETER.fullmatch(name) for name in self.program_names(node.elts[0])):
                return True
        except Unresolved:
            pass
        self.reader.called = called
        return False

    def command(self, items, line):
        """The project files a Python command runs, from the items after its
        interpreter: its script, its -m module or its -c code."""
        index = 0
        while index < len(items):
            values = self.reader.values(items[index])
            option = next(iter(values)) if len(values) == 1 else None
            if not (isinstance(option, str) and option.startswith('-') and option != '-'):
                return [self.file(value) for value in values]
            if option in ('-c', '-m'):
                if index + 1 == len(items):
                    raise Unresolved(f'{option} without an argument')
                following = self.reader.values(items[index + 1])
                if option == '-c':
                    return list(self.code(following, line))
                if any(str(value).partition('.')[0] in RUNNERS for value in following):
                    raise Unresolved(f'-m {sorted(following)[0]} runs code it is given')
                return [self.module(value) for value in following]
            index += 2 if option in ('-W', '-X', '--check-hash-based-pycs') else 1
        raise Unresolved('it runs no script')


def read(path):
    return reading(path, tuple(sorted(SOURCES.items())))


@functools.lru_cache(maxsize=None)
def reading(path, overlay):
    return Launching(path, parsed(path, dict(overlay)), dict(overlay))


def wrapping(path, name):
    """What wraps() reads for one project function. Within a call cycle the
    function in progress reads as no wrapper, and no reading that relied on
    that is kept."""
    key = (path, name, tuple(sorted(SOURCES.items())))
    if key in WRAPS:
        return WRAPS[key]
    if key in WRAPPING:
        CYCLIC.update(WRAPPING[WRAPPING.index(key):])
        return None, ()
    WRAPPING.append(key)
    try:
        found = read(key[0]).wraps(name)
    finally:
        WRAPPING.pop()
    if key in CYCLIC:
        CYCLIC.discard(key)
    else:
        WRAPS[key] = found
    return found


def closure(entries, follow_runs):
    """Every project Python file the entries import or load, transitively, and
    with follow_runs every script they run. A failure to resolve one fails, as
    does a function whose parameters the rule took from its own file's calls
    when another file that imports it refers to that function."""
    seen, pending, failures = {}, [Path(entry).resolve() for entry in entries], []
    while pending:
        path = pending.pop()
        if path not in seen and path.suffix == '.py':
            found = seen[path] = read(path).launched() if follow_runs else read(path)
            failures += found.failures + (found.run_failures if follow_runs else [])
            pending += [*found.imports, *(found.runs if follow_runs else ())]
    if failures:
        raise AssertionError('\n'.join(sorted(set(failures))))
    for path, found in seen.items():
        for other, by in seen.items():
            shared = found.reader.called & by.references
            if other != path and path in by.imports and shared:
                raise AssertionError(f'{relative(other)} may call {sorted(shared)} in {relative(path)}, whose '
                                     'parameters the rule took from calls in that file alone')
    return set(seen)
