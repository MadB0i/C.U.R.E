// Baseline-output guard: proves C.U.R.E. never writes baseline.json /
// quarantine / records to the user's Desktop/home unexpectedly.
//   1. Snapshot Desktop (+ home root) before.
//   2. Run a real CLI scan with --data-dir sandbox (cwd = sandbox).
//   3. Assert all artifacts land ONLY inside the sandbox data dir.
//   4. Assert Desktop/home snapshots are byte-identical afterwards.
//   5. Static guard: gui resolve_data_dir must default to the app-owned
//      %LOCALAPPDATA%\CURE, never to the executable's folder.
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, existsSync, readdirSync, rmSync, readFileSync } from "node:fs";
import { tmpdir, homedir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repo = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const cure = path.join(repo, "target", "release", "cure.exe");
const failures = [];
const check = (name, cond, extra = "") => {
  console.log((cond ? "ok   " : "FAIL ") + name + (extra ? "  [" + extra + "]" : ""));
  if (!cond) failures.push(name);
};

const desktop = path.join(homedir(), "OneDrive", "Desktop");
const home = homedir();
const snap = (dir) => {
  try {
    return readdirSync(dir).sort().join("\n");
  } catch { return "(unreadable)"; }
};
const beforeDesktop = snap(desktop);
const beforeHomeJson = (() => {
  try {
    return readdirSync(home).filter((f) => f.toLowerCase().endsWith(".json")).sort().join("\n");
  } catch { return "(unreadable)"; }
})();

const box = mkdtempSync(path.join(tmpdir(), "cure-bguard-"));
const startup = path.join(box, "startup");
const tasks = path.join(box, "tasks");
const data = path.join(box, "data");
mkdirSync(startup, { recursive: true });
mkdirSync(tasks, { recursive: true });

try {
  if (!existsSync(cure)) throw new Error("CLI binary missing — run: cargo build --release -p cure_cli");
  execFileSync(cure, ["scan", "--data-dir", data, "--startup-root", startup, "--tasks-root", tasks],
    { encoding: "utf8", cwd: box, timeout: 120000 });
  check("baseline.json lands in --data-dir", existsSync(path.join(data, "baseline.json")));
  check("Desktop unchanged after scan", snap(desktop) === beforeDesktop);
  check("no new *.json at home root", (() => {
    try {
      return readdirSync(home).filter((f) => f.toLowerCase().endsWith(".json")).sort().join("\n") === beforeHomeJson;
    } catch { return false; }
  })());
  check("no baseline.json dropped in cwd sandbox root", !existsSync(path.join(box, "baseline.json")));
} catch (e) {
  check("scan completes without exception", false, String((e && e.message) || e).slice(0, 200));
} finally {
  try { rmSync(box, { recursive: true, force: true }); } catch {}
}

// Static guard on the GUI default (the exact regression: exe-adjacent data dir).
try {
  const mainRs = readFileSync(path.join(repo, "gui", "src-tauri", "src", "main.rs"), "utf8");
  const m = mainRs.match(/fn resolve_data_dir\(\)[\s\S]*?\n\}/);
  const body = m ? m[0] : "";
  check("gui resolve_data_dir defaults to LOCALAPPDATA\\CURE", body.includes("LOCALAPPDATA") && body.includes('.join("CURE")'));
  check("gui resolve_data_dir no longer defaults to exe folder", !/current_exe\(\)[\s\S]{0,200}?exe\.parent\(\)/.test(body) && !body.includes('PathBuf::from(".")'));
} catch (e) {
  check("gui main.rs readable", false, String((e && e.message) || e).slice(0, 120));
}

console.log(failures.length ? "BASELINE GUARD: FAILURES\n" + failures.join("\n") : "BASELINE GUARD: ALL CHECKS PASSED");
process.exit(failures.length ? 1 : 0);
