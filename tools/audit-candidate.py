"""Verify the portable release's file boundary, integrity and build hygiene."""
import argparse
import hashlib
from pathlib import Path
import re
import zipfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("directory", type=Path)
args = parser.parse_args()
root = args.directory.resolve()
portable = root / "portable"
expected = {"cure-gui.exe", "cure.exe", "cure-watch.exe", "README.md", "LICENSE",
            "SECURITY.md", "RELEASE-NOTES.md", "BUILD-INFO.txt", "SHA256SUMS.txt",
            "docs/media/demo.gif"}
expected.update("docs/screenshots/" + name + ".png" for name in
                ["01-overview", "02-scan", "03-review-required", "04-evidence-inspector",
                 "05-quarantine-confirm", "06-quarantine", "07-all-clear"])
actual = {p.relative_to(portable).as_posix() for p in portable.rglob("*") if p.is_file()}
assert actual == expected, ("Unexpected portable files", actual ^ expected)
zips = list(root.glob("cure-v*.zip"))
assert len(zips) == 1, "Expected one versioned ZIP"
archive = zips[0]
with zipfile.ZipFile(archive) as z:
    assert len(z.namelist()) == len(expected) and set(z.namelist()) == expected, "ZIP boundary mismatch"
    for name in expected:
        assert z.read(name) == (portable / name).read_bytes(), "ZIP differs from staged file: " + name

def verify_manifest(path, directory, expected_names):
    lines = path.read_text().splitlines()
    parsed = [line.split("  ") for line in lines]
    assert {row[1] for row in parsed} == expected_names, "Checksum names mismatch"
    for digest, name in parsed:
        assert re.fullmatch(r"[0-9a-f]{64}", digest), "Malformed digest"
        target = directory / name if name.endswith(".zip") else portable / name
        assert hashlib.sha256(target.read_bytes()).hexdigest() == digest, "Checksum mismatch: " + name

exes = {"cure-gui.exe", "cure.exe", "cure-watch.exe"}
verify_manifest(root / "SHA256SUMS.txt", root, exes | {archive.name})
verify_manifest(portable / "SHA256SUMS.txt", portable, exes)
builder_paths = [str(Path(__file__).resolve().parents[1]), str(Path.home())]
for name in exes:
    data = (portable / name).read_bytes()
    assert data.startswith(b"MZ"), "Not a Windows executable: " + name
    for local in builder_paths:
        for spelling in [local, local.replace("\\", "/"), local.replace("\\", "\\\\")]:
            for encoding in ["utf-8", "utf-16-le"]:
                assert spelling.encode(encoding) not in data, "Builder path leaked: " + name
    assert not any(marker in data for marker in [b"CURE_E2E_CLEANUP", b"CURE_E2E_EXIT"]), "Desktop test driver in production: " + name
    assert not re.search(rb"(?:ghp_[A-Za-z0-9]{36}|AKIA[A-Z0-9]{16}|-----BEGIN (?:RSA |EC )?PRIVATE KEY-----\r?\n)", data), "Potential credential marker: " + name

info = (portable / "BUILD-INFO.txt").read_text()
assert re.search(r"Source commit: [0-9a-f]{40}", info), "Missing source SHA"
assert "Unsigned" in info, "Missing unsigned disclosure"
print("PASS: 17 intended files; ZIP/staging byte identity; 7 exact manifest hashes; no builder paths, test drivers or credential markers in EXEs")
