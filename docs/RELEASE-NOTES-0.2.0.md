# C.U.R.E 0.2.0 — Rescue Console

Evidence-first Windows persistence inspection and reversible quarantine.

- **System X-Ray**: a sectioned view of the machine being inspected — nine
  persistence-layer plates, a bounded chassis board with the system core, and an
  inspection beam that travels to whichever collector the backend reports as
  running. Layer state follows actual collection events and reported coverage.
- **Luma**: a compact offline status node, not a character. A graphite body with
  a violet ring that reports state — idle breathes, scanning sweeps an arc,
  cleanup collapses segments, review pulses amber, and success or seal draws a
  green ring once and rests. Flat and filter-free, sized to stay secondary to the
  measurements beside it. Motion can be paused with one gesture that also stops
  the scan field and storage core, and respects reduced-motion preferences.
- **Disk Cleanup** is a primary destination with a segmented storage core: one
  arc and one proportional bar per real engine category, sized from measured
  byte totals, with the reclaimed arc drawn from the engine's own
  `bytes_freed`.
- Six destinations: Overview, Rescue, Investigate, Quarantine, Disk Cleanup and
  Monitor — icon and title only.
- Dedicated finding inspection, full selectable technical values, signature,
  publisher and on-demand target hash/metadata; source-specific guidance.
- Risk rails and real score meters on every finding row; evidence kept to a
  measured column so verdict, evidence and action read together.
- Quarantine records show integrity and ACL/fidelity metadata with scoped undo.
- Graphite depth design system, restrained violet identity accent, offline
  system typography, reduced motion, compact-window support, keyboard focus and
  deterministic accessibility/layout/state/pixel verification.

Scanning performs no automatic remediation. Supported files move only after
confirmation. Registry, services, WMI, IFEO, AppInit and COM are guidance-only.
Disk cleanup permanently deletes confirmed selections and is a separate action.

C.U.R.E is not antivirus, not EDR and not automatic machine recovery. Plugging
in a rescue USB does not make an infected machine safe: nothing is scanned or
remediated until the operator starts a run and confirms each action.

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
