"""Linear interactive parity; adaptive decisions come from frozen native replies."""
import hashlib,json,os,re,select,signal,subprocess,time,zlib
from pathlib import Path
from compare import parse_result


class ParityMismatch(RuntimeError):
    def __init__(self, case, call, cpp, rust):
        if call is None:
            self.witness = {'case': case, 'expected': cpp, 'actual': rust}
        else:
            self.witness = {'case': case, 'call': call,
                            'cpp': [*cpp[:4], cpp[4].hex()],
                            'rust': [*rust[:4], rust[4].hex()]}
        super().__init__(json.dumps(self.witness))


def equal(case, call, cpp, rust):
    if cpp != rust:
        raise ParityMismatch(case, call, cpp, rust)


def require(condition,message):
    if not condition:raise RuntimeError(message)


class Peer:
    def __init__(self,binary,header,deadline,log):
        self.deadline=deadline;self.buffer=b'';self.stderr=log.open('wb');self.process=None
        try:
            self.process=subprocess.Popen([str(Path(binary).resolve())],stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,stderr=self.stderr)
            os.set_blocking(self.process.stdin.fileno(),False)
            os.set_blocking(self.process.stdout.fileno(),False)
            self.write(header)
        except BaseException:
            self.close(failed=True);raise

    def write(self,data):
        view=memoryview(data)
        while view:
            left=self.deadline-time.monotonic();require(left>0,'stream comparison deadline')
            _,ready,_=select.select([],[self.process.stdin],[],left)
            require(ready,'stream write deadline')
            size=os.write(self.process.stdin.fileno(),view);require(size>0,'stream write closed')
            view=view[size:]

    def call(self,request,parameters):
        self.write(request);limit=160+parameters[2]*2
        while b'\n' not in self.buffer:
            left=self.deadline-time.monotonic();require(left>0,'stream response deadline')
            ready,_,_=select.select([self.process.stdout],[],[],left)
            require(ready,'stream response deadline')
            data=os.read(self.process.stdout.fileno(),4096)
            require(data,'transport exited before complete response')
            self.buffer+=data;require(len(self.buffer)<=limit+4096,'unbounded transport response')
        line,_,self.buffer=self.buffer.partition(b'\n');raw=line+b'\n'
        return parse_result(raw,[parameters])[0],raw

    def close(self,failed=False):
        p=self.process
        try:
            if p is not None:
                if failed:
                    try:p.kill()
                    except ProcessLookupError:pass
                else:
                    try:p.stdin.close()
                    except BrokenPipeError:pass
                try:p.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    p.kill();p.wait()
                    if not failed:
                        raise RuntimeError('transport did not exit, never a mutation kill')
                if not failed:
                    require(p.returncode==0,'transport nonzero exit, never a mutation kill')
                    require(not self.buffer and not p.stdout.read(),'extra unsolicited transport rows')
        finally:
            if p is not None:
                if p.poll() is None:p.kill();p.wait()
                for stream in (p.stdin,p.stdout):
                    if stream is not None and not stream.closed:
                        try:stream.close()
                        except BrokenPipeError:pass
            self.stderr.close()

    def __enter__(self):return self
    def __exit__(self,kind,value,trace):self.close(failed=kind is not None)


