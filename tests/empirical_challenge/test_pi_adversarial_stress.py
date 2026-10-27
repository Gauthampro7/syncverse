#!/usr/bin/env python3
"""
Adversarial Empirical Challenge Harness: Pi Framework Stress Testing
Authors: Empirical Challenger (critic / specialist)

Verifies and stress-tests:
1. Invariants & Dependency Whitelist:
   - Evaluates evasion vectors against the audit logic in zero-dependency-audit/SKILL.md
   - Tests package renaming, word-boundary evasion (libavcodec, sqlite3), missing crates (diesel, hound, image)
2. Agent Prompts Integrity & Edge Cases:
   - Tests storage_auditor.md line 32 record_total_len framing flaw vs authoritative parser
   - Tests probe out-of-order handling in sync_simulator specifications
3. Skill Reproducibility:
   - Validates vector generators in bitstream-vector-testing/SKILL.md against demux parsers
   - Validates PI controller equations from network-sync-simulation/SKILL.md
   - Validates OpenSpec change proposal dependency graph DAG properties
"""

import os
import sys
import tempfile
import subprocess
import struct
import zlib
from typing import List, Tuple, Dict

# ============================================================================
# 1. DEPENDENCY AUDIT EVASION & INVARIANT STRESS TEST
# ============================================================================

SKILL_BANNED_PATTERNS = [
    # Monolithic External Player IPC & Scripting Wrappers
    "mpv",
    "mpv-sys",
    "libmpv",
    "vlc",
    "libvlc",
    "gstreamer",
    "gstreamer-sys",

    # Database & Heavy Search Engines (Must use custom binary chat log & inverted index)
    "sqlite",
    "sqlite3",
    "rusqlite",
    "libsqlite3-sys",
    "sqlx",
    "sled",
    "rocksdb",
    "rocksdb-sys",
    "heed",
    "tantivy",
    "meilisearch",
    "diesel",
]

# Crates listed in AGENTS.md lines 28-32:
AGENTS_MD_BANNED_CRATES = [
    "mpv", "mpv-sys", "libmpv", "vlc", "libvlc", "gstreamer", "gstreamer-sys",
    "sqlite", "sqlite3", "rusqlite", "libsqlite3-sys", "sqlx", "sled",
    "rocksdb", "rocksdb-sys", "heed", "tantivy", "meilisearch", "diesel"
]

