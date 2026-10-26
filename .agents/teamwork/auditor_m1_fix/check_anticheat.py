import os
import re

print("=== CHECK 2: ANTI-CHEAT & FORBIDDEN ENGINE MENTIONS ===")
banned_terms = [
    'ffmpeg', 'libav', 'gstreamer', 'mpv', 'vlc', 'symphonia', 'dav1d',
    'sqlite', 'rusqlite', 'sqlx', 'rocksdb', 'nom-ebml'
]
banned_regex = re.compile(r'\b(' + '|'.join(banned_terms) + r')\b', re.IGNORECASE)

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

for path in all_files:
    with open(path, 'r', encoding='utf-8') as f:
        lines = f.readlines()
    for idx, line in enumerate(lines, 1):
        m = banned_regex.search(line)
        if m:
            print(f"{path}:{idx}: [{m.group(0)}] -> {line.strip()}")
