"""Frozen cache-owner transcripts, keyed by the complete synthetic input bytes."""
from pathlib import Path
import json
import sys
_RUST = next(p for a in Path(__file__).resolve().parents for p in (a, a / 'rust') if (p / 'expected.py').is_file())
sys.path.insert(0, str(_RUST))
import expected


def request(arguments):
    args = list(map(str, arguments))
    if args[0] == '--session':
        # Work directories are fresh output sinks, not input authority.
        return dict(command='session', book_hex=Path(args[1]).read_bytes().hex(),
                    mode=args[2], orientation=args[3], settings=args[4])
    return dict(command='cache', book_hex=Path(args[0]).read_bytes().hex(),
                mode=args[1], orientation=args[2], settings=args[3])


class Corpus:
    """Consume the whole positive corpus in order before control lookups."""
    def __init__(self, path):
        import zlib
        self.path = Path(path)
        document = json.loads(zlib.decompress(self.path.read_bytes()))
        self.requests = document['requests']
        self.outputs = expected.load(self.path, self.requests)
        self.observed = []
        self.complete = False

    def observe(self, arguments):
        wanted = request(arguments)
        at = len(self.observed)
        if self.complete or at == len(self.requests) or wanted != self.requests[at]:
            raise ValueError('changed ordered cache-owner corpus')
        self.observed.append(wanted)
        return self.outputs[at]

    def finish(self):
        # Bind actual calls, rather than asking the fixture to validate itself.
        expected.load(self.path, self.observed)
        self.complete = True

    def witness(self, arguments):
        if not self.complete:
            raise ValueError('cache-owner controls require the complete positive corpus')
        wanted = request(arguments)
        try:
            return self.outputs[self.requests.index(wanted)]
        except ValueError:
            raise ValueError('missing recorded cache-owner witness') from None


def observe(command):
    return command[0].observe(command[1:])


def witness(command):
    return command[0].witness(command[1:])
