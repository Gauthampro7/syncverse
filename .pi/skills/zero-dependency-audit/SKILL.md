---
name: zero-dependency-audit
description: Operational skill for auditing Cargo manifests and lockfiles to enforce zero external media, codec, or database dependencies
version: 1.0.0
---

# Skill: Zero-Dependency Audit

## Overview
This skill defines the procedures and scripts for auditing the Syncverse workspace to ensure strict adherence to the **Zero-External-Codec Invariant**. It prevents accidental introduction of forbidden multimedia frameworks, third-party codec bindings, or external database engines.

---

## 1. Prohibited Dependencies List

The following crates and their prefixes are strictly blacklisted:

```text
# Media, Codec & Player Frameworks
ffmpeg
ffmpeg-sys
ffmpeg-next
libavcodec
libavformat
libavutil
libswscale
libswresample
gstreamer
gstreamer-sys
mpv
mpv-sys
libmpv
vlc
libvlc
symphonia
symphonia-core
dav1d
dav1d-sys
rav1e
openh264
minimp3
nom-ebml
hound
image
imageproc

# Database & Search Engines
sqlite
sqlite3
rusqlite
sqlx
sled
rocksdb
rocksdb-sys
heed
tantivy
meilisearch
diesel
```

---

## 2. Audit Procedures

### 2.1 Cargo Manifest Inspection
Inspect every `Cargo.toml` file across `crates/*/Cargo.toml` and root `Cargo.toml`.
Verify that neither `[dependencies]` nor `[dev-dependencies]` nor `[build-dependencies]` reference any prohibited crate.

### 2.2 Transitive Dependency Tree Audit
Run `cargo tree` to inspect the full transitive dependency graph:
```bash
cargo tree --workspace --all-features
```
Ensure that no indirect dependency pulls in any blacklisted crate or C-binding wrapper.

### 2.3 Automated Audit Script Implementation
The workspace enforces this rule via `scripts/audit_dependencies.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# Syncverse Zero-Dependency & Forbidden Framework Audit Script
# Authoritative specification: AGENTS.md Section 2.1
# ==============================================================================

BANNED_PATTERNS=(
  # Media, Codec & Player Frameworks
  "ffmpeg"
  "ffmpeg-sys"
  "ffmpeg-next"
  "libav"
  "libavcodec"
  "libavformat"
  "libavutil"
  "libswscale"
  "libswresample"
  "gstreamer"
  "gstreamer-sys"
  "mpv"
  "mpv-sys"
  "libmpv"
  "vlc"
  "libvlc"
  "symphonia"
  "symphonia-core"
  "dav1d"
  "dav1d-sys"
  "rav1e"
  "openh264"
  "minimp3"
  "nom-ebml"
  "hound"
  "image"
  "imageproc"

  # Database & Search Engines
  "sqlite"
  "sqlite3"
  "rusqlite"
  "sqlx"
  "sled"
  "rocksdb"
  "rocksdb-sys"
  "heed"
  "tantivy"
  "meilisearch"
  "diesel"
)

VIOLATIONS=0

# ------------------------------------------------------------------------------
# 1. Audit Cargo.toml manifests across workspace
# ------------------------------------------------------------------------------
echo "[INFO] Auditing Cargo.toml manifests for banned dependencies..."

MANIFEST_FILES=()
for f in "Cargo.toml" crates/*/Cargo.toml; do
  if [ -f "$f" ]; then
    MANIFEST_FILES+=("$f")
  fi
done

if [ ${#MANIFEST_FILES[@]} -eq 0 ]; then
  echo "[INFO] No Cargo.toml files found to inspect."
else
  for manifest in "${MANIFEST_FILES[@]}"; do
    for pattern in "${BANNED_PATTERNS[@]}"; do
      # Match:
      # - Direct dependency: ^[[:space:]]*${pattern}([[:alnum:]_-]*)[[:space:]]*(=|\.)
      # - Aliased dependency: package[[:space:]]*=[[:space:]]*["']${pattern}
      # - Bracketed table: ^[[:space:]]*\[.*${pattern}
      if sed 's/[[:space:]]*#.*$//' "$manifest" | grep -E "(^[[:space:]]*${pattern}([[:alnum:]_-]*)[[:space:]]*(=|\.)|package[[:space:]]*=[[:space:]]*[\"']${pattern}|^[[:space:]]*\[.*${pattern})" >/dev/null 2>&1; then
        echo "[ERROR] Banned dependency '${pattern}' detected in ${manifest}!"
        VIOLATIONS=$((VIOLATIONS + 1))
      fi
    done
  done
fi

# ------------------------------------------------------------------------------
# 2. Audit Cargo.lock if present
# ------------------------------------------------------------------------------
LOCKFILE="Cargo.lock"
if [ -f "$LOCKFILE" ]; then
  echo "[INFO] Auditing ${LOCKFILE} for banned dependencies..."
  for pattern in "${BANNED_PATTERNS[@]}"; do
    if grep -E "^name = \"${pattern}" "$LOCKFILE" >/dev/null 2>&1; then
      echo "[ERROR] Banned dependency '${pattern}' detected in ${LOCKFILE}!"
      VIOLATIONS=$((VIOLATIONS + 1))
    fi
  done
else
  echo "[INFO] No Cargo.lock found. Lockfile audit skipped."
fi

# ------------------------------------------------------------------------------
# 3. Final Verdict
# ------------------------------------------------------------------------------
if [ $VIOLATIONS -ne 0 ]; then
  echo "[FAIL] Found ${VIOLATIONS} banned dependency violation(s)!"
  exit 1
fi

echo "[PASS] Dependency audit clean. Zero forbidden external engines detected."
exit 0
```

---

## 3. Verification Protocol

1. Run the audit script:
   ```bash
   chmod +x scripts/audit_dependencies.sh
   ./scripts/audit_dependencies.sh
   ```
2. Verify exit code is `0`.
3. If exit code is non-zero, immediately locate the offending dependency in `Cargo.toml`, remove it, and replace it with an in-tree pure-Rust implementation.
