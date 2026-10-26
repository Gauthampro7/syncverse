#!/usr/bin/env bash
# ==============================================================================
# Syncplay-Alt Phase 4 Dependency & Architectural Invariant Audit Script
# Authority: AGENTS.md Section 2 (Phase 4 Realignment) & ORIGINAL_REQUEST.md
#
# Enforces Architectural Invariants:
# 1. Zero Monolithic Player Wrappers: External player processes and IPC scripting
#    wrappers (mpv, vlc, gstreamer) are strictly prohibited. Syncplay-Alt is a
#    true standalone player with in-process demuxing, decoding, and presentation.
# 2. Zero External Database Engines: Relational and document databases (sqlite,
#    diesel, sqlx, sled, rocksdb, heed, tantivy, meilisearch) are strictly prohibited.
#    Smart chat persistence is built on the in-house append-only syncplay_chat.bin log
#    and custom in-memory inverted index FTS.
# 3. Permitted Frameworks: In-process multimedia decoding (ffmpeg-next, symphonia,
#    cpal, rodio) and desktop GUI presentation (eframe, egui, wgpu, winit, rfd)
#    are whitelisted.
# ==============================================================================
set -euo pipefail

# Strictly banned monolithic player wrappers & external database engines:
BANNED_PATTERNS=(
  # Monolithic External Player IPC & Scripting Wrappers
  "mpv"
  "mpv-sys"
  "libmpv"
  "vlc"
  "libvlc"
  "gstreamer"
  "gstreamer-sys"

  # Database & Heavy Search Engines (Must use custom binary chat log & inverted index)
  "sqlite"
  "sqlite3"
  "rusqlite"
  "libsqlite3-sys"
  "sqlx"
  "sled"
  "rocksdb"
  "rocksdb-sys"
  "heed"
  "tantivy"
  "meilisearch"
  "diesel"
)

# Explicitly whitelisted Phase 4 multimedia and GUI frameworks:
WHITELISTED_PATTERNS=(
  "ffmpeg"
  "ffmpeg-next"
  "ffmpeg-sys-next"
  "symphonia"
  "symphonia-core"
  "eframe"
  "egui"
  "wgpu"
  "winit"
  "rfd"
  "cpal"
  "rodio"
  "image"
  "imageproc"
  "hound"
)

# Sanity check: Ensure whitelisted patterns are not in banned patterns
for white in "${WHITELISTED_PATTERNS[@]}"; do
  for banned in "${BANNED_PATTERNS[@]}"; do
    if [ "$white" = "$banned" ]; then
      echo "[FATAL] Configuration error: '$white' is present in both WHITELISTED and BANNED lists!" >&2
      exit 2
    fi
  done
done

VIOLATIONS=0

# ------------------------------------------------------------------------------
# 1. Audit Cargo.toml manifests across workspace
# ------------------------------------------------------------------------------
echo "[INFO] Auditing Cargo.toml manifests for banned monolithic player and database engines..."

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

echo "[PASS] Dependency audit clean (${#BANNED_PATTERNS[@]} banned patterns checked, 0 violations)."
echo "[PASS] Monolithic player wrappers and external database engines are strictly excluded."
exit 0
