"""Pinned native chapter traces against the single shared allocation-free core."""
import hashlib
import importlib.util
import json
from pathlib import Path
import shlex
import shutil
import sys
import tempfile
import time
from types import SimpleNamespace


def load(path,name):
    spec=importlib.util.spec_from_file_location(name,path)
    module=importlib.util.module_from_spec(spec)
    sys.modules[name]=module
    spec.loader.exec_module(module)
    return module


def check(gate,snapshot,positive_only):
    candidate=snapshot/'rust'
    frozen=candidate/'tests/chapter_layout'
    original=gate.ROOT/'vendor/freeink-sdk'
    support=load(candidate/'tests/inflate/run_support.py','chapter_shared_support')
    support.ROOT=gate.ROOT
    support.install_signal_handlers()
    tools=support.verify_tools()
    live_inventory=support.source_inventory(candidate)
    source_seals=[support.SourceSeal(candidate,live_inventory)]
    class SourceRun(support.Run):
        def run(self,*arguments,**keywords):
            with support.source_commands(source_seals):
                return super().run(*arguments,**keywords)
    output=Path(tempfile.mkdtemp(prefix='chapter-family-',dir=gate.SCRATCH))
    runner=SourceRun(output,tools,time.monotonic()+480)
    runner.env['RUSTFLAGS']='-Dwarnings -Ccodegen-units=1'
    args=SimpleNamespace(core_package='newt-core')
    root=gate.ROOT
    require=support.require
    report={}
    names=['cases','protocol','expectations','xml_protocol','xml_vectors']
    previous_modules={name:sys.modules.get(name) for name in names}
    previous_path=list(sys.path)
    try:
        provenance=json.loads((frozen/'PROVENANCE.json').read_text())
        require(provenance['sdk_commit']=='cd6f5b831c8fad899723b6d406586f00c575f021' and provenance['newt_commit']=='bc9ffda52b85f277e46c63718ac88d62fba5d368','source pins changed')
        patch_policy=json.loads((frozen/'tests/freeink-producer-policy.json').read_text())
        patch_receipt={'patch_sha256':patch_policy['patch_sha256'], 'authority':'archived producer policy; expected observations checked in'}
        native_inputs=json.loads((candidate/'tests/phased_reader/native-inputs.json').read_text())
        require(native_inputs['reference']==provenance['newt_commit'] and native_inputs['sdk']==provenance['sdk_commit'],'native product pins')
        for relative,sha in native_inputs['sha256'].items():
            require(support.digest(original/relative)==sha,'native product source: '+relative)
        book_rel=Path('libs/book/FreeInkBook')
        book=original/book_rel
        flags=['-O1','-g','-fsanitize=address,undefined','-fno-sanitize-recover=all','-fno-omit-frame-pointer']
        def includes(sdk):
            return [arg for p in [sdk/book_rel/'include',sdk/'libs/font/FreeInkFont/include',sdk/book_rel/'third_party/expat',sdk/book_rel/'third_party/libunibreak'] for arg in ['-I',p]]
        objects={}
        for name,source in [('parse',book/'src/vendor/expat_xmlparse.c'),('tok',book/'src/vendor/expat_xmltok.c'),('role',book/'src/vendor/expat_xmlrole.c'),('break',book/'src/vendor/unibreak_impl.c'),('bridge',candidate/'target-expat/native/expat_bridge.c')]:
            obj=output/(name+'.o')
            runner.run([tools['clang++'],'--no-default-config','-x','c','-std=c99',*flags,*includes(original),'-c',source,'-o',obj],root)
            objects[name]=obj
        native=['-Dwarnings','-Ccodegen-units=1','-C','linker='+str(tools['clang++']),'-C','link-arg=--no-default-config','-C','link-arg=-fsanitize=address,undefined','-C','link-arg=-fno-sanitize-recover=all']
        for n in ['parse','tok','role','bridge','break']: native+=['-C','link-arg='+str(objects[n])]
        runner.env['RUSTFLAGS']=shlex.join(native)
        def rust(ws,target,tests=False):
            if tests: runner.cargo(ws,target/'tests','test','-p',args.core_package)
            artifacts={}
            for binary in ['chapter-host','chapter-xml-host']:
                raw=runner.cargo(ws,target/binary,'build','-p','newt-parity','--bin',binary,'--features','xml-native','--message-format=json')
                fresh=set()
                for row in raw.splitlines():
                    msg=json.loads(row)
                    if msg.get('reason')=='compiler-artifact' and msg['target']['name'] in (args.core_package.replace('-','_'),binary):
                        require(msg['fresh'] is False,'core/transport must compile freshly')
                        fresh.add(msg['target']['name'])
                        if msg.get('executable'): artifacts[binary]=Path(msg['executable'])
                require(fresh=={args.core_package.replace('-','_'),binary},'actual core+host artifact missing')
            return artifacts
        binaries=rust(candidate,output/'baseline-target')
        native_messages=runner.cargo(candidate,output/'native-tests','test','-p','newt-parity','--bin','chapter-xml-host','--features','xml-native','--no-run','--message-format=json')
        test_executables=[item['executable'] for line in native_messages.splitlines()
                          if (item:=json.loads(line)).get('reason')=='compiler-artifact'
                          and item.get('executable') and item.get('profile',{}).get('test')]
        require(len(test_executables)==1,'chapter native test target absent')
        native_test=runner.execute([test_executables[0],'--test-threads=2'],candidate)
        require('native_scope_closes_once_and_fresh_eof_is_false ... ok' in native_test
                and 'test result: ok. 1 passed; 0 failed;' in native_test,'chapter native lifecycle test not executed')
        # Import only frozen harness bytes (dataclasses need registered modules).
        sys.path.insert(0,str(frozen/'tests'))
        for name in ['cases','protocol','expectations','xml_protocol','xml_vectors']:
            load(frozen/'tests'/f'{name}.py',name)
        expect=sys.modules['expectations']; protocol=sys.modules['protocol']; xp=sys.modules['xml_protocol']
        cases=dict(expect.DIFFERENTIAL)
        independent={w.name:w for w in expect.WITNESSES}
        cases.update({w.name:w.wire for w in expect.WITNESSES})
        runner.run([sys.executable,'-B',frozen/'tests/rejection.py',candidate/'tests/inflate/run_support.py',binaries['chapter-host'],binaries['chapter-xml-host']],candidate)
        reader=load(candidate/'expected.py','chapter_expected')
        differential=[{'name':name,'wire':wire} for name,wire in cases.items() if name not in independent]
        captured=json.loads((frozen/'expected-provenance.json').read_text())
        require(support.digest(frozen/'expected.json.zlib')==captured['fixture_sha256'],'captured chapter observations changed')
        expected_rows=reader.load(frozen/'expected.json.zlib',differential)
        observations={case['name']:row for case,row in zip(differential,expected_rows)}
        baseline={}; evidence=[]
        def reply(binary,wire,xml=False):
            raw=runner.execute([binary],candidate,wire)
            (xp.parse if xml else protocol.parse_reply)(raw,wire)
            return raw
        for name,wire in cases.items():
            a=independent[name].cpp if name in independent else observations[name]
            protocol.parse_reply(a,wire)
            b=reply(binaries['chapter-host'],wire)
            if name in independent:
                w=independent[name];w.validate()
                require(a==w.cpp and b==w.rust,'independent complete trace: '+name)
            else: require(a==b,'differential mismatch: '+name)
            baseline[('layout',name)]=b
            evidence.append(dict(name=name,independent=name in independent,equal=a==b))
        xml_cases={}
        for name,wire,expected in sys.modules['xml_vectors'].VECTORS:
            a=expected; xp.parse(a,wire); b=reply(binaries['chapter-xml-host'],wire,True)
            require(a==expected and b==expected,'XML independent complete trace: '+name)
            baseline[('xml',name)]=b; xml_cases[name]=wire
        kills=[]
        mappings={"core/xml_session.rs":"core/src/xml_session.rs"}
        for name in ["arabic","engine","images","plain","session","style","text","types"]:
            mappings["core/"+name+".rs"]="core/src/chapter_layout/"+name+".rs"
        for i,control in enumerate([] if positive_only else json.loads((frozen/'tests/mutations.json').read_text())):
            relative=mappings[control['path']]
            original=(candidate/relative).read_text()
            require(original.count(control['old'])==1,'mutation site not unique: '+control['id'])
            changed=original.replace(control['old'],control['new'],1)
            expected=dict(live_inventory);expected[relative]=hashlib.sha256(changed.encode()).hexdigest()
            mutant=output/f'mutant-{i:02d}'
            shutil.copytree(candidate,mutant,symlinks=True)
            support.verify_source_inventory(mutant,live_inventory)
            (mutant/relative).write_text(changed)
            source_seals.append(support.SourceSeal(mutant,expected))
            built=rust(mutant,output/f'mutant-target-{i:02d}')
            xml=control['family']=='xml';name=control['witness']
            raw=reply(built['chapter-xml-host' if xml else 'chapter-host'],xml_cases[name] if xml else cases[name],xml)
            require(raw!=baseline[(control['family'],name)],'semantic mutant survived: '+control['id'])
            kills.append(dict(id=control['id'],source_sha256=expected[relative],
                              response_sha256=hashlib.sha256(raw.encode()).hexdigest(),clean_complete=True))
        report.update(complete_family=not positive_only,mutations_executed=not positive_only,
                      native_lifecycle_tests=1,layout_cases=len(evidence),xml_independent=len(xml_cases),cases=evidence,
                      semantic_controls=kills,source_sha256=live_inventory,patch=patch_receipt,
                      expected_sha256=support.digest(frozen/'expected.json.zlib'),
                      rust_binaries={name:support.digest(binary) for name,binary in binaries.items()})
        report.update(comparisons=len(evidence)+len(xml_cases),independent_checks=len(independent)+len(xml_cases),
                      commands=runner.commands,
                      firmware_or_hardware_acceptance=False,
                      scope='Shared chapter core and host native XML/linebreak adapters; target native seams/resources unaccepted')
        return report
    finally:
        try:
            support.close_sources(source_seals)
            require(support.verify_tools()==tools,'chapter approved tools changed')
        finally:
            sys.path[:]=previous_path
            for name,previous in previous_modules.items():
                if previous is None:sys.modules.pop(name,None)
                else:sys.modules[name]=previous
            (output/'commands.json').write_text(json.dumps(runner.commands,indent=2)+'\n')


def verify_target(gate,artifact):
    lock=json.loads((gate.ROOT/'rust/tests/inflate/toolchains.json').read_text())
    entry=lock['executables']['nm'];nm=gate.ROOT/entry['path']
    if gate.digest(nm)!=entry['sha256']:raise RuntimeError('chapter approved nm changed')
    symbols=gate.run([nm,'-S','--defined-only',artifact]).stdout
    names=['nrust_chapter_'+part+'_borrowed' for part in ('plain','construct','xml','prescan','page_view')]
    for name in names:
        rows=[line.split() for line in symbols.splitlines() if line.split()[-1:]==[name]]
        if not any(len(row)==4 and row[-2]=='T' and int(row[1],16)>0 for row in rows):
            raise RuntimeError('chapter borrowed RV32 symbol absent: '+name)
    return names
