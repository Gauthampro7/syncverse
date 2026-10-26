import os
import re
import sys

print("=== CHECK 1: PLACEHOLDERS, STUBS & STRUCTURAL EMPTINESS ===")
suspicious_pattern = re.compile(r'\b(TODO|FIXME|TBD|XXX|WIP|PLACEHOLDER|STUB|MOCK|DUMMY|LOREM)\b', re.IGNORECASE)

openspec_files = []
for root, dirs, files in os.walk('openspec'):
    for f in files:
        if f.endswith('.md'):
            openspec_files.append(os.path.join(root, f))
openspec_files.sort()

challenge_files = []
for root, dirs, files in os.walk('tests/empirical_challenge'):
    for f in files:
        if f.endswith('.py'):
            challenge_files.append(os.path.join(root, f))
challenge_files.sort()

all_files = openspec_files + challenge_files

found_suspicious = False
for path in all_files:
    with open(path, 'r', encoding='utf-8') as f:
        lines = f.readlines()
    for idx, line in enumerate(lines, 1):
        m = suspicious_pattern.search(line)
        if m:
            print(f"SUSPICIOUS TOKEN in {path}:{idx}: {m.group(0)} in '{line.strip()}'")
            found_suspicious = True

if not found_suspicious:
    print("PASS: Zero suspicious placeholder tokens found across all files.")

# Check for actual empty code blocks: ```<lang>\n```
empty_cb_pattern = re.compile(r'```[a-zA-Z0-9_-]*\s*\n\s*```')
for path in openspec_files:
    with open(path, 'r', encoding='utf-8') as f:
        content = f.read()
    matches = empty_cb_pattern.findall(content)
    if matches:
        print(f"EMPTY CODE BLOCK in {path}: {matches}")

# Check for empty headers: header immediately followed by another header
# Note: A section heading followed by a subsection heading like '## 2. Requirements & Scenarios\n\n### Requirement:' is standard markdown hierarchy.
# What is invalid is e.g. '### Requirement: X' followed immediately by '### Requirement: Y' without any body.
empty_req_pattern = re.compile(r'###\s+Requirement:[^\n]+\n+\s*###\s+Requirement:', re.MULTILINE)
for path in openspec_files:
    with open(path, 'r', encoding='utf-8') as f:
        content = f.read()
    matches = empty_req_pattern.findall(content)
    if matches:
        print(f"EMPTY REQUIREMENT in {path}: {matches}")

print("Placeholder & Structural checks complete.")