def compare_stream(oracle,rust,mode,name,packed,expected,wrapped,chunks,ring,deadline,directory):
    size=32768 if ring else max(262144,len(expected or b'')+258)
    header=f'TINFL1 {mode} {size}\n'.encode();ip=op=turn=0;decoded=bytearray();first_mismatch=None
    case_id=f'{mode}-{name}-{int(ring)}-'+','.join(map(str,chunks))
    label=hashlib.sha256(case_id.encode()).hexdigest()[:20]
    if hasattr(oracle, 'stream_digest'):
        digest_check=oracle.stream_digest(header, case_id)
        max_loop_calls=min(200000, digest_check.expected_calls)
        ran_terminal=False;starvation_error=False;terminal=None;actual=None
        with Peer(rust,header,deadline,directory/(label+'-rust.stderr')) as new:
            for call_number in range(max_loop_calls):
                chunk=chunks[turn%len(chunks)];turn+=1;inp=packed[ip:ip+chunk]
                flags=(1 if wrapped else 8)|(0 if ring else 4)|(2 if ip+len(inp)<len(packed) else 0)
                capacity=size-op if ring else min(size-op,(0,1,2,7,258,4096)[turn%6])
                parameters=(flags,op,capacity,inp)
                request=f'CALL {flags} {op} {capacity} {inp.hex() or "-"}\n'.encode()
                actual,raw_new=new.call(request,parameters)
                digest_check.record(request,raw_new)
                status,consumed,produced,_,data=actual
                decoded.extend(data);ip+=consumed;op+=produced
                if ring:op%=size
                if status<=0:
                    ran_terminal=True;terminal=status;break
                if not (consumed or produced or capacity==0 or inp or status!=1):
                    starvation_error=True
            if ran_terminal:
                parameters=(flags&~2,op,size-op,b'');request=f'CALL {flags&~2} {op} {size-op} -\n'.encode()
                actual,raw_new=new.call(request,parameters)
                digest_check.record(request,raw_new)
                decoded.extend(actual[4])
        if not digest_check.matches():
            summary=digest_check.summary()
            raise ParityMismatch(case_id, None, summary['expected'], summary['actual'])
        require(ran_terminal, 'fixture watchdog, not a decoder domain limit')
        require(not starvation_error, 'impossible input-starvation progress')
        if expected is not None:
            require(terminal==0 and bytes(decoded)==expected,f'independent decoded expectation {case_id}')
            require(actual[3] == zlib.adler32(expected), f'independent Adler expectation {case_id}')
        known={'hand-empty-zlib-bad-checksum':-2,'reserved-block':-1,'stored-complement':-1,'header-method':-1,'header-dictionary':-1}
        if name in known:require(terminal==known[name],f'independent error status {case_id}')
        return {'id':case_id,'calls':digest_check.calls,'packed_sha256':hashlib.sha256(packed).hexdigest(),
                'decoded_sha256':hashlib.sha256(decoded).hexdigest(),'decoded_bytes':len(decoded),
                'independent_bytes_expected':expected is not None,'terminal_status':terminal,
                'wire_sha256':{'request':digest_check.req_hash.hexdigest(),
                               'cpp':digest_check.expected_response_sha256,
                               'rust':digest_check.resp_hash.hexdigest()}}
    hashes={key:hashlib.sha256() for key in ('request','cpp','rust')};hashes['request'].update(header)
    with Peer(oracle,header,deadline,directory/(label+'-cpp.stderr')) as old,Peer(rust,header,deadline,directory/(label+'-rust.stderr')) as new:
        for call_number in range(200000):
            chunk=chunks[turn%len(chunks)];turn+=1;inp=packed[ip:ip+chunk]
            flags=(1 if wrapped else 8)|(0 if ring else 4)|(2 if ip+len(inp)<len(packed) else 0)
            capacity=size-op if ring else min(size-op,(0,1,2,7,258,4096)[turn%6])
            parameters=(flags,op,capacity,inp)
            request=f'CALL {flags} {op} {capacity} {inp.hex() or "-"}\n'.encode()
            previous,raw_old=old.call(request,parameters);actual,raw_new=new.call(request,parameters)
            hashes['request'].update(request);hashes['cpp'].update(raw_old);hashes['rust'].update(raw_new)
            if previous != actual and first_mismatch is None:
                first_mismatch = ParityMismatch(case_id, call_number, previous, actual)
            status,consumed,produced,_,data=previous
            decoded.extend(data);ip+=consumed;op+=produced
            if ring:op%=size
            if status<=0:break
            require(consumed or produced or capacity==0 or inp or status!=1,'impossible input-starvation progress')
        else:raise RuntimeError('fixture watchdog, not a decoder domain limit')
        terminal=status
        parameters=(flags&~2,op,size-op,b'');request=f'CALL {flags&~2} {op} {size-op} -\n'.encode()
        previous,raw_old=old.call(request,parameters);actual,raw_new=new.call(request,parameters)
        hashes['request'].update(request);hashes['cpp'].update(raw_old);hashes['rust'].update(raw_new)
        if previous != actual and first_mismatch is None:
            first_mismatch = ParityMismatch(case_id, call_number + 1, previous, actual)
        decoded.extend(previous[4])
    if expected is not None:
        require(terminal==0 and bytes(decoded)==expected,f'independent decoded expectation {case_id}')
        require(previous[3] == zlib.adler32(expected), f'independent Adler expectation {case_id}')
    known={'hand-empty-zlib-bad-checksum':-2,'reserved-block':-1,'stored-complement':-1,'header-method':-1,'header-dictionary':-1}
    if name in known:require(terminal==known[name],f'independent error status {case_id}')
    if first_mismatch is not None:
        raise first_mismatch
    return {'id':case_id,'calls':call_number+2,'packed_sha256':hashlib.sha256(packed).hexdigest(),
            'decoded_sha256':hashlib.sha256(decoded).hexdigest(),'decoded_bytes':len(decoded),
            'independent_bytes_expected':expected is not None,'terminal_status':terminal,
            'wire_sha256':{key:h.hexdigest() for key,h in hashes.items()}}


