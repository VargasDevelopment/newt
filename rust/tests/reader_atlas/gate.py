#!/usr/bin/env python3
"""Check frozen atlas output fixtures against Rust and retain RV32 code."""
import argparse
import importlib.util
import json
from pathlib import Path
import shutil
import sys
import time
import zlib
sys.dont_write_bytecode = True
HOST = 'x86_64-unknown-linux-gnu'
RV32 = 'riscv32imc-unknown-none-elf'
FIELDS = ('metrics','status','frame_sha256','ink_bits')

def load(name, path):
    spec = importlib.util.spec_from_file_location(name,path)
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module

def require(ok, message):
    if not ok: raise RuntimeError(message)

def expected(c): return {k:c[k] for k in FIELDS}

def semantic_kill(actual, c, control):
    # Must only be called AFTER Run.execute AND strict decode of the entire batch.
    require(actual['status']==c['status'], 'unexpected draw status is not the specified control kill')
    component = control['component']
    if component == 'bitmap-only':
        require(actual['metrics']==c['metrics'], 'width-only mutant changed metrics')
        return actual['frame_sha256']!=c['frame_sha256'] and actual['ink_bits']==0
    if isinstance(component,int): return actual['metrics'][component]!=c['metrics'][component]
    return actual['frame_sha256']!=c['frame_sha256']

def admit(run, transport, binary, cwd, cases):
    raw = run.execute([binary],cwd,''.join(transport.encode(c['request']) for c in cases))
    return transport.decode(raw,cases)

def protocol_controls(transport, c):
    # Independent syntactic fixtures: no expectation regeneration.
    valid = 'A1 '+' '.join(map(str,c['metrics']+[c['status']]))+' '+'00'*c['request'][10]+'\n'
    transport.decode(valid,[c])
    malformed = [valid[:-1],valid.replace('\n','\r\n'),valid+valid,
                 valid.replace('A1 ','A2 ',1),valid.replace('A1 ','A1  ',1),
                 valid.replace('A1 ','A1 +',1),valid[:-3]+'\n',valid.replace('00','GG',1)]
    for raw in malformed:
        try: transport.decode(raw,[c])
        except ValueError: pass
        else: raise RuntimeError('malformed response admitted')
    # A clean unchanged value cannot count as a kill, including bitmap-only.
    for component in (None,0,1,'bitmap-only'):
        require(not semantic_kill(expected(c),c,{'component':component}), 'false kill admitted')

