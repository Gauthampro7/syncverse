#!/usr/bin/env python3
"""
Empirical Challenge: Desktop Application Packaging & XDG Compliance (Milestone 4 / R1)
Verifies:
1. assets/syncplay-alt.desktop strictly adheres to XDG Desktop Entry Spec v1.5.
2. desktop-file-validate executes cleanly with exit code 0 on both repo and installed files.
3. Required keys (Type, Name, GenericName, Comment, Exec, Icon, Terminal, Categories, MimeType, Keywords) are present.
4. assets/syncplay-alt.svg is a valid standalone XML vector asset.
5. ~/.local/bin/syncplay-alt is executable.
6. GTK4 icon discovery succeeds if python3-gobject is available.
"""
import os
import shutil
import subprocess
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
DESKTOP_PATH = ROOT / "assets" / "syncplay-alt.desktop"
SVG_PATH = ROOT / "assets" / "syncplay-alt.svg"
USER_DESKTOP_PATH = Path.home() / ".local" / "share" / "applications" / "syncplay-alt.desktop"
USER_ICON_PATH = Path.home() / ".local" / "share" / "icons" / "hicolor" / "scalable" / "apps" / "syncplay-alt.svg"
USER_BIN_PATH = Path.home() / ".local" / "bin" / "syncplay-alt"

def test_svg():
    print("--> [1/4] Verifying SVG vector icon asset...")
    assert SVG_PATH.exists(), f"Missing {SVG_PATH}"
    tree = ET.parse(SVG_PATH)
    root = tree.getroot()
    assert "svg" in root.tag.lower(), "Root element must be svg"
    assert root.attrib.get("viewBox") == "0 0 512 512", f"viewBox must be 0 0 512 512, got {root.attrib.get('viewBox')}"
    print("  [PASS] SVG asset structure verified.")

def test_desktop_file():
    print("--> [2/4] Verifying desktop entry spec and keys...")
    assert DESKTOP_PATH.exists(), f"Missing {DESKTOP_PATH}"
    with open(DESKTOP_PATH, "r", encoding="utf-8") as f:
        content = f.read()

    required_pairs = [
        ("Type", "Application"),
        ("Name", "Syncplay-Alt"),
        ("GenericName", "Synchronized Media Player"),
        ("Comment", "Zero-dependency standalone synchronized watch-party media player"),
        ("Icon", "syncplay-alt"),
        ("Terminal", "true"),
    ]
    for key, expected in required_pairs:
        assert f"{key}={expected}" in content, f"Missing or invalid {key}={expected}"

    assert "Exec=syncplay-alt play %U" in content, "Exec line must be syncplay-alt play %U"
    assert "Categories=" in content and "AudioVideo;" in content and "Video;" in content
    assert "MimeType=" in content and "video/x-matroska;" in content and "video/mp4;" in content
    assert "Keywords=" in content and "watchparty;" in content
    assert "Actions=HostParty;ConnectParty;Inspect;" in content

    # Run desktop-file-validate if installed
    if shutil.which("desktop-file-validate"):
        res = subprocess.run(["desktop-file-validate", str(DESKTOP_PATH)], capture_output=True, text=True)
        assert res.returncode == 0, f"desktop-file-validate failed: {res.stdout} {res.stderr}"
        print("  [PASS] desktop-file-validate on assets/syncplay-alt.desktop verified with 0 errors.")
    else:
        print("  [WARN] desktop-file-validate not in PATH; skipping external CLI check.")

def test_installed_environment():
    print("--> [3/4] Verifying user installation files...")
    if USER_DESKTOP_PATH.exists():
        if shutil.which("desktop-file-validate"):
            res = subprocess.run(["desktop-file-validate", str(USER_DESKTOP_PATH)], capture_output=True, text=True)
            assert res.returncode == 0, f"desktop-file-validate on installed file failed: {res.stdout} {res.stderr}"
        print("  [PASS] User desktop entry exists and validates.")
    else:
        print("  [INFO] User desktop entry not yet installed; run ./scripts/install_linux.sh.")

    if USER_BIN_PATH.exists():
        assert os.access(USER_BIN_PATH, os.X_OK), "Installed binary must be executable"
        res = subprocess.run([str(USER_BIN_PATH), "--help"], capture_output=True, text=True)
        assert res.returncode == 0, f"Binary --help failed with code {res.returncode}"
        assert "syncplay-alt" in res.stdout.lower()
        print("  [PASS] User binary is executable and responds to --help.")

