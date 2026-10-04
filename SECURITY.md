# Security model

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

Release binaries are currently unsigned. Verify published SHA-256 checksums
against the corresponding release assets; checksums alone do not authenticate
the publisher. Windows may show a SmartScreen or unknown-publisher prompt.

For a vulnerability report, use the repository's private reporting facility
when available: [GitHub security advisories](https://github.com/MadB0i/C.U.R.E/security/advisories).
Do not include real credentials or unredacted host evidence in public issues.
