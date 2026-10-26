import os
import re

print("=== CHECK 5: CHANGE PROPOSAL DAG & CRATE ORDERING ===")

changes_dir = 'openspec/changes'
change_files = sorted([f for f in os.listdir(changes_dir) if f.endswith('.md')])

graph = {}
node_metadata = {}

for cf in change_files:
    path = os.path.join(changes_dir, cf)
    node_name = cf.replace('.md', '')
    with open(path, 'r', encoding='utf-8') as f:
        content = f.read()

    # Look for dependencies in metadata or frontmatter or text
    # e.g., Dependencies: None or Dependencies: 001-demuxer-bitstream
    deps = []
    dep_match = re.search(r'Dependencies:\s*([^\n]+)', content, re.IGNORECASE)
    if dep_match:
        dep_str = dep_match.group(1).strip()
        if dep_str.lower() not in ['none', 'n/a', '[]']:
            # extract identifiers like 001-demuxer-bitstream
            extracted = re.findall(r'00\d-[a-z0-9-]+', dep_str)
            deps = extracted

    # Target crates / Crate:
    crates = []
    crate_matches = re.findall(r'`(syncplay-[a-z0-9-]+)`', content)
    # Filter unique crates
    for c in crate_matches:
        if c not in crates:
            crates.append(c)

    graph[node_name] = deps
    node_metadata[node_name] = {
        'path': path,
        'dependencies': deps,
        'crates': crates
    }
    print(f"Node: {node_name}")
    print(f"  Explicit Dependencies: {deps}")
    print(f"  Crates referenced: {crates}")

# Check DAG cycle detection
visited = {} # 0: unvisited, 1: visiting, 2: visited
has_cycle = False
cycle_nodes = []

def dfs(node, path):
    global has_cycle
    visited[node] = 1
    path.append(node)
    for neighbor in graph.get(node, []):
        if neighbor not in graph:
            print(f"ERROR: Referenced dependency {neighbor} does not exist!")
            continue
        if visited.get(neighbor, 0) == 1:
            has_cycle = True
            cycle_nodes.append(list(path) + [neighbor])
        elif visited.get(neighbor, 0) == 0:
            dfs(neighbor, path)
    path.pop()
    visited[node] = 2

for node in graph:
    visited[node] = 0

for node in graph:
    if visited[node] == 0:
        dfs(node, [])

print(f"\nCycle detection result: {'CYCLE DETECTED: ' + str(cycle_nodes) if has_cycle else 'PASS (Acyclic Graph)'}")

# Topological Sort
in_degree = {n: 0 for n in graph}
for n in graph:
    for dep in graph[n]:
        # Note: If A depends on B, then B must be built before A.
        # So edge is dep -> n
        in_degree[n] += 1

adj = {n: [] for n in graph}
for n in graph:
    for dep in graph[n]:
        adj[dep].append(n)

from collections import deque
queue = deque([n for n in graph if in_degree[n] == 0])
topo_order = []
while queue:
    curr = queue.popleft()
    topo_order.append(curr)
    for nxt in adj.get(curr, []):
        in_degree[nxt] -= 1
        if in_degree[nxt] == 0:
            queue.append(nxt)

print(f"Topological Execution Order: {topo_order}")
print(f"All {len(graph)} nodes included: {len(topo_order) == len(graph)}")

# Verify forward-pointing numbering: each change proposal 00X only depends on 00Y where Y < X
strictly_forward = True
for node, deps in graph.items():
    node_num = int(node.split('-')[0])
    for dep in deps:
        dep_num = int(dep.split('-')[0])
        if dep_num >= node_num:
            print(f"VIOLATION: {node} depends on forward or equal change {dep} ({dep_num} >= {node_num})")
            strictly_forward = False

if strictly_forward:
    print("PASS: Strictly forward-pointing dependency DAG (every change 00X depends only on changes 00Y with Y < X).")
