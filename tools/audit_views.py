from pathlib import Path

content = Path("gui/dist/index.html").read_text(encoding="utf-8")
views = [
    'id="view-overview"',
    'id="view-agents"',
    'id="scan-view"',
    'id="results-view"',
    'id="view-quarantine"',
    'id="view-detective"',
    'id="cleanup-view"',
    'id="view-eventlog"',
    'id="view-canary"',
    'id="view-incident"',
]

lines = content.splitlines()
for v in views:
    found = False
    for i, line in enumerate(lines):
        if v in line:
            print(f"{v}: line {i+1}")
            found = True
            break
    if not found:
        print(f"{v}: NOT FOUND")
