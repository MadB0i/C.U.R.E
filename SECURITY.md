# Security Policy

## Reporting a vulnerability

If you find something exploitable in C.U.R.E, email the maintainer directly rather than opening a
public issue. Include the affected version, reproduction steps, and what an attacker gains.

You will get an acknowledgement within a few days. Fixes land in a patch release; there is no
paid support tier and no SLA, so set expectations accordingly.

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
