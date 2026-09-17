// Quarantine regression: drives the REAL cure CLI in an isolated sandbox.
// Proves the full production path with real filesystem operations:
//   CREATE TEST FILE -> SCAN (must NOT auto-move) -> QUARANTINE (explicit)
//   -> VERIFY MOVED (bytes identical) -> VERIFY LISTED -> UNDO
//   -> VERIFY ORIGINAL RESTORED (bytes identical).
// Fails loudly on any deviation. Cleans up its sandbox afterwards.
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync, statSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repo = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const cure = path.join(repo, "target", "release", "cure.exe");
if (!existsSync(cure)) {
  console.error("FAIL: CLI binary missing — run: cargo build --release -p cure_cli");
  process.exit(1);
}

const failures = [];
const check = (name, cond, extra = "") => {
  console.log((cond ? "ok   " : "FAIL ") + name + (extra ? "  [" + extra + "]" : ""));
  if (!cond) failures.push(name);
};

const box = mkdtempSync(path.join(tmpdir(), "cure-qreg-"));
const startup = path.join(box, "startup");
const tasks = path.join(box, "tasks");
const data = path.join(box, "data");
mkdirSync(startup, { recursive: true });
mkdirSync(tasks, { recursive: true });

const seedName = "cure-v4-quarantine-test.bat";
const seedPath = path.join(startup, seedName);
// Suspicious-looking but inert payload with fixed bytes for identity checks.
const payload = Buffer.from("@echo off\r\nREM CURE V4 quarantine regression payload\r\n" + "X".repeat(512) + "\r\n", "utf8");
writeFileSync(seedPath, payload);

const run = (args) => execFileSync(cure, args, { encoding: "utf8", cwd: box, timeout: 120000 });

try {
  // 1. SCAN — detection only; the file must still be in place afterwards.
  const scanOut = run(["scan", "--data-dir", data, "--startup-root", startup, "--tasks-root", tasks]);
  const idMatch = scanOut.split("\n").map((l) => l.match(/id=([0-9a-f]{16})/)).find(Boolean);
  // find the id on the line mentioning our seed file
  let id = null;
  for (const line of scanOut.split("\n")) {
    if (line.includes(seedName)) {
      const m = line.match(/id=([0-9a-f]{16})/);
      if (m) { id = m[1]; break; }
    }
  }
  check("scan detects seeded file", id !== null, id || "no id in scan output");
  if (!id) throw new Error("seeded file not detected — cannot continue");
  check("scan does NOT auto-move (no automatic remediation)", existsSync(seedPath));
  check("baseline.json written inside --data-dir", existsSync(path.join(data, "baseline.json")));
  check("no quarantine/ created by scan alone", !existsSync(path.join(data, "quarantine")));

  // 2. Explicit QUARANTINE.
  const qOut = run(["quarantine", id, "--data-dir", data, "--startup-root", startup, "--tasks-root", tasks]);
  check("quarantine command succeeds", /moved:/.test(qOut), qOut.split("\n")[0]);
  check("source file moved away", !existsSync(seedPath));
  const qdir = path.join(data, "quarantine");
  const moved = existsSync(qdir) ? readdirSync(qdir).filter((f) => f.startsWith(id + "_")) : [];
  check("quarantined file present under data dir", moved.length === 1, moved.join(","));
  if (moved.length === 1) {
    const movedBytes = readFileSync(path.join(qdir, moved[0]));
    check("quarantined bytes identical", movedBytes.equals(payload));
  }
  const records = JSON.parse(readFileSync(path.join(qdir, "records.json"), "utf8"));
  check("record listed with original path", records[id] && records[id].original_path === seedPath, records[id] ? records[id].original_path : "no record");
  check("record quarantine_path exists", !!(records[id] && existsSync(records[id].quarantine_path)));

  // 3. Second quarantine of the same id is an idempotent no-op with notice
  // (CLI reports "already in quarantine", exit 0) — must not duplicate.
  let doubleOut = "";
  try {
    doubleOut = run(["quarantine", id, "--data-dir", data, "--startup-root", startup, "--tasks-root", tasks]);
  } catch (e) {
    doubleOut = "";
  }
  check("double quarantine is idempotent with notice", /already in quarantine/.test(doubleOut), doubleOut.split("\n")[0]);
  const movedTwice = existsSync(qdir) ? readdirSync(qdir).filter((f) => f.startsWith(id + "_")) : [];
  check("no duplicate quarantined file", movedTwice.length === 1);

  // 4. UNDO — scoped restore under the scanned roots.
  const uOut = run(["undo", id, "--data-dir", data, "--startup-root", startup, "--tasks-root", tasks]);
  check("undo command succeeds", /restored:/.test(uOut), uOut.split("\n")[0]);
  check("original restored", existsSync(seedPath));
  if (existsSync(seedPath)) {
    check("restored bytes identical", readFileSync(seedPath).equals(payload));
  }
  const recordsAfter = JSON.parse(readFileSync(path.join(qdir, "records.json"), "utf8"));
  check("record cleared after undo", !(id in recordsAfter));
} catch (e) {
  check("no unexpected exception", false, String(e && e.message || e).slice(0, 200));
} finally {
  try { rmSync(box, { recursive: true, force: true }); } catch {}
}

console.log(failures.length ? "QUARANTINE REGRESSION: FAILURES\n" + failures.join("\n") : "QUARANTINE REGRESSION: ALL CHECKS PASSED");
process.exit(failures.length ? 1 : 0);
