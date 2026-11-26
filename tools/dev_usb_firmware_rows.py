"""Number-only firmware rows kept from the DEV serial stream."""
import re
import time

# The firmware's number-only rows the bench needs: the loop's stack and heap
# lows, the idle page cache's census, admission and target, the CrossInk
# import's result, the library scan's counts, the reader's heap admission
# at an open (#325), the cover producer's step and cache census, Home's
# one cover read per visit (#316), and the DEV USB link totals (#170). Whole-line matches only; any other text,
# such as a name, is never retained.
NUMBER = rb'(0|[1-9][0-9]{0,19})'
SIGNED = rb'(0|-?[1-9][0-9]{0,9})'
FLAG = rb'([01])'
FIRMWARE_ROWS = {
    'put_stats': re.compile(rb'PUT_STATS reason=(?P<reason>%s) id=(?P<id>%s) offset=(?P<offset>%s) '
                            rb'received=(?P<received>%s) expected=(?P<expected>%s) bytes=(?P<bytes>%s) '
                            rb'empty=(?P<empty>%s) pauses=(?P<pauses>%s) pause_ms=(?P<pause_ms>%s) '
                            rb'rx_ms=(?P<rx_ms>%s) write_ms=(?P<write_ms>%s) ack_ms=(?P<ack_ms>%s) '
                            rb'commit_ms=(?P<commit_ms>%s)' % ((NUMBER,) * 13)),
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
    'reader_action': re.compile(rb'Reader action: schema=(?P<schema>%s) total=(?P<total>%s) residual=(?P<residual>%s) '
                                rb'flags=(?P<flags>%s) actions=(?P<actions>%s) route=(?P<route>%s) miss=(?P<miss>%s)'
                                % ((NUMBER,) * 7)),
    'reader_action_us': re.compile(rb'Reader action us: v=(?P<receipt>%s),(?P<verify>%s),(?P<ordinary>%s),'
                                   rb'(?P<projection>%s),(?P<metadata>%s),(?P<progress>%s),(?P<cache>%s),'
                                   rb'(?P<composition>%s),(?P<display>%s)' % ((NUMBER,) * 9)),
    'reader_open_us': re.compile(rb'Reader open us: select=(?P<select>%s) progress=(?P<progress>%s) '
                                 rb'source=(?P<source>%s) bind=(?P<bind>%s) commit=(?P<commit>%s) device=(?P<device>%s)'
                                 % ((NUMBER,) * 6)),
    'reader_close_us': re.compile(rb'Reader close us: home=(?P<home>%s) progress=(?P<progress>%s) '
                                  rb'release=(?P<release>%s) exit=(?P<exit>%s) destination=(?P<destination>%s) '
                                  rb'device=(?P<device>%s)' % ((NUMBER,) * 6)),
    'reader_action_rows': re.compile(rb'Reader action rows: emitted=(?P<emitted>%s) expected=(?P<expected>%s)'
                                     % ((NUMBER,) * 2)),
    'reader_turn': re.compile(rb'Reader turn: pages=(?P<pages>%s) layout_us=(?P<layout_us>%s) '
                              rb'heap_min=(?P<heap_min>%s) largest=(?P<largest>%s) stack_min=(?P<stack_min>%s)'
                              % ((NUMBER,) * 5)),
    'pc_admit': re.compile(rb'PC admit: at=(?P<at>%s) r=(?P<r>%s) u=(?P<u>%s)' % ((NUMBER,) * 3)),
    'pc_start': re.compile(rb'PC start: at=(?P<at>%s) from=(?P<from>%s) next=(?P<next>%s)' % ((NUMBER,) * 3)),
    'pc_finish': re.compile(rb'PC finish: at=(?P<at>%s) status=(?P<status>%s) interrupted=(?P<interrupted>%s)'
                            % (NUMBER, NUMBER, FLAG)),
    'pc_grant': re.compile(rb'PC grant: warm=(?P<warm>%s) census_us=(?P<census_us>%s) files=(?P<files>%s) '
                           rb'outcome=(?P<outcome>%s) units=(?P<units>%s) entries=(?P<entries>%s)'
                           % (FLAG, NUMBER, NUMBER, NUMBER, NUMBER, NUMBER)),
    'pc_seg': re.compile(rb'PC seg: idx=(?P<idx>%s) pages=(?P<pages>%s) bytes=(?P<bytes>%s) '
                         rb'layout_us=(?P<layout_us>%s) tee_us=(?P<tee_us>%s) write_us=(?P<write_us>%s) '
                         rb'fin_us=(?P<fin_us>%s) commit_us=(?P<commit_us>%s)' % ((NUMBER,) * 8)),
    'usb_link': re.compile(rb'USB link: blips=(?P<blips>%s) lost=(?P<lost>%s) latched=(?P<latched>%s) '
                           rb'skipped=(?P<skipped>%s) end_link=(?P<end_link>%s) end_idle=(?P<end_idle>%s) '
                           rb'end_tx=(?P<end_tx>%s) end_tail=(?P<end_tail>%s)' % ((NUMBER,) * 8)),
    'pc_seal': re.compile(rb'PC seal: idx=(?P<idx>%s) read=(?P<read>%s) crc=(?P<crc>%s) dir=(?P<dir>%s) '
                          rb'sync=(?P<sync>%s) verify=(?P<verify>%s) heap=(?P<heap>%s) largest=(?P<largest>%s) '
                          rb'stack=(?P<stack>%s)' % ((NUMBER,) * 9)),
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
