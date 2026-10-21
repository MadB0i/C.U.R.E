"""Verify a release candidate's file boundary, integrity and build hygiene.

This is a **blocking gate**. A candidate that fails it must not be
published, and the release workflow runs it as a required step.

What it proves:

* The candidate contains exactly the files it is supposed to contain — no
  development mock, no preview page, no test fixture, no stray build
  output. The expected set is derived from the repository, not hard-coded,
  so a renamed screenshot cannot silently break the gate or let something
  unexpected through.
* The ZIP and the staged tree are byte-identical.
* Every file in the checksum manifests is present and hashes correctly.
* The executables are real Windows binaries that contain no builder path,
  no desktop test driver and no credential marker.
* The build information names the source commit and discloses that the
  binaries are unsigned.
"""

import argparse
import hashlib
import re
import zipfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]

# The executables a candidate must carry.
EXECUTABLES = {"cure-gui.exe", "cure.exe", "cure-watch.exe"}

# The fixed documents a candidate must carry.
DOCUMENTS = {
    "README.md",
    "LICENSE",
    "SECURITY.md",
    "RELEASE-NOTES.md",
    "BUILD-INFO.txt",
    "SHA256SUMS.txt",
    "docs/media/demo.gif",
}

# Files that must never reach a candidate, whatever else is there.
FORBIDDEN = {
    "mock-tauri.js",
    "index.dev.html",
    "storage-core.js",
    "storage.css",
    "LUMA-PREVIEW.html",
    "e2e-result.json",
    "baseline.json",
}


def expected_screenshots() -> set[str]:
    """The documentation screenshots, taken from the repository.

    The set is derived rather than hard-coded so that renaming a
    screenshot cannot break the gate — and so that a screenshot which was
    deleted cannot keep being demanded.
    """
    directory = REPO / "docs" / "screenshots"
    if not directory.is_dir():
        raise AssertionError("docs/screenshots is missing from the repository")
    return {
        "docs/screenshots/" + p.name
        for p in directory.iterdir()
        if p.is_file() and p.suffix.lower() == ".png"
    }


def verify_manifest(path: Path, portable: Path, root: Path, expected_names: set[str]) -> None:
    lines = path.read_text(encoding="utf-8").splitlines()
    parsed = [line.split("  ", 1) for line in lines if line.strip()]
    assert {row[1] for row in parsed} == expected_names, (
        "Checksum names mismatch",
        sorted(expected_names ^ {row[1] for row in parsed}),
    )
    for digest, name in parsed:
        assert re.fullmatch(r"[0-9a-f]{64}", digest), "Malformed digest: " + name
        target = root / name if name.endswith(".zip") else portable / name
        assert target.exists(), "Checksummed file is missing: " + name
        assert hashlib.sha256(target.read_bytes()).hexdigest() == digest, (
            "Checksum mismatch: " + name
        )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    root = args.directory.resolve()
    portable = root / "portable"
    assert portable.is_dir(), "The candidate has no portable/ directory: " + str(portable)

    expected = EXECUTABLES | DOCUMENTS | expected_screenshots()
    actual = {p.relative_to(portable).as_posix() for p in portable.rglob("*") if p.is_file()}
    assert actual == expected, (
        "Unexpected portable files",
        sorted(actual ^ expected),
    )
    leaked = {name for name in actual if Path(name).name in FORBIDDEN}
    assert not leaked, "Development or test files reached the candidate: " + str(sorted(leaked))

    zips = list(root.glob("cure-v*.zip"))
    assert len(zips) == 1, "Expected exactly one versioned ZIP, found " + str(len(zips))
    archive = zips[0]
    with zipfile.ZipFile(archive) as z:
        assert set(z.namelist()) == expected, "ZIP boundary mismatch"
        for name in expected:
            assert z.read(name) == (portable / name).read_bytes(), (
                "ZIP differs from staged file: " + name
            )

    verify_manifest(root / "SHA256SUMS.txt", portable, root, EXECUTABLES | {archive.name})
    verify_manifest(portable / "SHA256SUMS.txt", portable, portable, EXECUTABLES)

    builder_paths = [str(REPO), str(Path.home())]
    for name in EXECUTABLES:
        data = (portable / name).read_bytes()
        assert data.startswith(b"MZ"), "Not a Windows executable: " + name
        for local in builder_paths:
            for spelling in [local, local.replace("\\", "/"), local.replace("\\", "\\\\")]:
                for encoding in ["utf-8", "utf-16-le"]:
                    assert spelling.encode(encoding) not in data, (
                        "Builder path leaked into " + name
                    )
        assert not any(
            marker in data for marker in [b"CURE_E2E_CLEANUP", b"CURE_E2E_EXIT"]
        ), "Desktop test driver in production: " + name
        assert not re.search(
            rb"(?:ghp_[A-Za-z0-9]{36}|AKIA[A-Z0-9]{16}|"
            rb"-----BEGIN (?:RSA |EC )?PRIVATE KEY-----\r?\n)",
            data,
        ), "Potential credential marker: " + name

    info = (portable / "BUILD-INFO.txt").read_text(encoding="utf-8")
    assert re.search(r"Source commit: [0-9a-f]{40}", info), "Missing source SHA"
    assert "Unsigned" in info, "Missing unsigned disclosure"

    print(
        f"PASS: {len(expected)} intended files; ZIP/staging byte identity; "
        f"{len(EXECUTABLES) + 1} exact manifest hashes; no builder paths, "
        "test drivers or credential markers in EXEs"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
