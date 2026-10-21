# C.U.R.E 0.2.0 — Windows security and storage intelligence

C.U.R.E inspects a Windows machine and reports what it found, with the
evidence attached and nothing changed behind your back.

This release adds a **Storage Workstation**: a measured view of what is
actually on the drive, where it came from, what can be reclaimed, and a
removal flow that is previewed, approved and receipted.

---

## The Storage Workstation

**Measured, not guessed.** Every figure comes from the filesystem. Sizes
are the bytes the files actually occupy on the volume, not their apparent
length, so a sparse file is not counted as though it were full.

**The headline never over-promises.** The reclaimable figure is the
*union* of the removable files: a file that is both a temporary file and a
verified duplicate contributes its bytes once. The duplicate figure is
reported as a subset of it, never added on top. When part of the machine
could not be read, the number is labelled a floor rather than a total, and
the Coverage tab names every location that was refused.

**Twelve kinds of thing are recognised**, each with its own safety tier:

| Category | Tier |
| --- | --- |
| Temporary files (user and Windows) | Safe to review |
| Browser caches (Chrome, Edge, Brave, Firefox, Chromium, Opera, Vivaldi) | Safe to review |
| Recycle Bin payloads | Safe to review |
| Crash dumps and diagnostic logs | Safe to review |
| Windows Update download cache | **Review carefully** — Windows manages it itself; the supported mechanism is named instead |
| Previous Windows installation | Review carefully |
| Developer build caches (`node_modules`, `target`, `.gradle`, `.cargo`, …) | Review carefully |
| Old downloads and old installers | Review carefully |
| Application data that looks unused | **Never** a recommendation — age alone proves nothing |
| Operating system, drivers, recovery, installed applications | Protected |

**Duplicates are verified, never assumed.** A group is reported only after
every member has been read end to end and its SHA-256 matches. The engine
suggests which copy to keep — the most recent one, or the one outside a
disposable location — and it never removes a copy on its own. A file whose
hard-link count cannot be read is never promised as reclaimable.

**Removal is previewed, bound and receipted.** The confirmation dialog
lists every file that will go and every file that was refused, and asks
for a typed word. The backend then runs *that* plan: it re-checks the scan
identity, the plan's own digest, each file's size, its tier, its location
and its identity on the volume, and it removes the file through a verified
handle rather than through a path that could have been swapped underneath
it. A file that was replaced between approval and execution is refused,
not deleted.

**The receipt is honest about what it knows.** It separates the files that
were planned, removed, refused and failed; the size those files occupied
before they were removed; the part of that whose recovery could not be
promised; and the volume's free-space change, which is labelled as an
observation of the whole drive rather than a per-file guarantee.

**Discovery does not stop at the user profile.** Build directories are
recognised wherever they live — a project on `D:\Projects` counts, not only
one under `%USERPROFILE%`. Drives and folders can be added explicitly, and
a whole-drive inventory is available with depth, entry and time budgets.

---

## LUMA, the companion

A ginger tabby with amber eyes, a violet tech collar and a white-tipped
paws, who sits in the corner of the interface and reacts to what the
product is actually doing. She is a vector rig, not a picture: a four-joint
tail, twelve authored poses, a seeded beat scheduler, and a proper curled
sleeping shape rather than a sitting cat with closed eyes.

Two hosts, one personality: the Security companion answers the Rescue scan
and the Storage companion answers the Storage Workstation. A global event
never overwrites a screen-specific companion.

She honours the operating system's reduced-motion preference, pauses when
the window is not focused, and can be petted with the mouse or the
keyboard. Petting one cat purrs one cat.

---

## The rest of the console

Unchanged in spirit, improved in substance:

* **Rescue** — nine collection sources, an X-Ray of the machine, and an
  honest coverage report: incomplete coverage never reads as clean.
* **Investigate** — every finding with the reason it was flagged, its
  source, its target, its signature and its SHA-256.
* **Quarantine** — a confirmed move with a scoped undo. The engine's own
  deletion is permanent; quarantine is not.
* **Monitor** — session events, the canary tripwire, and login observation.

---

## Installing

**Windows installer (NSIS).** Installs for the current user; no
administrator rights are required and nothing is registered as a startup
service. The installer does not disable antivirus, does not bypass
Windows security warnings, and does not install a USB watcher — pairing
and watcher installation are a separate, explicit act.

**Portable ZIP.** The same three executables and the documentation, with
no installer at all. Extract it and run `cure-gui.exe`.

> **The binaries are unsigned.** Windows will show a SmartScreen warning
> on first run. This is disclosed rather than hidden: a publisher
> certificate is a commercial decision, not a technical one, and no
> certificate is claimed.

The application keeps its own records in `%LOCALAPPDATA%\CURE`. It never
writes anything next to the executable, and it never modifies the machine
outside an action you confirmed.

---

## What is not in this release

* No code signing, and therefore a SmartScreen prompt on first run.
* The USB watcher is not installed by the installer. `cure-watch.exe` is
  shipped for operators who want it, and it is installed explicitly.
* There is no automatic update mechanism. A new release is a new download.

---

## Checksums

The installer and the portable archive for this build:

```
fddd4aefe21da570cc91e2a480615f3cdb6c295ccd534db1f6717f16b6636479  CURE_0.2.0_x64-setup.exe
33676a1d64df833f528b862538f888ddeca329a23e54abc4bf870a0bb1285455  cure-v0.2.0.zip
```

The portable archive carries its own `SHA256SUMS.txt` for the three
executables inside it. Verify them against the published assets; a checksum
proves the file is the one that was published, not who published it.
