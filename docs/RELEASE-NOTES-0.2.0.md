# C.U.R.E 0.2.0 — Evidence Console

Evidence-first Windows persistence inspection and reversible quarantine.

- Five destinations: Overview, Rescue, Investigate, Quarantine and Monitor.
- Evidence Sweep follows actual collection events and reported coverage.
- Dedicated finding inspection, full selectable technical values, signature,
  publisher and on-demand target hash/metadata; source-specific guidance.
- Quarantine records show integrity and ACL/fidelity metadata with scoped undo.
- Offline system typography, reduced motion, compact-window support, keyboard
  focus and deterministic accessibility/layout/state verification.

Scanning performs no automatic remediation. Supported files move only after
confirmation. Registry, services, WMI, IFEO, AppInit and COM are guidance-only.
Disk cleanup permanently deletes confirmed selections and is a separate action.

## Assets and integrity

Portable ZIP: cure-gui.exe, cure.exe, cure-watch.exe, README.md, LICENSE,
SECURITY.md, RELEASE-NOTES.md, BUILD-INFO.txt, README screenshots/demo and binary SHA256SUMS.txt. The external
SHA256SUMS.txt also covers the ZIP. Verify downloaded files with
`Get-FileHash -Algorithm SHA256` and compare the full hash.

**Unsigned binaries:** this release has no Authenticode publisher signature.
Windows may show SmartScreen or an unknown-publisher prompt. SHA-256 confirms
byte integrity against the published list; it does not authenticate a publisher.

Windows 10/11 and WebView2 are required. Elevation affects collection access.
Incomplete coverage is never represented as clean. Some collectors do not
report access coverage; the GUI cannot verify watcher/USB connection state.
Canary is an experimental tripwire. Risk scoring is heuristic and the IOC
provider remains a labelled demo fixture. ACL/timestamp restore is best effort.

README screenshots and demo use labelled deterministic mock data. They show
the workflow, not a validation claim about malware detection.
