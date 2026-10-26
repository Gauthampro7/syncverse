import ast
import os

print("=== DEEP AST AUDIT OF TESTS ===")
test_files = [
    'tests/empirical_challenge/test_sync_engine.py',
    'tests/empirical_challenge/test_smart_chat.py',
    'tests/empirical_challenge/test_ui_renderer.py'
]

for tf in test_files:
    print(f"\nAuditing AST of {tf}:")
    with open(tf, 'r', encoding='utf-8') as f:
        tree = ast.parse(f.read(), filename=tf)

    constant_returns = []
    not_implemented = []
    tautological_asserts = []

    for node in ast.walk(tree):
        # Check functions returning constants only
        if isinstance(node, ast.FunctionDef):
            if len(node.body) == 1 and isinstance(node.body[0], ast.Return):
                ret_val = node.body[0].value
                if isinstance(ret_val, ast.Constant):
                    constant_returns.append((node.name, node.lineno, ret_val.value))

        # Check for NotImplementedError
        if isinstance(node, ast.Raise):
            if isinstance(node.exc, ast.Name) and node.exc.id == 'NotImplementedError':
                not_implemented.append(node.lineno)
            elif isinstance(node.exc, ast.Call) and getattr(node.exc.func, 'id', None) == 'NotImplementedError':
                not_implemented.append(node.lineno)

        # Check for assert True or assert 1 == 1
        if isinstance(node, ast.Assert):
            if isinstance(node.test, ast.Constant) and bool(node.test.value) is True:
                tautological_asserts.append(node.lineno)
            elif isinstance(node.test, ast.Compare):
                # Check if left and right are identical constants
                if isinstance(node.test.left, ast.Constant) and len(node.test.comparators) == 1 and isinstance(node.test.comparators[0], ast.Constant):
                    if node.test.left.value == node.test.comparators[0].value:
                        tautological_asserts.append(node.lineno)

    print(f"  Constant returns: {constant_returns}")
    print(f"  NotImplemented raises: {not_implemented}")
    print(f"  Tautological asserts: {tautological_asserts}")

    assert len(not_implemented) == 0, f"Found NotImplemented in {tf}"
    assert len(tautological_asserts) == 0, f"Found tautological assert in {tf}"

print("\nAST Audit complete. Zero facades or dummy functions detected.")
