"""Number-only firmware rows kept from the DEV serial stream."""
import re
import time

# The firmware's number-only rows the bench needs: the loop's stack and heap
# lows, the idle page cache's census, admission and target, the CrossInk
# import's result, the library scan's counts, the reader's heap admission
# at an open (#325), the cover producer's step and cache census, and Home's
# one cover read per visit (#316). Whole-line matches only; any other text,
# such as a name, is never retained.
NUMBER = rb'(0|[1-9][0-9]{0,19})'
SIGNED = rb'(0|-?[1-9][0-9]{0,9})'
FLAG = rb'([01])'
FIRMWARE_ROWS = {
    'runtime': re.compile(rb'Runtime: kind=(?P<kind>reader|stage-regional|stage-full-direct|stage-full-overlay|'
                          rb'stage-monochrome|none) stack-size=(?P<stack_size>%s) stack-min-free=(?P<stack_min_free>%s) '
                          rb'heap-min-free=(?P<heap_min_free>%s)' % (NUMBER, NUMBER, NUMBER)),
    'census': re.compile(rb'PC census: o=(?P<o>%s) r=(?P<r>%s) n=(?P<unknown>%s),(?P<page>%s),(?P<semantic>%s),'
                         rb'(?P<progress>%s),(?P<identity>%s) b=(?P<b>%s) ms=(?P<ms>%s) u=(?P<u>%s)' % ((NUMBER,) * 10)),
    'admission': re.compile(rb'PC admission: r=(?P<r>%s) s=(?P<s>%s) u=(?P<u>%s)' % (NUMBER, NUMBER, NUMBER)),
    'target': re.compile(rb'PC target: from=(?P<from>%s) next=(?P<next>%s)' % (NUMBER, NUMBER)),
    'crossink': re.compile(rb'CROSSINK result=(?P<result>done|nothing|cancelled|unavailable|unconfirmed|unsupported) '
                           rb'sources=(?P<sources>%s) bound=(?P<bound>%s) ms=(?P<ms>%s)' % (NUMBER, NUMBER, NUMBER)),
    'library': re.compile(rb'LIBRARY result=(?P<result>ok|unavailable) books=(?P<books>%s) outside=(?P<outside>%s) '
                          rb'same=(?P<same>%s) full=(?P<full>%s) names=(?P<names>%s) skipped=(?P<skipped>%s) '
                          rb'stopped=(?P<stopped>%s) entries=(?P<entries>%s) dirs=(?P<dirs>%s) cmp=(?P<cmp>%s) '
                          # NEWT-80 adds list, walks and runs; older firmware's row still decodes.
                          rb'(?:list=(?P<list>none|kept|built|ram) walks=(?P<walks>%s) runs=(?P<runs>%s) )?'
                          rb'ms=(?P<ms>%s)' % ((NUMBER,) * 13)),
    'reader_admission': re.compile(rb'Reader admission: free=(?P<free>%s) largest=(?P<largest>%s) '
                                   rb'required=(?P<required>%s) block=(?P<block>%s) credit=(?P<credit>%s)'
                                   % ((NUMBER,) * 5)),
    'cover': re.compile(rb'COVER work=(?P<work>cached|attempt) result=(?P<result>%s) elapsed_ms=(?P<elapsed_ms>%s) '
                        rb'reads=(?P<reads>%s) read_bytes=(?P<read_bytes>%s) writes=(?P<writes>%s) '
                        rb'write_bytes=(?P<write_bytes>%s) opens=(?P<opens>%s) path_stats=(?P<path_stats>%s) '
                        rb'min_free_local=(?P<min_free_local>%s) min_largest_observed=(?P<min_largest_observed>%s) '
                        rb'monitor_start=(?P<monitor_start>%s) monitor_stop=(?P<monitor_stop>%s) '
                        rb'stack_min_free=(?P<stack_min_free>%s)' % ((NUMBER,) * 10 + (SIGNED, SIGNED, NUMBER))),
    'cover_cache': re.compile(rb'COVER_CACHE scanned=(?P<scanned>%s) art_shape=(?P<art_shape>%s) '
                              rb'noart_shape=(?P<noart_shape>%s) tmp_names=(?P<tmp_names>%s) other=(?P<other>%s) '
                              rb'errors=(?P<errors>%s) limited=(?P<limited>%s) interrupted=(?P<interrupted>%s) '
                              rb'skipped=(?P<skipped>%s)' % ((NUMBER,) * 6 + (FLAG,) * 3)),
    # step: 0 none, 1 card unmounted, 2 I/O loan refused, 3 no memory,
    # 4 no valid identity, 5 known no cover, 6 cover file not ready, 7 ready.
    'home_cover': re.compile(rb'HOME_COVER eligible=(?P<eligible>%s) attempted=(?P<attempted>%s) step=(?P<step>[0-7]) '
                             rb'ready=(?P<ready>%s) painted=(?P<painted>%s)' % ((FLAG,) * 4)),
}
MAX_FIRMWARE_ROWS = 256
MAX_LISTEN_SECONDS = 600


def decode_firmware_row(line):
    """Return one kept row's fields, or None for any other line."""
    for kind, pattern in FIRMWARE_ROWS.items():
        match = pattern.fullmatch(line)
        if match:
            return {'row': kind, **{name: int(value) if re.fullmatch(rb'-?[0-9]+', value) else value.decode('ascii')
                                    for name, value in match.groupdict().items() if value is not None}}
    return None


class FirmwareRows:
    """Kept rows in arrival order, capped; the rest are only counted."""
    def __init__(self):
        self.rows, self.dropped = [], 0

    def keep(self, line):
        row = decode_firmware_row(line)
        if row is not None and len(self.rows) >= MAX_FIRMWARE_ROWS:
            self.dropped += 1
        elif row is not None:
            self.rows.append({**row, 'monotonic': time.monotonic()})

    def report(self):
        return {'firmware_rows': self.rows, 'firmware_rows_dropped': self.dropped}


def listen(record, seconds, quiet):
    """Keep the open port (reopening resets the X3) and read only unframed rows.

    `record` returns the next protocol record, keeping rows on the way, and
    raises `quiet` when its deadline passes without one."""
    try:
        unexpected = record(time.monotonic() + seconds)
    except quiet:
        return
    raise ValueError('Unexpected protocol record while listening: ' + unexpected[3].partition(' ')[0])
