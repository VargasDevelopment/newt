"""Stable identities over all positive records plus every mutation witness."""
import hashlib


def fixtures(corpus, controls):
    cases = []
    occurrences = {}
    for record in corpus.records():
        identity = hashlib.sha256(record['input'].encode()).hexdigest()
        occurrence = occurrences.get(identity, 0)
        occurrences[identity] = occurrence + 1
        cases.append(dict(record, name=f'xml-{identity}-{occurrence}'))
    existing = {case['input'] for case in cases}
    for control in controls:
        if control['witness'] not in existing:
            cases.append({'name': 'witness-' + control['name'], 'input': control['witness']})
            existing.add(control['witness'])
    return cases