def run_gate(root, output, seconds=540, only=None, source_root=None, positive_only=False):
    root=Path(root).resolve(); output=Path(output).resolve()
    require(0 < seconds <= 540, 'internal budget must be <=540s')
    deadline=time.monotonic()+seconds
    # Reuse canonical source-copy, process ownership and Cargo context helpers.
    rust=Path(source_root).resolve() if source_root else root/'rust'
    boot=load('atlas_boot',rust/'tests/inflate/run_support.py')
    boot.install_signal_handlers()
    require(output.is_relative_to(root/'.build/rust') and not output.is_relative_to(rust),
            'output must be a fresh directory under .build/rust outside sources')
    output.mkdir()
    sources=output/'sealed'; sources.mkdir()
    snapshots=[]
    products=[sources]
    def copy_tree(src,dst):
        before=boot.source_inventory(src); shutil.copytree(src,dst)
        boot.verify_source_inventory(dst,before); snapshots.extend([(src,before),(dst,before)])
    copy_tree(rust,sources/'rust')
    support=load('atlas_run',sources/'rust/tests/inflate/run_support.py')
    support.ROOT=root
    tools=support.verify_tools()
    extras=json.loads((sources/'rust/tests/jpeg_decoder/tool_bindings.json').read_text())
    canonical_tools=dict(tools)
    def verify_tools():
        require(support.verify_tools()==canonical_tools,'canonical tool map changed')
        for name,e in extras.items():
            require(support.digest(root / e['path'])==e['sha256'],'tool changed: '+name)
    verify_tools()
    tools.update({k:root / v['path'] for k,v in extras.items()})
    run=support.Run(output,tools,deadline)
    run.env['RUSTFLAGS']='-Dwarnings -Ccodegen-units=1'
    recipe=sources/'rust/tests/reader_atlas'
    transport=load('atlas_transport',recipe/'transport.py')
    cases=json.loads(zlib.decompress((recipe/'fixtures.json.zlib').read_bytes()))['cases']
    controls=json.loads((recipe/'controls.json').read_text())
    byid={c['id']:c for c in cases}
    for control in controls:
        require(control['frozen_witnesses']==[byid[w] for w in control['witnesses']], 'witness differs from frozen corpus')
    if only:
        selected=[c for c in controls if c['name'] in only]
        require(len(selected)==len(set(only)),'unknown control')
    else: selected=controls
    if positive_only: selected=[]
    report={'complete':False,'scope':'all 8471 frozen-output/Rust positives fresh; selected control witnesses fresh; link-only RV32 borrowed retention',
            'full_controls':not bool(only) and not positive_only,'controls':[],'firmware':False,'hardware':False}
    def seal():
        for path,binding in snapshots: support.verify_source_inventory(path,binding)
        verify_tools()
    def workspace(name, control=None):
        dst=sources/name
        shutil.copytree(sources/'rust',dst)
        if control:
            for edit in control['edits']:
                path=dst/'reader-atlas'/edit['file']; text=path.read_text()
                require(text.count(edit['anchor'])==1,'mutation anchor not unique: '+control['name'])
                path.write_text(text.replace(edit['anchor'],edit['replacement'],1))
        snapshots.append((dst,support.source_inventory(dst)))
        target=output/(name+'-target'); products.append(target)
        run.cargo(dst,target,'build','-p','newt-reader-atlas','--target',HOST,'--bin','reader-atlas-host')
        return dst,target,target/HOST/'debug/reader-atlas-host'
    try:
        baseline,target,binary=workspace('baseline')
        run.cargo(baseline,target,'test','-p','newt-reader-atlas','--target',HOST,'--lib')
        run.cargo(baseline,target,'build','-p','newt-reader-atlas','--target',RV32,'--release','--lib')
        for start in range(0,len(cases),32):
            batch=cases[start:start+32]
            require(admit(run,transport,binary,baseline,batch)==[expected(c) for c in batch],'positive mismatch')
        witnesses=list({w:byid[w] for c in selected for w in c['witnesses']}.values())
        if witnesses:
            require(admit(run,transport,binary,baseline,witnesses)==[expected(c) for c in witnesses],'fresh witness positive failed')
        run.execute([tools['python'],'-B',recipe/'reject.py',sources/'rust/tests/inflate/run_support.py',binary],baseline)
        protocol_controls(transport,byid['face-0-cp-65'])
        for index,control in enumerate(selected):
            dst,_,mutant=workspace(f'mutant-{index:02d}',control)
            selected_cases=[byid[w] for w in control['witnesses']]
            rows=admit(run,transport,mutant,dst,selected_cases)
            require(any(semantic_kill(row,c,control) for row,c in zip(rows,selected_cases)), 'mutant survived: '+control['name'])
            report['controls'].append({'name':control['name'],'witnesses':control['witnesses'],'clean_compile_runtime_transport':True,'semantic_kill':True})
            seal()
        report['retention']=retain(run,support,recipe,baseline,target,products)
        seal()
        report.update(complete=True,baseline_cases=len(cases),comparisons=len(cases),
                      baseline_binary_sha256=support.digest(binary),expected_sha256=support.digest(recipe/'fixtures.json.zlib'))
    finally:
        # Any failing closure/tool seal overrides success. Partial receipts remain explicit.
        report['commands']=run.commands
        try:
            seal()
            require(time.monotonic() <= deadline,'internal 540s allowance exceeded')
        except BaseException:
            report['complete']=False
            report['closing_seal_failed']=True
            raise
        finally:
            (output/'evidence.json').write_text(json.dumps(report,indent=2)+'\n')
            (output/'bindings.json').write_text(json.dumps({'sources':{str(p):v for p,v in snapshots}},indent=2)+'\n')
    # Only a complete run with both receipts written can remove generated products.
    # Sources rebuild from this commit; retain receipts/logs, not passing scratch.
    if report['complete']:
        for path in products:
            if path.is_dir(): shutil.rmtree(path)
            else: path.unlink()
    return report

def retain(run,support,recipe,baseline,target,products):
    # Actual canonical target-probe module and canonical workspace profile/lock.
    run.cargo(baseline,target,'build','-p','newt-target-probe','--release','--target',RV32,'--lib')
    archive=target/RV32/'release/libnewt_target_probe.a'
    obj=run.output/'retain.o'; elf=run.output/'atlas-retained.elf'
    products.extend([obj,elf])
    run.run([run.tools['clang++'],'--no-default-config','--target=riscv32','-march=rv32imc','-mabi=ilp32',
             '-x','c','-std=c99','-Os','-ffreestanding','-fno-builtin','-c',recipe/'retain.c','-o',obj],baseline)
    run.run([run.tools['rust-lld'],'-flavor','gnu','-m','elf32lriscv','--gc-sections','-e','atlas_retained_probe',obj,archive,'-o',elf],baseline)
    symbols=run.execute([run.tools['nm'],'-S','--defined-only',elf],baseline)
    for name in ('atlas_retained_probe','atlas_borrowed_runtime','reader_atlas_metrics_runtime','reader_atlas_draw_runtime'):
        require(any(len(w)==4 and name in w[-1] and w[2] in ('t','T') and int(w[1],16)>0
                    for w in map(str.split,symbols.splitlines())), 'nonempty retained body missing: '+name)
    require(not run.execute([run.tools['nm'],'--undefined-only',elf],baseline).strip(),'unresolved ELF imports')
    forbidden=('__rust_alloc','__rust_realloc','__rust_dealloc','__rdl_alloc','__rdl_dealloc','malloc','calloc','realloc','_ZN5alloc')
    require(not any(n in symbols for n in forbidden),'allocator retained')
    return {'scope':'RV32 staticlib + freestanding C retained ELF; runtime-borrowed inputs/output; not RV32 execution or firmware',
            'elf_sha256':support.digest(elf),'staticlib_sha256':support.digest(archive)}

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--seconds',type=int,default=540)
    parser.add_argument('--only',action='append',help='focused control; never full acceptance')
    parser.add_argument('--positive-only',action='store_true',help='diagnostic; never full acceptance')
    args=parser.parse_args()
    run_gate(args.root,args.output,args.seconds,args.only,positive_only=args.positive_only)
