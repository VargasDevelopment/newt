"""Both sanitizer classes must fail before a valid-looking reply can escape."""
import os
import subprocess
import sys

for mode, diagnostic in [('u', b'runtime error:'), ('a', b'AddressSanitizer')]:
    result = subprocess.run([sys.argv[1], mode], stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, timeout=5, check=False,
                            env={**os.environ, 'ASAN_OPTIONS': os.environ.get('ASAN_OPTIONS', '') + ':symbolize=0'})
    if result.returncode == 0 or diagnostic not in result.stderr or result.stdout:
        raise RuntimeError(f'{mode}: fatal sanitizer control failed: exit={result.returncode}')
    print(f'PASS fatal sanitizer {mode}: exit={result.returncode}', flush=True)
