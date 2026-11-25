"""Native bounds and legacy SAX equivalence, reusing the XML family's objects."""
import re


def check(root, packet, output, runner, tools, objects, flags, expat, positive_only):
    compiler = [tools['clang++'], '--no-default-config']
    fixture = output/'entity-limits.o'
    runner.run([*compiler, '-x', 'c', '-std=c99', *flags, '-I', expat,
                '-c', packet/'entity_limits.c', '-o', fixture], root)
    settings = ('XML_SetBillionLaughsAttackProtectionActivationThreshold',
                'XML_SetBillionLaughsAttackProtectionMaximumAmplification')
    def link(bridge, name):
        binary = output/name
        runner.run([*compiler, '-fuse-ld='+str(tools['native-ld']), *flags,
                    fixture, *(objects[n] for n in ('parse', 'tok', 'role')), bridge,
                    *(f'-Wl,--wrap={n}' for n in (*settings, 'XML_ParserFree')),
                    '-o', binary], root)
        return binary
    def result(binary, case):
        rows = runner.execute([binary, case], root).splitlines()
        assert rows.pop() == 'PASS entity limits', rows
        if case == 'ordinary':
            assert len(rows) == 1 and re.fullmatch(
                r'PASS ordinary chapter: [0-9]+ bytes, SAX trace byte-identical to main', rows[0]), rows
            return True
        if case.startswith('fail-'):
            assert len(rows) == 1 and re.fullmatch(r'SETUP [01] [01]', rows[0]), rows
            return rows == ['SETUP 1 1']
        expand_rows = [row for row in rows if row.startswith('EXPAND ')]
        boundary_rows = [row for row in rows if row.startswith('BOUNDARY ')]
        expected_boundary = 4 if case == 'factor' else 0
        assert len(expand_rows) == 8 and len(boundary_rows) == expected_boundary and len(expand_rows) + len(boundary_rows) == len(rows), rows
        limit = 300000 if case == 'factor' else 16384
        for row in expand_rows:
            assert re.fullmatch(r'EXPAND [01] [01] (1|2048) [01] [0-9]+ [0-9]+ [0-9]+ [0-9]+', row), row
            _, nested, attr, chunk, ok, text_bytes, attr_bytes, feeds, total = row.split()
            chunk_n, feeds_n, total_n = int(chunk), int(feeds), int(total)
            max_feeds = (total_n + chunk_n - 1) // chunk_n + 1
            if ok != '0' or int(text_bytes) >= limit or int(attr_bytes) != 0 or feeds_n > max_feeds:
                return False
            # Flat element threshold expansion aborts before reading the rest of the input.
            if case == 'threshold' and nested == '0' and attr == '0' and feeds_n >= (total_n + chunk_n - 1) // chunk_n:
                return False
        for row in boundary_rows:
            assert re.fullmatch(r'BOUNDARY [01] (1|2048) [01] [0-9]+ [0-9]+ [0-9]+ [0-9]+', row), row
            _, attr, chunk, ok, text_bytes, attr_bytes, feeds, total = row.split()
            chunk_n, feeds_n, total_n = int(chunk), int(feeds), int(total)
            max_feeds = (total_n + chunk_n - 1) // chunk_n + 1
            if ok != '0' or int(text_bytes) >= 150 * 1024 or int(attr_bytes) != 0 or feeds_n > max_feeds:
                return False
        return True
    binary = link(objects['bridge'], 'entity-limits')
    for case in ('ordinary', 'threshold', 'factor', 'fail-threshold', 'fail-factor'):
        assert result(binary, case), case
    if not positive_only:
        pristine = (root/'target-expat/native/expat_bridge.c').read_text()
        controls = (
            ('threshold', settings[0]+'(parser, 16384)', 'XML_TRUE'),
            ('factor', settings[1]+'(parser, 8.0f)', 'XML_TRUE'),
            ('fail-threshold', '!'+settings[0]+'(parser, 16384)', '('+settings[0]+'(parser, 16384), 0)'),
            ('fail-factor', '!'+settings[1]+'(parser, 8.0f)', '('+settings[1]+'(parser, 8.0f), 0)'),
        )
        for case, good, bad in controls:
            assert pristine.count(good) == 1
            source, obj = output/(case+'.c'), output/(case+'.o')
            source.write_text(pristine.replace(good, bad))
            runner.run([*compiler, '-x', 'c', '-std=c99', *flags, '-I', expat, '-c', source, '-o', obj], root)
            assert not result(link(obj, 'entity-control-'+case), case), 'survived: '+case
    print('PASS entity limits: shared native constructor, ordinary chapter equals main; four independent controls')
