import re
from pathlib import Path

dist = Path("gui/dist")
emoji_pattern = re.compile(r'[\U00010000-\U0010ffff]|[\u2600-\u27bf]')

found_emojis = []
for f in dist.glob("*.html"):
    text = f.read_text(encoding="utf-8")
    for i, line in enumerate(text.splitlines()):
        m = emoji_pattern.findall(line)
        if m:
            found_emojis.append((f.name, i+1, m, line.strip()[:100]))

for f in dist.glob("*.js"):
    text = f.read_text(encoding="utf-8")
    for i, line in enumerate(text.splitlines()):
        m = emoji_pattern.findall(line)
        if m:
            found_emojis.append((f.name, i+1, m, line.strip()[:100]))

print(f"Total emoji lines found: {len(found_emojis)}")
for item in found_emojis[:20]:
    print(f"  {item[0]}:{item[1]} {item[2]} -> {item[3]}")

# Also search for companion/mascot/luma
mascot_matches = []
for f in dist.glob("*.*"):
    if f.suffix in [".html", ".js", ".css"]:
        text = f.read_text(encoding="utf-8")
        for i, line in enumerate(text.splitlines()):
            if any(k in line.lower() for k in ["data-companion", "luma-cat", "operator-mount", "luma_cat"]):
                mascot_matches.append((f.name, i+1, line.strip()[:100]))

print(f"\nTotal mascot matches: {len(mascot_matches)}")
for item in mascot_matches[:20]:
    print(f"  {item[0]}:{item[1]} -> {item[2]}")
