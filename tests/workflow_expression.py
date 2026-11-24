"""GitHub Actions `if` expressions evaluated as GitHub does, for the workflow
tests (tests/ci_workflow_test.py, tests/public_ci_test.py)."""
import ast
import json
import math
import re


LITERALS = {'true': True, 'false': False, 'null': None}
STATUS_FUNCTIONS = frozenset(('success', 'failure', 'cancelled', 'always'))


def loose_equal(left, right):
    """GitHub's ==: strings ignore case, and values of different types compare as
    numbers, so null == false holds; a guard must compare toJSON text instead."""
    def number(value):
        if value is None or isinstance(value, bool):
            return int(bool(value))
        try:
            return float(value) if value != '' else 0
        except ValueError:
            return math.nan
    if type(left) is type(right):
        return left.lower() == right.lower() if isinstance(left, str) else left == right
    return number(left) == number(right)


def trust_expression(expression):
    """Evaluate a job's actual small workflow `if` expression. Unknown syntax fails
    closed, rather than testing a separate policy implementation. A context path
    such as github.event.repository.private reads values['event.repository.private'],
    and needs.classify.outputs.lane reads values['needs.classify.outputs.lane'];
    None stands for a value the event does not carry. As in GitHub Actions, an
    expression that calls no status check function implicitly requires success()."""
    text = re.sub(r'!(?!=)', ' not ', expression.replace('&&', ' and ').replace('||', ' or '))
    syntax = ast.parse(text.strip(), mode='eval')
    has_status_check = any(isinstance(node, ast.Call) and isinstance(node.func, ast.Name)
                           and not node.keywords and not node.args and node.func.id in STATUS_FUNCTIONS
                           for node in ast.walk(syntax))
    def path(node):
        if isinstance(node, ast.Name) and node.id == 'github':
            return []
        if isinstance(node, ast.Name) and node.id == 'needs':
            return ['needs']
        if isinstance(node, ast.Attribute):
            return [*path(node.value), node.attr]
        raise AssertionError('Unreviewed workflow trust expression')
    def effective(node, values):
        if isinstance(node, ast.Expression):
            return effective(node.body, values)
        if isinstance(node, ast.Constant):
            return node.value
        if isinstance(node, ast.Name) and node.id in LITERALS:
            return LITERALS[node.id]
        if isinstance(node, ast.Attribute):
            return values['.'.join(path(node))]
        if isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and not node.keywords:
            if node.func.id in ('success', 'failure', 'cancelled') and not node.args:
                return values[node.func.id]
            if node.func.id == 'always' and not node.args:
                return True
            if node.func.id == 'toJSON' and len(node.args) == 1:  # GitHub's JSON of a missing value is null
                return json.dumps(effective(node.args[0], values))
        if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.Not):
            return not effective(node.operand, values)
        if isinstance(node, ast.BoolOp):
            items = [effective(value, values) for value in node.values]
            if isinstance(node.op, ast.And):
                return all(items)
            if isinstance(node.op, ast.Or):
                return any(items)
        if isinstance(node, ast.Compare) and len(node.ops) == 1 and isinstance(node.ops[0], (ast.Eq, ast.NotEq)):
            equal = loose_equal(effective(node.left, values), effective(node.comparators[0], values))
            return equal if isinstance(node.ops[0], ast.Eq) else not equal
        raise AssertionError('Unreviewed workflow trust expression')
    def evaluate(values):
        matched = effective(syntax, values)
        return (has_status_check or values.get('success', True)) and matched
    return evaluate


PRIVATE_ID = '1400798123'  # The private repository's immutable ID (fresh-gates.yml).
# What each event shows of where it runs: (event.repository.private, repository_id).
# A scheduled run's event carries no repository.
PRIVATE, PUBLIC = {'event.repository.private': True, 'repository_id': PRIVATE_ID}, \
    {'event.repository.private': False, 'repository_id': '987654321'}


def where(values, event_name):
    """values as event_name shows them: a scheduled run's event has no repository."""
    return {**values, 'event_name': event_name,
            **({'event.repository.private': None} if event_name == 'schedule' else {})}
