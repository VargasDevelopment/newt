# SPDX-License-Identifier: MIT
"""Shared rlib discovery and native C link list for host startup tests."""
from pathlib import Path

HOST = 'x86_64-unknown-linux-gnu'
NATIVE_LINK = ('expat_xmlparse', 'expat_xmltok', 'expat_xmlrole', 'unibreak_impl', 'bridge')


def rlibs(deps, require):
    found = {}
    for path in sorted(Path(deps).glob('libnewt_*.rlib')):
        name = path.name[3:].split('-')[0]
        require(name not in found, 'one rlib per crate: ' + name)
        found[name] = path
    return found
