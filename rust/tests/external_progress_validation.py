"""Validate transport grammar, without validating the record policy under test."""
import re
_HEX=re.compile(r'(?:[0-9a-f]{2})*\Z')
_TOKEN=re.compile(r'[A-Za-z0-9_.-]+\Z')
def _decimal(value, maximum):
    if not re.fullmatch(r'0|[1-9][0-9]*',value) or int(value)>maximum:
        raise ValueError('noncanonical or out-of-range number')
def _hex(value):
    if not _HEX.fullmatch(value):raise ValueError('invalid lowercase hex')
def _token(value):
    if not _TOKEN.fullmatch(value):raise ValueError('invalid name token')
def _state(value):
    previous=None
    if not value:return
    for entry in value.split(','):
        identity,data=entry.split('=');name,key=identity.split('/')
        _token(name);_token(key);_hex(data)
        current=(name,key)
        if previous is not None and current<=previous:raise ValueError('noncanonical record map')
        previous=current

def validate(reply, operation):
    if operation not in ('xp.load','xp.checkpoint','xp.peers','xp.save','xp.save-peers'):
        raise ValueError('invalid operation')
    fields=reply.split('|')
    if len(fields)!=9:raise ValueError('wrong field count')
    result,status,book,namespace,output,admission,trace,visible,durable=fields
    _decimal(result,1 if operation in ('xp.save','xp.save-peers') else 3)
    _decimal(status,12);_decimal(book,(1<<64)-1)
    if namespace:_token(namespace)
    _hex(output)
    if len(output)!=146:raise ValueError('wrong output extent')
    if int(output[112:114],16)>3:raise ValueError('invalid peer mask')
    if not re.fullmatch('[01]{2}',admission):raise ValueError('invalid admission flags')
    if trace:
        for entry in trace.split(','):
            fields=entry.split(':')
            if entry=='end':continue
            if len(fields)==3 and fields[0]=='begin' and fields[2] in ('0','1'):_token(fields[1])
            elif len(fields)==2 and fields[0]=='read':_token(fields[1])
            elif len(fields)==3 and fields[0]=='put':_token(fields[1]);_hex(fields[2])
            else:raise ValueError('invalid trace token')
    _state(visible);_state(durable)