def stop_for_signal(signum,_):raise KeyboardInterrupt(f'received signal{signum}')


def script_cases():
    return [
        ('suspended-huffman', 16, [
            ((6, 0, 16, bytes([5, 0, 0, 4]) + bytes(32)), (1, 36, 0, 1, b'')),
            ((6, 0, 16, bytes(4)), (2, 0, 16, 1, bytes(16)))]),
        ('saved-copy-cursor', 33026, [
            ((0, 32767, 1, bytes([3, 2, 0])), (2, 2, 1, 1, bytes(1))),
            ((4, 0, 2, bytes([0])), (0, 1, 2, 1, bytes(2)))]),
        ('fresh-bad-param', 3, [((0, 0, 3, b''), (-3, 0, 0, 0, b''))]),
    ]


def compare_script(oracle, rust, mode, name, size, calls, deadline, directory):
    header = f'TINFL1 {mode} {size}\n'.encode()
    label = hashlib.sha256(f'script-{mode}-{name}'.encode()).hexdigest()[:20]
    first_mismatch = None
    if hasattr(oracle, 'stream_digest'):
        digest_check = oracle.stream_digest(header, f'script-{mode}-{name}')
        with Peer(rust, header, deadline, directory / (label + '-rust.stderr')) as new:
            for index, (parameters, expected) in enumerate(calls):
                flags, offset, capacity, data = parameters
                request = f'CALL {flags} {offset} {capacity} {data.hex() or "-"}\n'.encode()
                actual, raw_new = new.call(request, parameters)
                digest_check.record(request, raw_new)
                if actual != expected and first_mismatch is None:
                    first_mismatch = ParityMismatch(f'{mode}-{name}', index, expected, actual)
        if first_mismatch is not None:
            raise first_mismatch
        if not digest_check.matches():
            summary = digest_check.summary()
            raise ParityMismatch(f'{mode}-{name}', None, summary['expected'], summary['actual'])
        return {'id': f'{mode}-{name}', 'calls': len(calls), 'independent_exact_rows': True}
    with Peer(oracle,header,deadline,directory/(label+'-cpp.stderr')) as old,Peer(rust,header,deadline,directory/(label+'-rust.stderr')) as new:
        for index, (parameters, expected) in enumerate(calls):
            flags, offset, capacity, data = parameters
            request = f'CALL {flags} {offset} {capacity} {data.hex() or "-"}\n'.encode()
            cpp, _ = old.call(request, parameters)
            actual, _ = new.call(request, parameters)
            if cpp != actual and first_mismatch is None:
                first_mismatch = ParityMismatch(f'{mode}-{name}', index, cpp, actual)
            require(cpp == expected, f'independent script expectation {name} call{index}: {cpp}')
    if first_mismatch is not None:
        raise first_mismatch
    return {'id': f'{mode}-{name}', 'calls': len(calls), 'independent_exact_rows': True}