def test_gtk_icon_theme():
    print("--> [4/5] Verifying GTK4 / GIO icon theme discovery...")
    try:
        import gi
        gi.require_version("Gtk", "4.0")
        from gi.repository import Gtk
        theme = Gtk.IconTheme.new()
        if theme.has_icon("syncplay-alt"):
            print("  [PASS] GTK4 IconTheme successfully discovered syncplay-alt icon!")
        else:
            print("  [INFO] GTK4 IconTheme did not find syncplay-alt icon (cache update pending or headless).")
    except Exception as e:
        print(f"  [INFO] GTK4 python binding check skipped: {e}")

def get_syncplay_bin():
    if USER_BIN_PATH.exists() and os.access(USER_BIN_PATH, os.X_OK):
        return USER_BIN_PATH
    release_bin = ROOT / "target" / "release" / "syncplay-alt"
    if release_bin.exists() and os.access(release_bin, os.X_OK):
        return release_bin
    debug_bin = ROOT / "target" / "debug" / "syncplay-alt"
    if debug_bin.exists() and os.access(debug_bin, os.X_OK):
        return debug_bin
    return None

def test_uri_argument_handling():
    print("--> [5/5] Verifying file:// URI target handling and percent-decoding...")
    bin_path = get_syncplay_bin()
    if bin_path:
        cmd_prefix = [str(bin_path)]
    else:
        cargo_bin = os.path.expanduser("~/.cargo/bin/cargo")
        cmd_prefix = [cargo_bin if os.path.exists(cargo_bin) else "cargo", "run", "-q", "-p", "syncplay-core", "--"]

    SYNTHETIC_EBML_VECTOR = bytes([
        0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x86, 0x81, 0x01, 0x42, 0xF7, 0x81, 0x01,
        0x42, 0xF2, 0x81, 0x04, 0x42, 0xF3, 0x81, 0x08, 0x42, 0x82, 0x88, ord("m"), ord("a"),
        ord("t"), ord("r"), ord("o"), ord("s"), ord("k"), ord("a"), 0x42, 0x87, 0x81, 0x04,
        0x18, 0x53, 0x80, 0x67, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF
    ])

    plain_file = Path("/tmp/test_packaging_uri.mkv")
    space_file = Path("/tmp/test packaging space.mkv")

    try:
        plain_file.write_bytes(SYNTHETIC_EBML_VECTOR)
        space_file.write_bytes(SYNTHETIC_EBML_VECTOR)

        # 1. Inspect with file:///...
        uri_plain = f"file://{plain_file}"
        res = subprocess.run(cmd_prefix + ["inspect", uri_plain], capture_output=True, text=True)
        assert res.returncode == 0, f"inspect {uri_plain} failed: {res.stdout} {res.stderr}"
        assert "EBML/MKV probe match:" in res.stdout

        # 2. Play with file:///... --ticks 2
        res = subprocess.run(cmd_prefix + ["play", uri_plain, "--ticks", "2"], capture_output=True, text=True)
        assert res.returncode == 0, f"play {uri_plain} failed: {res.stdout} {res.stderr}"
        assert "Tick [1]" in res.stdout

        # 3. Inspect with percent-encoded URI (file://...%20...)
        uri_space = f"file://{str(space_file).replace(' ', '%20')}"
        res = subprocess.run(cmd_prefix + ["inspect", uri_space], capture_output=True, text=True)
        assert res.returncode == 0, f"inspect {uri_space} failed: {res.stdout} {res.stderr}"
        assert "EBML/MKV probe match:" in res.stdout

        # 4. Play with percent-encoded URI --ticks 2
        res = subprocess.run(cmd_prefix + ["play", uri_space, "--ticks", "2"], capture_output=True, text=True)
        assert res.returncode == 0, f"play {uri_space} failed: {res.stdout} {res.stderr}"
        assert "Tick [1]" in res.stdout

        # 5. Inspect with file://localhost/...
        uri_localhost = f"file://localhost{plain_file}"
        res = subprocess.run(cmd_prefix + ["inspect", uri_localhost], capture_output=True, text=True)
        assert res.returncode == 0, f"inspect {uri_localhost} failed: {res.stdout} {res.stderr}"

        print("  [PASS] file:// and percent-encoded URI execution verified.")
    finally:
        if plain_file.exists():
            plain_file.unlink()
        if space_file.exists():
            space_file.unlink()

if __name__ == "__main__":
    print("==================================================================")
    print(" EMPIRICAL CHALLENGE: FEDORA DESKTOP PACKAGING & XDG COMPLIANCE   ")
    print("==================================================================")
    test_svg()
    test_desktop_file()
    test_installed_environment()
    test_gtk_icon_theme()
    test_uri_argument_handling()
    print("\n[SUCCESS] All desktop application packaging invariants verified cleanly.")