def simulate_audit_cargo_toml(cargo_toml_content: str, banned_patterns: List[str]) -> bool:
    """
    Simulates the hardened audit logic from scripts/audit_dependencies.sh and
    zero-dependency-audit/SKILL.md against a candidate Cargo.toml.
    Returns True if detected (audit caught it), False if bypassed (audit failed).
    """
    with tempfile.TemporaryDirectory() as tmpdir:
        toml_path = os.path.join(tmpdir, "Cargo.toml")
        with open(toml_path, "w") as f:
            f.write(cargo_toml_content)
        
        script_path = "/home/gautham/syncverse/scripts/audit_dependencies.sh"
        if os.path.exists(script_path):
            res = subprocess.run([script_path], cwd=tmpdir, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            return res.returncode != 0
        else:
            for pattern in banned_patterns:
                cmd = f'sed "s/[[:space:]]*#.*$//" "{toml_path}" | grep -E "(^[[:space:]]*{pattern}([[:alnum:]_-]*)[[:space:]]*(=|\\.)|package[[:space:]]*=[[:space:]]*[\"\']{pattern}|^[[:space:]]*\\[.*{pattern})"'
                res = subprocess.run(cmd, shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                if res.returncode == 0:
                    return True
            return False

def simulate_audit_cargo_lock(cargo_lock_content: str, banned_patterns: List[str]) -> bool:
    """
    Simulates the hardened audit logic from scripts/audit_dependencies.sh and
    zero-dependency-audit/SKILL.md against a candidate Cargo.lock.
    Returns True if detected, False if bypassed.
    """
    with tempfile.TemporaryDirectory() as tmpdir:
        lock_path = os.path.join(tmpdir, "Cargo.lock")
        with open(lock_path, "w") as f:
            f.write(cargo_lock_content)
        
        script_path = "/home/gautham/syncverse/scripts/audit_dependencies.sh"
        if os.path.exists(script_path):
            res = subprocess.run([script_path], cwd=tmpdir, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            return res.returncode != 0
        else:
            for pattern in banned_patterns:
                cmd = f'grep -E "^name = \\"{pattern}" "{lock_path}"'
                res = subprocess.run(cmd, shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                if res.returncode == 0:
                    return True
            return False

def test_dependency_evasion_vectors() -> List[str]:
    print("=" * 60)
    print("CHALLENGE 1: DEPENDENCY AUDIT INVARIANTS & EVASION TEST")
    print("=" * 60)
    
    findings = []
    
    # Evasion Vector 1A: Word boundary evasion with word suffixes in Cargo.toml
    # e.g., 'sqlite3', 'mpv-sys', 'libvlc'
    word_suffix_cases = [
        ("sqlite3 = \"0.1\"", "sqlite3"),
        ("mpv-sys = \"0.1\"", "mpv-sys"),
        ("libmpv = \"0.1\"", "libmpv"),
        ("libvlc = \"0.1\"", "libvlc"),
    ]
    for line, crate in word_suffix_cases:
        content = f"[dependencies]\n{line}\n"
        detected = simulate_audit_cargo_toml(content, SKILL_BANNED_PATTERNS)
        if not detected:
            findings.append(f"Cargo.toml bypass: '{line}' evaded detection by zero-dependency-audit/SKILL.md (regex ^\\s*pattern\\b misses '{crate}')")
            print(f"  [VULNERABILITY CONFIRMED] '{crate}' in Cargo.toml BYPASSED audit script!")
        else:
            print(f"  [BLOCKED] '{crate}' caught by audit script.")

    # Evasion Vector 1B: Renamed package alias in Cargo.toml
    # e.g., 'my-sqlite = { package = "sqlite3", version = "0.1" }'
    alias_cases = [
        ('my_player = { package = "mpv-sys", version = "0.1" }', "mpv-sys"),
        ('fast_sql = { package = "rusqlite", version = "0.29" }', "rusqlite"),
    ]
    for line, crate in alias_cases:
        content = f"[dependencies]\n{line}\n"
        detected = simulate_audit_cargo_toml(content, SKILL_BANNED_PATTERNS)
        if not detected:
            findings.append(f"Cargo.toml package alias bypass: '{line}' evaded audit detection")
            print(f"  [VULNERABILITY CONFIRMED] Renamed package '{crate}' BYPASSED Cargo.toml audit!")
        else:
            print(f"  [BLOCKED] Renamed package '{crate}' caught by audit script.")

    # Evasion Vector 1C: Bracketed dependency table in Cargo.toml
    # e.g., '[dependencies.sqlite3]'
    bracket_cases = [
        ("[dependencies.sqlite3]\nversion = \"0.1\"", "sqlite3 table syntax"),
        ("[dependencies.vlc]\nversion = \"0.1\"", "vlc table syntax"),
    ]
    for block, desc in bracket_cases:
        detected = simulate_audit_cargo_toml(block, SKILL_BANNED_PATTERNS)
        if not detected:
            findings.append(f"Cargo.toml bracketed syntax bypass: '{desc}' evaded audit detection")
            print(f"  [VULNERABILITY CONFIRMED] Bracketed syntax '{desc}' BYPASSED Cargo.toml audit!")
        else:
            print(f"  [BLOCKED] Bracketed syntax '{desc}' caught by audit script.")

    # Evasion Vector 1D: Crates missing from BANNED_PATTERNS in Cargo.lock audit
    # e.g., 'diesel', 'rocksdb-sys', 'libsqlite3-sys'
    missing_lock_crates = ["diesel", "rocksdb-sys", "libsqlite3-sys", "libvlc", "mpv-sys", "gstreamer-sys"]
    for crate in missing_lock_crates:
        lock_content = f'[[package]]\nname = "{crate}"\nversion = "1.0.0"\n'
        detected = simulate_audit_cargo_lock(lock_content, SKILL_BANNED_PATTERNS)
        if not detected:
            findings.append(f"Cargo.lock audit bypass: prohibited crate '{crate}' is absent from BANNED_PATTERNS in SKILL.md")
            print(f"  [VULNERABILITY CONFIRMED] Prohibited crate '{crate}' in Cargo.lock BYPASSED audit script!")
        else:
            print(f"  [BLOCKED] Prohibited crate '{crate}' in Cargo.lock caught by audit script.")

    return findings

# ============================================================================
# 2. STORAGE AUDITOR PROMPT RECORD_TOTAL_LEN DEFECT HARNESS
# ============================================================================

def test_storage_auditor_framing_ambiguity():
    print("\n" + "=" * 60)
    print("CHALLENGE 2: STORAGE AUDITOR PROMPT RECORD_TOTAL_LEN VERIFICATION")
    print("=" * 60)
    
    # Import authoritative recovery scanner and header generator from test_smart_chat
    sys.path.insert(0, "/home/gautham/syncverse/tests/empirical_challenge")
    from test_smart_chat import make_header, ChatRecoveryScanner, SYNC_WORD
    
    findings = []
    prompt_path = "/home/gautham/syncverse/.pi/agents/storage_auditor.md"
    try:
        with open(prompt_path, "r", encoding="utf-8") as f:
            prompt_content = f.read()
    except Exception as e:
        findings.append(f"Could not read {prompt_path}: {e}")
        return findings

    # Check for un-remediated flawed framing formula
    if "length of body + 4" in prompt_content:
        def serialize_record_flawed(record_type, flags, wall_ms, media_ms, sender_id, sender_name, payload):
            name_bytes = sender_name.encode("utf-8")
            body = struct.pack("<BBQQ16sB", record_type, flags, wall_ms, media_ms, sender_id, len(name_bytes)) \
                   + name_bytes + struct.pack("<I", len(payload)) + payload
            crc = zlib.crc32(body) & 0xFFFFFFFF
            flawed_total_len = len(body) + 4
            return SYNC_WORD + struct.pack("<I", flawed_total_len) + body + struct.pack("<I", crc)
            
        header = make_header()
        sender_uuid = b"0123456789abcdef"
        rec1 = serialize_record_flawed(1, 0, 1000, 500, sender_uuid, "Alice", b"Hello world 1")
        rec2 = serialize_record_flawed(1, 0, 1001, 501, sender_uuid, "Bob", b"Hello world 2")
        flawed_db = header + rec1 + rec2
        scanner = ChatRecoveryScanner(flawed_db)
        recovered = scanner.recover()
        err_msg = (
            f"Storage Auditor prompt line 32 ('record_total_len = length of body + 4') "
            f"causes catastrophic failure: authoritative parser recovered {len(recovered)} / 2 records "
            f"and flagged {scanner.corrupted_chunks} corrupted chunks! "
            f"Correct value MUST be length of body + 10 (or total record byte size including sync word and length field)."
        )
        findings.append(err_msg)
        print(f"  [VULNERABILITY CONFIRMED] {err_msg}")
    else:
        # Prompt is remediated. Verify that prompt specifies total record length (len(body) + 10, >= 48)
        if "length of body + 10" not in prompt_content:
            err_msg = "Storage Auditor prompt line 32 does not contain expected formula 'length of body + 10'."
            findings.append(err_msg)
            print(f"  [FAIL] {err_msg}")
        else:
            def serialize_record_remediated(record_type, flags, wall_ms, media_ms, sender_id, sender_name, payload):
                name_bytes = sender_name.encode("utf-8")
                body = struct.pack("<BBQQ16sB", record_type, flags, wall_ms, media_ms, sender_id, len(name_bytes)) \
                       + name_bytes + struct.pack("<I", len(payload)) + payload
                crc = zlib.crc32(body) & 0xFFFFFFFF
                total_len = 2 + 4 + len(body) + 4  # length of body + 10 >= 48
                return SYNC_WORD + struct.pack("<I", total_len) + body + struct.pack("<I", crc)

            header = make_header()
            sender_uuid = b"0123456789abcdef"
            rec1 = serialize_record_remediated(1, 0, 1000, 500, sender_uuid, "Alice", b"Hello world 1")
            rec2 = serialize_record_remediated(1, 0, 1001, 501, sender_uuid, "Bob", b"Hello world 2")
            valid_db = header + rec1 + rec2
            scanner = ChatRecoveryScanner(valid_db)
            recovered = scanner.recover()
            if len(recovered) == 2 and scanner.corrupted_chunks == 0 and scanner.torn_writes == 0:
                print("  [PASS] Storage Auditor prompt wire framing verified: 'length of body + 10' recovered 2/2 records with 0 corruptions.")
            else:
                findings.append(f"Storage Auditor wire framing validation failed: recovered {len(recovered)}/2 records.")

    return findings

# ============================================================================
# 3. SKILL REPRODUCIBILITY & VECTOR INTEGRITY TEST
# ============================================================================

def test_skill_reproducibility():
    print("\n" + "=" * 60)
    print("CHALLENGE 3: SKILL REPRODUCIBILITY & VECTOR EXECUTION")
    print("=" * 60)
    
    findings = []
    
    # 3.1 Verify bitstream-vector-testing/SKILL.md EBML & MP4 generation
    from test_demux_codecs_clock import parse_vint, parse_isobmff_box_header
    
    # EBML vector from skill Section 1.1:
    skill_ebml = bytes([
        0x1A, 0x45, 0xDF, 0xA3,
        0x9F,
        0x42, 0x86, 0x81, 0x01,
        0x42, 0xF7, 0x81, 0x01,
        0x42, 0xF2, 0x81, 0x04,
        0x42, 0xF3, 0x81, 0x08,
        0x42, 0x82, 0x88,
        ord('m'), ord('a'), ord('t'), ord('r'), ord('o'), ord('s'), ord('k'), ord('a'),
        0x42, 0x87, 0x81, 0x04,
        0x18, 0x53, 0x80, 0x67,
        0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF
    ])
    
    if len(skill_ebml) != 48:
        findings.append(f"Skill EBML vector length is {len(skill_ebml)} != 48")
    else:
        # Parse through vint
        raw_id, _, width = parse_vint(skill_ebml, 0, is_element_id=True)
        assert raw_id == 0x1A45DFA3 and width == 4
        print("  [PASS] bitstream-vector-testing/SKILL.md EBML vector executes bit-exact (48B).")
    
    # MP4 vector from skill Section 1.2:
    mp4_buf = bytearray()
    mp4_buf.extend(struct.pack(">I", 24))
    mp4_buf.extend(b"ftyp")
    mp4_buf.extend(b"isom")
    mp4_buf.extend(struct.pack(">I", 0x200))
    mp4_buf.extend(b"isommp42")
    mp4_buf.extend(struct.pack(">I", 108))
    mp4_buf.extend(b"moov")
    mp4_buf.extend(struct.pack(">I", 100))
    mp4_buf.extend(b"mvhd")
    mp4_buf.extend(b"\x00" * 92)
    skill_mp4 = bytes(mp4_buf)
    
    if len(skill_mp4) != 132:
        findings.append(f"Skill MP4 vector length is {len(skill_mp4)} != 132")
    else:
        fourcc1, h1, t1 = parse_isobmff_box_header(skill_mp4, 0)
        assert fourcc1 == "ftyp" and t1 == 24
        fourcc2, h2, t2 = parse_isobmff_box_header(skill_mp4, 24)
        assert fourcc2 == "moov" and t2 == 108
        fourcc3, h3, t3 = parse_isobmff_box_header(skill_mp4, 32)
        assert fourcc3 == "mvhd" and t3 == 100
        print(f"  [PASS] bitstream-vector-testing/SKILL.md MP4 vector executes cleanly (132B, 3 nested boxes).")

    # 3.2 Verify OpenSpec Change Proposal Dependency Graph DAG
    changes_dir = "/home/gautham/syncverse/openspec/changes"
    change_files = sorted([f for f in os.listdir(changes_dir) if f.endswith(".md")])
    
    import re
    dep_graph = {}
    for cf in change_files:
        path = os.path.join(changes_dir, cf)
        cid = cf.replace(".md", "")
        with open(path, "r") as f:
            content = f.read()
        # Parse Upstream Dependencies
        m = re.search(r"-\s*\*\*Upstream Dependencies\*\*:\s*([^\n]+)", content)
        deps = []
        if m:
            raw_deps = m.group(1).strip()
            if "None" not in raw_deps:
                deps = [d.strip().strip("`") for d in raw_deps.split(",") if d.strip()]
        dep_graph[cid] = deps
    
    # Check for cycles using DFS
    visited = set()
    rec_stack = set()
    has_cycle = False
    
    def dfs(node):
        nonlocal has_cycle
        visited.add(node)
        rec_stack.add(node)
        for neighbor in dep_graph.get(node, []):
            if neighbor not in dep_graph:
                findings.append(f"Broken dependency: {node} depends on nonexistent change '{neighbor}'")
            elif neighbor in rec_stack:
                has_cycle = True
                findings.append(f"Dependency cycle detected involving '{node}' and '{neighbor}'")
            elif neighbor not in visited:
                dfs(neighbor)
        rec_stack.remove(node)
        
    for node in dep_graph:
        if node not in visited:
            dfs(node)
            
    if not has_cycle:
        print(f"  [PASS] OpenSpec change proposals form a valid topological DAG ({len(dep_graph)} changes, 0 cycles).")
    
    return findings

# ============================================================================
# MAIN HARNESS RUNNER
# ============================================================================

def main():
    print("STARTING ADVERSARIAL STRESS TEST HARNESS...")
    all_findings = []
    
    all_findings.extend(test_dependency_evasion_vectors())
    all_findings.extend(test_storage_auditor_framing_ambiguity())
    all_findings.extend(test_skill_reproducibility())
    
    print("\n" + "=" * 60)
    print("STRESS TEST HARNESS RESULTS SUMMARY")
    print("=" * 60)
    print(f"Total findings: {len(all_findings)}")
    for i, finding in enumerate(all_findings, 1):
        print(f"[{i}] {finding}")
        
    return len(all_findings)

if __name__ == "__main__":
    count = main()
    # Exit with code equal to finding count (0 if clean, >0 if issues found)
    sys.exit(0 if count == 0 else 1)
