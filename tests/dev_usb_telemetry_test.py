"""Bounded actual protocol parser controls; authored bytes, no device or SDK."""
import importlib.util
from pathlib import Path
import sys
import time
import zlib

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
spec = importlib.util.spec_from_file_location('telemetry_client', ROOT / 'tools/dev-usb.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def rejects(callback, text):
    try:
        callback()
    except (ValueError, OSError) as error:
        assert text in str(error), str(error)
    else:
        raise AssertionError('Expected bounded protocol failure')


def telemetry_validation():
    """Actual client parser, fragmented authored wire only; no serial or SDK."""
    class Peer(module.ProtocolClient):
        def __init__(self, tags=b'', terminal='RENDER_RETURN gen=2', wrong=False):
            super().__init__(0.1)
            self.boot, self.session, self.request_id = 12345, 2, 0x80000000
            self.tags, self.terminal, self.wrong = tags, terminal, wrong
            self.pending = b''
            self.writes = []
        def write_data(self, data, remaining):
            self.writes.append(data)
            identifier = int(data.split()[3])
            def frame(body):
                payload = f'!N1 12345 2 {identifier} {body}'.encode()
                return payload + f' *{zlib.crc32(payload):08x}\n'.encode()
            if b' PRESS ' in data:
                self.pending = (self.tags + frame('ACCEPTED') +
                    frame('WRONG' if self.wrong else 'CONSUMED') + frame(self.terminal) +
                    b'8279_ISLAND_POF\n')
            else:
                self.pending = self.tags + frame('STATUS ready=1')
        def read_data(self, remaining):
            data, self.pending = self.pending[:13], self.pending[13:]
            return data
    import base64
    import struct
    raw = struct.pack('<IHBB', 0xfffffff0, 65535, 128, 12 | 128)
    body = 'PANEL version=1 capacity=31 next=0 count=1 data=' + base64.b64encode(raw).decode()
    ring = module.decode_panel_ring(body)
    assert ring['entries'] == [{'sequence': 0xffffffff, 'ms': 0xfffffff0, 'duration_ms': 65535,
        'duration_saturated': True, 'mode': 'fast', 'reason': 'reader', 'outcome': 'fallback'}]
    for bad in (body.replace('count=1', 'count=32'), body.replace('next=0', 'next=4294967296'),
                body + ' private=title', body.replace('data=', 'data=AAAA'),
                body.replace('capacity=31', 'capacity=32')):
        rejects(lambda: module.decode_panel_ring(bad), '')
    invalid_id = struct.pack('<IHBB', 0, 1, 127, 0)
    rejects(lambda: module.decode_panel_ring('PANEL version=1 capacity=31 next=1 count=1 data=' +
            base64.b64encode(invalid_id).decode()), 'Unknown panel ring ID')
    tags = (b'private arbitrary log must not be retained\n'
            b'[driver] 8279_DIRECT_GRAY_DRF\n8279_ISLAND_POF\n'
            b'[EPD] displayBuffer mode=0 off=1\n'
            b'[EPD] displayBuffer mode=1 off=0\n'
            b'[EPD] displayBuffer mode=2 off=1\n'
            b'[EPD] displayBuffer mode=2 off=10\n'
            b'[EPD] displayBuffer mode=2 off=1-extra\n'
            b'prefix8279_DIRECT_GRAY_DRFsuffix\n')
    peer = Peer(tags)
    assert peer.request('PRESS next') == ['ACCEPTED', 'CONSUMED', 'RENDER_RETURN gen=2']
    assert [entry['tag'] for entry in peer.press_telemetry] == [
        '8279_DIRECT_GRAY_DRF', '8279_ISLAND_POF', '[EPD] displayBuffer mode=0 off=1',
        '[EPD] displayBuffer mode=1 off=0', '[EPD] displayBuffer mode=2 off=1']
    # Live panel text is redundant and must never fill/abort a PRESS audit.
    panel = Peer(b'Panel: ms=42 mode=fast reason=reader\n' * 2100)
    panel.timeout = 2
    panel.request('PRESS next')
    assert not panel.press_telemetry
    request, terminal = peer.audit[0], peer.audit[-1]
    for entry in peer.press_telemetry:
        assert set(entry) == {'boot', 'session', 'request_id', 'tag', 'monotonic'}
        assert (entry['boot'], entry['session'], entry['request_id']) == (12345, 2, request['request_id'])
        assert request['monotonic'] <= entry['monotonic'] <= terminal['monotonic']
    assert peer.active_press is None
    previous = list(peer.press_telemetry)
    peer.request('STATUS')
    assert peer.press_telemetry == previous  # Post-terminal and STATUS logs cannot classify a PRESS.
    buffered = Peer(b'ISLAND_POF\n[EPD] displayBuffer mode=2 off=0\n')
    buffered.buffer = bytearray(b'8279_DIRECT_GRAY_DRF\n8279_')
    buffered.request('PRESS next')
    assert [entry['tag'] for entry in buffered.press_telemetry] == ['[EPD] displayBuffer mode=2 off=0']
    no_visual = Peer(b'8279_DIRECT_GRAY_DRF\n', terminal='NO_VISUAL_CHANGE gen=1')
    assert no_visual.request('PRESS next')[-1] == 'NO_VISUAL_CHANGE gen=1'
    assert no_visual.press_telemetry[0]['tag'] == '8279_DIRECT_GRAY_DRF'  # Evaluator, never parser, decides eligibility.
    wrong = Peer(b'8279_ISLAND_POF\n', wrong=True)
    rejects(lambda: wrong.request('PRESS next'), 'completion unknown')
    assert len(wrong.writes) == 1 and wrong.active_press is None
    assert len(wrong.press_telemetry) == 1 and wrong.audit[-1]['body'] == 'WRONG'
    full = Peer(b'8279_ISLAND_POF\n' * (module.MAX_PRESS_TELEMETRY + 1))
    rejects(lambda: full.request('PRESS next'), 'completion unknown')
    assert len(full.press_telemetry) == module.MAX_PRESS_TELEMETRY
    assert len(full.writes) == 1 and full.active_press is None



def firmware_row_validation():
    """Number-only firmware rows survive presses and --listen; any other text is dropped."""
    class Peer(module.ProtocolClient):
        def __init__(self, pending=b''):
            super().__init__(0.1)
            self.boot, self.session, self.request_id = 12345, 2, 0x80000000
            self.pending = pending
        def write_data(self, data, remaining):
            identifier = int(data.split()[3])
            def frame(body):
                payload = f'!N1 12345 2 {identifier} {body}'.encode()
                return payload + f' *{zlib.crc32(payload):08x}\n'.encode()
            self.pending += (b'PC target: from=8 next=1\n' + frame('ACCEPTED') + frame('CONSUMED') +
                             b'PC admission: r=1 s=3 u=0\n' + frame('RENDER_RETURN gen=2'))
        def read_data(self, remaining):
            data, self.pending = self.pending[:13], self.pending[13:]
            if not data:
                time.sleep(min(remaining, 0.01))
            return data
    runtime = b'Runtime: kind=reader stack-size=16384 stack-min-free=7052 heap-min-free=50468\n'
    admission = b'Reader admission: free=139000 largest=114676 required=128324 block=49171 credit=1200\n'
    census = b'PC census: o=1 r=2 n=0,4,40,40,48 b=123 ms=88 u=0\n'
    crossink = b'CROSSINK result=done sources=3 bound=2 ms=410\n'
    library = (b'LIBRARY result=ok books=60 outside=12 same=3 full=0 names=1 skipped=0 stopped=0 '
               b'entries=240 dirs=9 cmp=3 list=built walks=2 runs=2 ms=420\n'
               # Firmware before NEWT-80 has no list, walks or runs.
               b'LIBRARY result=ok books=65 outside=0 same=0 full=0 names=0 skipped=0 stopped=0 '
               b'entries=145 dirs=3 cmp=0 ms=77\n')
    cover = (b'COVER work=cached result=0 elapsed_ms=56 reads=15 read_bytes=3412 writes=0 write_bytes=0 opens=1 '
             b'path_stats=1 min_free_local=144112 min_largest_observed=114676 monitor_start=0 monitor_stop=-1 '
             b'stack_min_free=7052\n'
             b'COVER_CACHE scanned=4 art_shape=3 noart_shape=1 tmp_names=0 other=0 errors=0 limited=0 interrupted=0 '
             b'skipped=0\n'
             b'HOME_COVER eligible=1 attempted=1 step=6 ready=0 painted=0\n')
    page_turn = (b'Reader action: schema=4 total=228318 residual=4386 flags=0 actions=1 route=5 miss=2\n'
                 b'Reader action us: v=0,0,71894,0,6,0,586,58125,93211\n'
                 b'Reader open us: select=12 progress=34 source=56 bind=78 commit=90 device=11\n'
                 b'Reader close us: home=1 progress=2 release=3 exit=4 destination=5 device=6\n'
                 b'Reader action rows: emitted=7 expected=7\n'
                 b'Reader turn: pages=129 layout_us=71200 heap_min=50468 largest=49152 stack_min=6120\n'
                 b'PC admit: at=21879 r=3 u=127\n'
                 b'PC start: at=21879 from=0 next=0\n'
                 b'PC grant: warm=0 census_us=14250 files=12 outcome=0 units=127 entries=244\n'
                 b'PC seg: idx=1 pages=128 bytes=175104 layout_us=1240000 tee_us=98000 write_us=71000 fin_us=210000 commit_us=8400\n'
                 b'PC seal: idx=1 read=42000 crc=55000 dir=29000 sync=18000 verify=64000 heap=48120 largest=45056 stack=5980\n'
                 b'PC finish: at=32065 status=0 interrupted=0\n')
    dropped = (b'PC census: o=1 r=2 n=0,4,40,40,48 b=123 ms=88 u=0 title=Some Book\n'  # extra text
               b'PC target: from=chapter next=1\n'  # non-numeric field
               b'Runtime: kind=book-title stack-size=1 stack-min-free=1 heap-min-free=1\n'  # unknown kind
               b'PC admission: r=01 s=3 u=0\n'  # non-canonical number
               b'PC admission: r=1 s=3 u=99999999999999999999999\n'  # unbounded number
               b'x PC target: from=8 next=1\n'  # not a whole line
               b'CROSSINK result=Some Book sources=3 bound=2 ms=410\n'  # unknown result
               b'LIBRARY result=ok books=1 outside=1 same=0 full=0 names=0 skipped=0 stopped=0 '
               b'entries=3 dirs=1 cmp=0 list=kept walks=1 runs=0 ms=5 name=/Some Book.epub\n'  # a name
               b'Reader admission: free=1 largest=1 required=1 block=1 credit=1 book=Some Book\n'  # extra text
               b'HOME_COVER eligible=1 attempted=1 step=8 ready=0 painted=0\n'  # unknown step
               b'HOME_COVER eligible=1 attempted=1 step=7 ready=1 painted=1 name=Some Book\n'  # a name
               b'Reader turn: pages=1 layout_us=2 heap_min=3 largest=4 stack_min=5 book=Some Book\n'  # extra text
               b'PC grant: warm=2 census_us=1 files=0 outcome=0 units=1 entries=1\n'  # invalid flag
               b'PC finish: at=1 status=0 interrupted=2\n'  # invalid flag
               b'PC seg: idx=01 pages=1 bytes=2 layout_us=3 tee_us=4 write_us=5 fin_us=6 commit_us=7\n'  # non-canonical
               b'PC seal: idx=0 read=1 crc=2 dir=3 sync=4 verify=5 heap=6 largest=7 stack=8 title=Some Book\n')
    peer = Peer()
    assert peer.request('PRESS next') == ['ACCEPTED', 'CONSUMED', 'RENDER_RETURN gen=2']
    peer.pending = dropped + census + crossink + library + runtime + admission + cover + page_turn
    module.listen(peer.record, 0.3, module.ProtocolTimeout)
    rows = [{key: value for key, value in row.items() if key != 'monotonic'} for row in peer.firmware_rows.rows]
    assert rows == [{'row': 'target', 'from': 8, 'next': 1}, {'row': 'admission', 'r': 1, 's': 3, 'u': 0},
                    {'row': 'census', 'o': 1, 'r': 2, 'unknown': 0, 'page': 4, 'semantic': 40, 'progress': 40,
                     'identity': 48, 'b': 123, 'ms': 88, 'u': 0},
                    {'row': 'crossink', 'result': 'done', 'sources': 3, 'bound': 2, 'ms': 410},
                    {'row': 'library', 'result': 'ok', 'books': 60, 'outside': 12, 'same': 3, 'full': 0, 'names': 1,
                     'skipped': 0, 'stopped': 0, 'entries': 240, 'dirs': 9, 'cmp': 3, 'list': 'built', 'walks': 2,
                     'runs': 2, 'ms': 420},
                    {'row': 'library', 'result': 'ok', 'books': 65, 'outside': 0, 'same': 0, 'full': 0, 'names': 0,
                     'skipped': 0, 'stopped': 0, 'entries': 145, 'dirs': 3, 'cmp': 0, 'ms': 77},
                    {'row': 'runtime', 'kind': 'reader', 'stack_size': 16384, 'stack_min_free': 7052,
                     'heap_min_free': 50468},
                    {'row': 'reader_admission', 'free': 139000, 'largest': 114676, 'required': 128324,
                     'block': 49171, 'credit': 1200},
                    {'row': 'cover', 'work': 'cached', 'result': 0, 'elapsed_ms': 56, 'reads': 15, 'read_bytes': 3412,
                     'writes': 0, 'write_bytes': 0, 'opens': 1, 'path_stats': 1, 'min_free_local': 144112,
                     'min_largest_observed': 114676, 'monitor_start': 0, 'monitor_stop': -1, 'stack_min_free': 7052},
                    {'row': 'cover_cache', 'scanned': 4, 'art_shape': 3, 'noart_shape': 1, 'tmp_names': 0, 'other': 0,
                     'errors': 0, 'limited': 0, 'interrupted': 0, 'skipped': 0},
                    {'row': 'home_cover', 'eligible': 1, 'attempted': 1, 'step': 6, 'ready': 0, 'painted': 0},
                    {'row': 'reader_action', 'schema': 4, 'total': 228318, 'residual': 4386, 'flags': 0,
                     'actions': 1, 'route': 5, 'miss': 2},
                    {'row': 'reader_action_us', 'receipt': 0, 'verify': 0, 'ordinary': 71894, 'projection': 0,
                     'metadata': 6, 'progress': 0, 'cache': 586, 'composition': 58125, 'display': 93211},
                    {'row': 'reader_open_us', 'select': 12, 'progress': 34, 'source': 56, 'bind': 78,
                     'commit': 90, 'device': 11},
                    {'row': 'reader_close_us', 'home': 1, 'progress': 2, 'release': 3, 'exit': 4,
                     'destination': 5, 'device': 6},
                    {'row': 'reader_action_rows', 'emitted': 7, 'expected': 7},
                    {'row': 'reader_turn', 'pages': 129, 'layout_us': 71200, 'heap_min': 50468, 'largest': 49152,
                     'stack_min': 6120},
                    {'row': 'pc_admit', 'at': 21879, 'r': 3, 'u': 127},
                    {'row': 'pc_start', 'at': 21879, 'from': 0, 'next': 0},
                    {'row': 'pc_grant', 'warm': 0, 'census_us': 14250, 'files': 12, 'outcome': 0, 'units': 127,
                     'entries': 244},
                    {'row': 'pc_seg', 'idx': 1, 'pages': 128, 'bytes': 175104, 'layout_us': 1240000,
                     'tee_us': 98000, 'write_us': 71000, 'fin_us': 210000, 'commit_us': 8400},
                    {'row': 'pc_seal', 'idx': 1, 'read': 42000, 'crc': 55000, 'dir': 29000, 'sync': 18000,
                     'verify': 64000, 'heap': 48120, 'largest': 45056, 'stack': 5980},
                    {'row': 'pc_finish', 'at': 32065, 'status': 0, 'interrupted': 0}], rows
    assert 'Book' not in repr(peer.firmware_rows.report())
    capped = Peer(census * 259)
    module.listen(capped.record, 0.5, module.ProtocolTimeout)
    assert capped.firmware_rows.report()['firmware_rows_dropped'] == 3
    assert len(capped.firmware_rows.rows) == 256
    framed = Peer()
    framed.request('PRESS next')
    framed.pending = b'!N1 12345 2 7 STATUS ready=1 *' + f'{zlib.crc32(b"!N1 12345 2 7 STATUS ready=1"):08x}'.encode() + b'\n'
    rejects(lambda: module.listen(framed.record, 0.3, module.ProtocolTimeout), 'Unexpected protocol record while listening')
    started = time.monotonic()
    module.listen(Peer().record, 0.2, module.ProtocolTimeout)
    assert 0.2 <= time.monotonic() - started < 1


if __name__ == '__main__':
    telemetry_validation()
    firmware_row_validation()
    print('PASS: number-only firmware rows and --listen; bounded recognized PRESS telemetry, literal tags, fragments, windows, partial failures and audit cap; no device access')
