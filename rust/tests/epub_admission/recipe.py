#!/usr/bin/env python3
"""Offline canonical EPUB host parity, semantic controls and RV32 compilation."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shlex
import shutil
import sys
import time
sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def require(ok, why):
    if not ok: raise RuntimeError(why)


def dump(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True)+'\n')


def main(argv=None, tool_root=None):
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--positive-only', action='store_true', help='partial proof; no full acceptance')
    parser.add_argument('--allowance', type=int, default=540)
    args = parser.parse_args(argv)
    tool_root = ROOT if tool_root is None else tool_root
    require(1 <= args.allowance <= 540, 'deadline must be <=540 seconds')
    deadline=time.monotonic()+args.allowance
    output = args.output.resolve()
    require(output.is_relative_to(tool_root / '.build/rust') and not output.exists(), 'fresh .build/rust output required')
    require(not output.is_relative_to(HERE), 'output outside sources')
    # Frozen inputs retain pinned hashes; live source identities are recorded below.
    expected = json.loads((HERE/'INPUTS.json').read_text())
    def sha(p):
        with p.open('rb') as f: return hashlib.file_digest(f,'sha256').hexdigest()
    def verify_paths():
        # Original manifests authenticate the capture; runtime products remain sealed below.
        data = json.loads(__import__('zlib').decompress((HERE/'expected-v2.json.zlib').read_bytes()))
        require(data['source_provenance'] == expected, 'EPUB frozen provenance changed')
        load(ROOT/'rust/tests/xml_session/native_inputs.py', 'epub_expat_inputs').verify(
            ROOT/'rust/tests/package_parsers', tool_root/'vendor/freeink-sdk')
    support=load(ROOT/'rust/tests/inflate/run_support.py','epub_canonical_run')
    root_seal=support.SourceSeal(ROOT/'rust')
    verify_paths()
    root_seal.check()
    support.ROOT = tool_root
    tools=support.verify_tools()
    support.install_signal_handlers()
    inventory_path=tool_root/'.toolchain/rust/installed.json'
    def component_inventory():
        prefix=tool_root/'.toolchain/rust/toolchain'
        result={}
        for p in sorted(prefix.rglob('*')):
            key=str(p.relative_to(prefix))
            if p.is_symlink(): result[key]={'link':os.readlink(p)}
            elif p.is_file(): result[key]={'sha256':sha(p),'bytes':p.stat().st_size}
        return result
    components=component_inventory()
    output.mkdir(parents=True)
    dump(output/'tools-before.json',dict(tools={k:dict(path=str(v),sha256=sha(v)) for k,v in tools.items()},receipt_sha256=sha(inventory_path),components=components))
    runner=support.Run(output,tools,deadline)
    sources=output/'sources'; sources.mkdir()
    copied=sources/'rust'
    support.copy_workspace(ROOT/'rust',copied)
    # Existing unrelated Core integration tests need their canonical PNG fixtures.
    shutil.copytree(ROOT/'rust/tests/png_decoder/tests/fixtures',copied/'tests/png_decoder/tests/fixtures')
    bridge=sources/'expat_bridge.c'
    shutil.copyfile(ROOT/'rust/target-expat/native/expat_bridge.c',bridge)
    closures={sources:support.SourceSeal(sources), ROOT/'rust':root_seal}
    dump(output/'copied-inputs.json',{**closures[sources].hashes, **{'live/rust/'+k:v for k,v in root_seal.hashes.items()}})
    protocol=load(HERE/'protocol.py','epub_protocol')
    corpus=load(HERE/'corpus.py','epub_corpus')
    mutations=load(HERE/'mutations.py','epub_mutations')
    controls=load(HERE/'controls.py','epub_controls')
    vectors=load(HERE/'vectors.py','epub_vectors')
    cases=corpus.cases(vectors)
    require(len({c['id'] for c in cases})==len(cases),'duplicate case id')
    dump(output/'case-manifest.json',[{**c,'data':c['data'].hex(),**({'payload':c['payload'].hex()} if 'payload' in c else {}),'wire':corpus.wire(c)} for c in cases])
    def verify(full=False):
        if full:
            try: verify_paths()
            finally: support.close_sources(closures.values())
        else:
            for seal in closures.values(): seal.check()
    def command(argv,cwd=output,stdin=None,runtime=False):
        verify()
        try: return (runner.execute if runtime else runner.run)(argv,cwd,stdin)
        finally: verify()
    def cargo(workspace,target,*argv):
        verify()
        try: return runner.cargo(workspace,target,*argv)
        finally: verify()
    records=[]
    try:
        # Genuine Expat/bridge product objects remain; no C++ reference enters this test.
        book=tool_root/'vendor/freeink-sdk/libs/book/FreeInkBook'
        include=book/'include'; expat=book/'third_party/expat'
        flags=['-O1','-g','-fsanitize=address,undefined','-fno-sanitize-recover=all','-fno-omit-frame-pointer']
        native=['--no-default-config','-fuse-ld='+str(tools['native-ld']),*flags]
        includes=['-I',include,'-I',expat,'-I',tool_root/'vendor/freeink-sdk/libs/font/FreeInkFont/include']
        objects={}
        for name,source in [('parse',book/'src/vendor/expat_xmlparse.c'),('tok',book/'src/vendor/expat_xmltok.c'),('role',book/'src/vendor/expat_xmlrole.c'),('bridge',bridge)]:
            obj=output/(name+'.o')
            command([tools['clang++'],'--no-default-config','-x','c','-std=c99',*flags,*includes,'-c',source,'-o',obj])
            objects[name]=obj
        frozen_transport=load(ROOT/'rust/tests/inflate/expected_transport.py','epub_expected')
        fail_calls=json.loads((HERE/'fail-call-count.json').read_text())
        require(type(fail_calls) is int and fail_calls >= 0,'invalid frozen fail-call count')
        valid=next(c for c in cases if c['id']=='valid-nav-not-parsed')
        native_cases=[c for c in cases if 'typed' not in c]
        native_cases += [{**valid,'id':f'fail-call-{call}','fail_call':call} for call in range(fail_calls)]
        reference=frozen_transport.Frozen(HERE/'expected-v2.json.zlib',
            [dict(mode=0,input=corpus.wire(c)) for c in native_cases],expected)
        cpp=HERE/'expected-v2.json.zlib'
        host_flags=['-Dwarnings','-Ccodegen-units=1','-C','link-arg=-fsanitize=address,undefined','-C','link-arg=-fno-sanitize-recover=all']
        for name in ('parse','tok','role','bridge'): host_flags+=['-C','link-arg='+str(objects[name])]
        runner.env['RUSTFLAGS']=shlex.join(host_flags)
        def build(workspace,target,tests=False):
            messages=cargo(workspace,target,'test' if tests else 'build',*(['--no-run'] if tests else []),'-p','newt-parity','--bin','epub-admission-host','--features','xml-native','--message-format=json')
            artifacts=[]
            for line in messages.splitlines():
                message=json.loads(line)
                if message.get('reason')=='compiler-artifact' and message.get('executable'):
                    require(message.get('fresh') is False,'stale executable')
                    path=Path(message['executable'])
                    require(path.resolve().is_relative_to(target.resolve()) and path.is_file(),'artifact path')
                    artifacts.append(path)
            require(bool(artifacts),'no fresh executable')
            dump(output/(target.name+'-artifacts.json'),[dict(path=str(p),sha256=sha(p)) for p in artifacts])
            return artifacts
        # Candidate typed layer tests and native lifecycle are mandatory even partial.
        cargo(copied,output/'core-unit-tests','test','-p','newt-core','--lib','--message-format=json')
        cargo(copied,output/'core-layer-tests','test','-p','newt-core','--test','epub_admission','--message-format=json')
        for executable in build(copied,output/'host-tests',True): command([executable],runtime=True)
        rust,=build(copied,output/'host-target')
        def reply(binary,case):
            runner.env['NEWT_TYPED_CASE']=case.get('typed','')
            wire=corpus.wire(case)
            raw=reference.text(wire) if binary==cpp else command([binary],stdin=wire,runtime=True)
            rows=protocol.response(raw,wire)
            return rows
        baseline={}
        for case in cases:
            r=reply(rust,case);corpus.check(case,r)
            record=dict(id=case['id'],rust=r,domain=case.get('domain','shared semantic domain'))
            if 'typed' not in case:
                c=reply(cpp,case);corpus.check(case,c,cpp=True)
                record['cpp']=c
                record['arena_diagnostics']=dict(rust=r[-2][1:],cpp=c[-2][1:],matched_claim=False)
                if 'domain' not in case: require(protocol.semantic(r)==protocol.semantic(c),'semantic/read/event parity: '+case['id'])
            baseline[case['id']]=r;records.append(record)
            dump(output/'positive-receipts.json',records)
        controls.run(protocol,corpus,next(c for c in cases if c['id']=='valid-nav-not-parsed'),baseline['valid-nav-not-parsed'])
        chapter=next(c for c in cases if c['id']=='chapter-stored')
        controls.run(protocol,corpus,chapter,baseline[chapter['id']])
        dump(output/'negative-controls.json',dict(protocol_field_deletion=True,field_bounds=True,expectation_tampering=True))
        # Dynamic fail-call coverage spans each observed baseline source read.
        valid=next(c for c in cases if c['id']=='valid-nav-not-parsed')
        require(fail_calls == sum(r[0]=='R' for r in baseline[valid['id']]), 'frozen dynamic read corpus changed')
        for call in range(fail_calls):
            case={**valid,'id':f'fail-call-{call}','fail_call':call}
            r=reply(rust,case);c=reply(cpp,case)
            require(protocol.semantic(r)==protocol.semantic(c),'fail-call parity '+str(call))
            # Some failed optional lookups may be recoverable; equality alone
            # does not establish fault propagation. Record every observed status.
            records.append(dict(id=case['id'],rust=r,cpp=c))
            dump(output/'positive-receipts.json',records)
        dump(output/'positive-receipts.json',records)
        # Borrowed actual core and nonconstant runtime-input functions compile for RV32.
        runner.env['RUSTFLAGS']='-Dwarnings -Ccodegen-units=1'
        cargo(copied,output/'rv32-core','build','-p','newt-core','--target',support.TARGET,'--message-format=json')
        cargo(copied,output/'rv32-borrowed','build','-p','newt-target-probe','--target',support.TARGET,'--message-format=json')
        archives=list((output/'rv32-borrowed'/support.TARGET/'debug').glob('libnewt_target_probe.a'))
        require(len(archives)==1,'missing RV32 borrowed archive')
        symbols=command([tools['nm'],'-S','--defined-only',archives[0]])
        for symbol in ('epub_probe_inspect','epub_probe_chapter'):
            require(any(symbol in line and len(line.split())>=4 and int(line.split()[1],16)>0 for line in symbols.splitlines()),'missing/nonempty target body: '+symbol)
        dump(output/'rv32-receipt.json',dict(archive=str(archives[0]),sha256=sha(archives[0]),symbols=symbols,evidence='RV32 library and borrowed inspect/chapter nonempty bodies only; no generic XML backend claim'))
        runner.env['RUSTFLAGS']=shlex.join(host_flags)
        kills=[]
        if not args.positive_only:
            for index,proposal in enumerate(mutations.proposals()):
                stage=output/f'mutant-{index:02d}';stage.mkdir()
                mc=stage/'rust';shutil.copytree(copied,mc)
                mutations.apply(mc,proposal)
                closures[stage]=support.SourceSeal(stage)
                dump(output/(stage.name+'-inputs.json'),closures[stage].hashes)
                binary,=build(mc,output/(stage.name+'-target'))
                witnesses=[]
                for case_id in proposal[4]:
                    case=next(c for c in cases if c['id']==case_id)
                    rows=reply(binary,case) # clean runtime + valid state grammar FIRST
                    if rows!=baseline[case_id]: witnesses.append(dict(id=case_id,baseline=baseline[case_id],mutant=rows))
                require(bool(witnesses),'survived mutation: '+proposal[0])
                kills.append(dict(name=proposal[0],file=proposal[1],old=proposal[2],new=proposal[3],witnesses=witnesses))
                dump(output/'mutation-receipts.json',kills)
        dump(output/'result.json',dict(complete=False,host_comparison_complete=not args.positive_only,partial_positive_proof=args.positive_only,mutations=len(kills),pending=['full XML RV32 real backend/retained exports/ELF stack and storage measurement','full processing-policy cancellation (NoPolicy oracle domain only)','hardware evidence'],arena_totals='separately validated; never claimed matched'))
    finally:
        verify(full=True)
        require(component_inventory()==components,'Rust component file set changed')
        for receipt in runner.commands:
            for suffix in ('stdout','stderr'):
                p=output/(receipt['log']+'.'+suffix)
                if p.is_file(): receipt[suffix+'_sha256']=sha(p)
        dump(output/'commands.json',runner.commands)
        dump(output/'tools-after.json',dict(tools={k:dict(path=str(v),sha256=sha(v)) for k,v in support.verify_tools().items()},components=component_inventory()))

    for path in output.iterdir():
        if path.is_dir():
            shutil.rmtree(path)


if __name__=='__main__': main()
