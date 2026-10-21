# Security Policy

## Reporting a vulnerability

If you find something exploitable in C.U.R.E, use the repository's private
reporting facility: [GitHub security advisories](https://github.com/MadB0i/C.U.R.E/security/advisories).
Please do not open a public issue first. Include the affected version,
reproduction steps, and what an attacker gains.

Do not include real credentials or unredacted host evidence in any report.
Scan exports redact `%USERPROFILE%` and the username by default; keep that
redaction on unless a specific path must be named.

You will get an acknowledgement within a few days. Fixes land in a patch release; there is no
paid support tier and no SLA, so set expectations accordingly.

**Storage cleanup is in scope in particular.** A bypass of the preview,
confirmation, tier, root-confinement or re-validation guards; a path
traversal or reparse-point escape that reaches outside an authorised root; or
a reclaim estimate that reports bytes as freed that were not, are all
reportable defects.

## In scope

C.U.R.E routinely runs elevated, moves files around, and restores them later. That is where the
interesting bugs live:

- **Quarantine or undo escaping its intended scope** — writing, moving, or restoring outside the
  data directory, or crossing a reparse point/junction into a directory the user never selected
- **Time-of-check-to-time-of-use in the scan → verdict → quarantine path**, including a file being
  swapped after it is hashed
- **Undo restoring attacker-controlled content**, or an ACL snapshot being manipulated so undo
  restores different bytes than it recorded
- **Integrity-check bypass** — a quarantined file whose size/hash no longer matches its record
  being reported as clean
- **Privilege handling** — anything that causes C.U.R.E to retain elevated rights after a scan, or
  to perform a remediation step without the confirmation it is supposed to require
- **Injection into rendered output** — the overlay card, reports, or logs executing content taken
  from a scanned filename, service name, or WMI subscription
- **Storage cleanup guard bypass** — removing a file outside the preview, outside an authorised scan
  root, at a tier that should be refused, without the confirmation, or after the target changed in a
  way the re-validation should have caught
- **A reclaim or duplicate figure that misstates what was measured** — reporting bytes as reclaimed
  that were not, or a duplicate group that is not byte-identical

## Out of scope

- **Detection completeness.** Missing something is the expected failure mode of a heuristic scanner,
  not a vulnerability. If it missed something, that is a detection bug worth an issue, not a
  security advisory.
- Anything requiring an attacker who already has code execution as the user, or physical access to
  the machine.
- Social engineering, and malicious use of C.U.R.E against someone else's machine. This tool is for
  machines you own or are authorised to service.
- Denial of service, and defects in Windows itself or in the antivirus suites C.U.R.E defers to.

## Design notes

These are properties the code is built around, and therefore things worth attacking:

- **Move, never delete.** Quarantine relocates files and records size + hash. Undo restores
  byte-identical, ACLs included, or tells you it could not.
- **Nothing is remediated silently.** Quarantine asks first. Registry, services, and WMI are never
  touched automatically — C.U.R.E will tell you the manual backup-first steps instead.
- **Local-first.** No telemetry, no analytics, no network calls. A finding never leaves the
  machine unless you send it.
- Every finding ships with the evidence that produced it, so a verdict can be checked rather than
  trusted.

## Safe harbor

Good-faith research is fine. Do not access data you do not own, do not degrade the tool for other
users, and give reasonable time to ship a fix before disclosing publicly. We will not pursue legal
action against research that follows these rules.

## Implementation limits

C.U.R.E is a local Windows inspection instrument. Scores and signatures are
evidence for review, not guaranteed malware detection or real-time protection.

Scanning does not remediate. Quarantine requires an explicit action and moves
supported persistence files rather than deleting them. Registry, service,
WMI, IFEO, AppInit and COM findings require manual investigation and backup.
Process termination, overlay closure and permanent disk cleanup are separate
operator actions. A Rescue Scan click does not authorize them.

Quarantine records preserve original paths, byte size, SHA-256 and available
security metadata. Undo is scoped to scanned roots, checks archived bytes and
refuses collisions. ACL/timestamp restoration is best effort; fidelity notes
belong to the engine's records. Legacy records can lack this metadata.

The GUI ships only local system fonts and assets. Signature revocation is
cache-only by default. The IOC provider contains labelled demonstration
fixtures. Incomplete/denied/unavailable collection cannot establish all-clear.
Several collectors do not expose explicit access coverage.

Paired rescue USBs may trigger the consented host watcher, which launches its
pinned host-installed GUI. Drive-supplied executables are untrusted. The GUI
does not have an API to verify watcher or USB connection state.

Scanned names, paths, command lines, task definitions and window titles are
attacker-controlled data. They must remain escaped and must never be executed
or interpolated into a shell by the frontend. Same-user code execution is
outside the enforcement boundary of a user-space scanner.

### Storage cleanup — what it does and does not promise

Storage cleanup is the only path in this product that permanently removes
files. It is guarded accordingly, and the guard has limits worth stating.

**What is enforced.** Every removal requires a current scan, a selection, a
preview listing each item with the reasoning that classified it, a typed
confirmation word, and a re-validation of each target immediately before
deletion: still present, still a regular file, still not a link, still under
an authorised scan root, still permitted by its tier, and unchanged in size
since the preview. Anything that fails a check is refused and listed on the
receipt. The webview never supplies a delete instruction — it supplies a
selection, and the engine decides.

**What is not enforced.**

- *Not reversible.* There is no quarantine copy and no undo. The preview says
  so in those words before the confirmation is enabled.
- *Reclaim estimates are not guarantees.* They use allocated size, but NTFS
  compression, deduplication, snapshots and concurrent writes all mean the
  volume's free-space delta may differ from the estimate. Both numbers are
  reported so the divergence is visible rather than hidden.
- *Last-access timestamps are not evidence.* Windows frequently disables
  last-access updates. C.U.R.E records them and never reasons from them;
  "unused" is always a statement about the directory's newest file, not
  about you.
- *Abandoned application data is never actionable.* It is reported at tier
  `unknown` with an explicit caveat, because an application that is still
  installed but idle, keeps state elsewhere, or holds a licence tied to a
  folder looks identical to one that is gone.
- *Scan-to-delete is not atomic.* There is a documented window between the
  re-check and the removal in which a hostile writer could swap a file for a
  junction. Closing it needs handle-relative deletion, which this
  implementation does not do.
- *The Recycle Bin payload is removed, its index record is not.* `$R` items
  are what holds the bytes; `$I` records are left alone because the shell
  owns that bookkeeping. Explorer may show stale entries until it reconciles.
- *Cloud-only files are never downloaded* while scanning, so a scan will not
  tell you their real size.

**Protected regardless of tier selection.** `C:\Windows` outside named
cleanable subtrees, `WinSxS`, `DriverStore`, `Installer`, `System32\config`,
`System Volume Information`, recovery partitions, restore locations,
`Program Files`, `ProgramData` outside named caches, and the Recycle Bin's
own index records are never proposed for removal. Registry, services, WMI,
IFEO, AppInit and COM are never modified at all, by any path.

**The installer does not weaken Windows security.** It installs for the
current user, needs no administrator rights, does not disable or exclude
antivirus, does not suppress Windows security warnings, and does not
install the USB watcher or any other persistent component. The watcher is
a separate, explicitly installed program.

**Removal is verified, not assumed.** The engine re-checks each target
against the live filesystem immediately before it is removed — its
identity on the volume, its size, its tier, its location — and removes it
through a verified handle rather than through a path that could have been
swapped underneath it. A file that changed, moved, became a link, left its
authorised root or changed tier is refused and listed on the receipt.

Release binaries are currently unsigned. Verify published SHA-256 checksums
against the corresponding release assets; checksums alone do not authenticate
the publisher. Windows may show a SmartScreen or unknown-publisher prompt.

For a vulnerability report, use the repository's private reporting facility
when available: [GitHub security advisories](https://github.com/MadB0i/C.U.R.E/security/advisories).
Do not include real credentials or unredacted host evidence in public issues.
