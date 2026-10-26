import os
import re

print("=== CHECK 3 & 4: RFC-2119 NORMATIVE KEYWORDS & SCENARIOS ===")

spec_dirs = [
    'openspec/specs/demuxer/spec.md',
    'openspec/specs/codecs/spec.md',
    'openspec/specs/av-pipeline/spec.md',
    'openspec/specs/sync-engine/spec.md',
    'openspec/specs/smart-chat/spec.md',
    'openspec/specs/ui-renderer/spec.md'
]

rfc2119_pattern = re.compile(r'\b(MUST|SHALL|MUST NOT|SHALL NOT|SHOULD|SHOULD NOT|MAY|REQUIRED)\b')

total_requirements = 0
total_scenarios = 0
errors = []

for spec_path in spec_dirs:
    print(f"\n--- Checking Spec: {spec_path} ---")
    with open(spec_path, 'r', encoding='utf-8') as f:
        content = f.read()

    # Split by ### Requirement:
    req_splits = re.split(r'(?=###\s+Requirement:)', content)
    req_blocks = [r for r in req_splits if r.strip().startswith('### Requirement:')]
    print(f"Found {len(req_blocks)} Requirement blocks.")
    total_requirements += len(req_blocks)

    for r_idx, block in enumerate(req_blocks, 1):
        lines = block.strip().split('\n')
        req_title = lines[0].strip()

        # Find the text before the first #### Scenario:
        scenario_splits = re.split(r'(?=####\s+Scenario:)', block)
        req_body = scenario_splits[0]
        scenarios = scenario_splits[1:]

        # Verify RFC-2119 keywords in req_body
        keywords_found = rfc2119_pattern.findall(req_body)
        if not keywords_found:
            err = f"ERROR: No RFC-2119 keywords found in {req_title} ({spec_path})"
            print(err)
            errors.append(err)
        else:
            # print(f"  Req {r_idx}: {req_title} -> RFC-2119 keywords: {set(keywords_found)}")
            pass

        # Verify scenarios
        if not scenarios:
            err = f"ERROR: Zero scenarios found for {req_title} ({spec_path})"
            print(err)
            errors.append(err)
        else:
            for s_idx, sc in enumerate(scenarios, 1):
                total_scenarios += 1
                sc_lines = sc.strip().split('\n')
                sc_title = sc_lines[0].strip()

                has_given = bool(re.search(r'\bGiven\b', sc, re.IGNORECASE))
                has_when = bool(re.search(r'\bWhen\b', sc, re.IGNORECASE))
                has_then = bool(re.search(r'\bThen\b', sc, re.IGNORECASE))

                if not (has_given and has_when and has_then):
                    err = f"ERROR: Scenario missing Given/When/Then: {sc_title} in {spec_path} (Given={has_given}, When={has_when}, Then={has_then})"
                    print(err)
                    errors.append(err)

print("\n================ SUMMARY ================")
print(f"Total Specifications checked: {len(spec_dirs)}")
print(f"Total Requirements checked: {total_requirements}")
print(f"Total Scenarios checked: {total_scenarios}")
print(f"Total Errors: {len(errors)}")
if errors:
    print("FAILED checks:")
    for e in errors:
        print("  - " + e)
else:
    print("PASS: 100% of requirements have RFC-2119 keywords and 100% have valid Given/When/Then scenarios!")
