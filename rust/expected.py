"""Read checked-in reference outputs; test runs never regenerate them."""
import hashlib
import json
from pathlib import Path
import zlib


def input_digest(requests):
    encoded = json.dumps(requests, sort_keys=True, separators=(',', ':'),
                         ensure_ascii=True, allow_nan=False).encode('ascii')
    return hashlib.sha256(encoded).hexdigest()


def load_document(path, requests):
    path = Path(path)
    raw = path.read_bytes()
    document = json.loads(zlib.decompress(raw) if path.suffix == '.zlib' else raw)
    if (not isinstance(document, dict) or type(document.get('schema')) is not int
            or document['schema'] != 1 or document.get('input_sha256') != input_digest(requests)
            or not isinstance(document.get('outputs'), list)
            or len(document['outputs']) != len(requests)):
        raise ValueError('Expected-output fixture does not match the complete case list: ' + str(path))
    return document


def load(path, requests):
    return load_document(path, requests)['outputs']


def load_literals(path, requests, answers):
    if len(requests) != len(answers):
        raise ValueError('Incomplete literal reference corpus')
    bound = [dict(request, expected_sha256=input_digest([answer]))
             for request, answer in zip(requests, answers)]
    captured = load(path, bound)
    if captured != [hashlib.sha256(answer.encode('utf-8')).hexdigest() for answer in answers]:
        raise ValueError('Literal reference differs from captured native output')
    return answers
