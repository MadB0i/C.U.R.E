//! QA process-safety guard (Stage UI-02B).
//!
//! A previous test-harness cleanup executed `taskkill /IM codex.exe` and
//! terminated two owner-controlled Codex CLI processes. This test exists so
//! that can never happen again: it statically scans every test/dev tooling
//! file in the repository for name-based process killing and fails with the
//! exact file, line and pattern when one appears.
//!
//! Allowed (ownership-scoped) patterns are documented in
//! `testing/native/process_ownership.py`; this guard is the backstop that
//! keeps them that way.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Directories whose files are process-control tooling. Product source
/// (core/src, gui/src-tauri, cli, watch, winwatch, dirwatch) is NOT scanned:
/// the product legitimately uses TerminateProcess behind explicit
/// operator-confirmed, identity-verified commands.
const TOOLING_DIRS: [&str; 3] = ["testing", "gui/devtools", "tools"];

/// File extensions scanned by the guard.
const SCANNED_EXTENSIONS: [&str; 5] = ["py", "ps1", "mjs", "js", "rs"];

/// Case-insensitive structural patterns that mark a line as a name-based
/// kill. The matching lives in `line_matches_forbidden`; this documentation
/// block lists them:
///   * `taskkill` with the `/IM` flag — kills by image name.
///   * `Stop-Process` without `-Id` — PowerShell kills by name or pipeline.
///   * `Get-Process` and `Stop-Process` on the same line — piped name kill.
///   * `process_iter` (psutil) plus a protected agent/system image name on
///     the same line — the exact pattern that killed the owner's Codex.
///   * `TerminateProcess` called directly from tooling — that API belongs
///     to the audited product commands, never to test scripts.
///
/// Image names whose termination is reserved for the owner. A tooling line
/// that both kills something and references one of these names fails even
/// when the generic patterns above do not match.
const PROTECTED_IMAGES: [&str; 12] = [
    "codex",
    "opencode",
    "antigravity",
    "gemini",
    "aider",
    "claude",
    "cursor",
    "windsurf",
    "ollama",
    "zcode",
    "msedgewebview2",
    "cure-gui",
];

fn repo_root() -> PathBuf {
    // This test lives in core/tests/, so the workspace root is two up.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core crate must have a parent")
        .to_path_buf()
}

fn collect_tooling_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in TOOLING_DIRS {
        let dir = root.join(dir);
        if !dir.is_dir() {
            continue;
        }
        let walker = walk(dir.clone());
        for path in walker {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase());
            if ext.map(|e| SCANNED_EXTENSIONS.contains(&e.as_str())) == Some(true) {
                files.push(path);
            }
        }
    }
    files
}

/// Small recursive walker (std-only; the `walkdir` dev-dependency is not
/// worth adding for three directories). Skips dependency and evidence
/// output directories.
fn walk(dir: PathBuf) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if path.is_dir() {
            if name == "node_modules"
                || name == "__pycache__"
                || name == "cure-evidence"
                || name.starts_with('.')
            {
                continue;
            }
            out.extend(walk(path));
        } else {
            out.push(path);
        }
    }
    out
}

fn line_matches_forbidden(line_lower: &str) -> Option<&'static str> {
    let has = |needle: &str| line_lower.contains(needle);

    if has("taskkill") && (has("/im") || has("/ im")) {
        return Some("taskkill /IM (image-name kill)");
    }
    if has("stop-process") && !has("-id") {
        return Some("Stop-Process without -Id (name or pipeline kill)");
    }
    if has("get-process") && has("stop-process") {
        return Some("Get-Process piped into Stop-Process (name kill)");
    }
    if has("process_iter") && PROTECTED_IMAGES.iter().any(|n| has(n)) {
        return Some("psutil process_iter filtering a protected image name");
    }
    if has("terminateprocess") && !line_lower.contains("core/") {
        return Some("direct TerminateProcess in tooling");
    }
    None
}

fn line_targets_protected_image_with_kill(line_lower: &str) -> bool {
    let kill_words = ["taskkill", "stop-process", ".kill(", ".kill()"];
    if !kill_words.iter().any(|k| line_lower.contains(k)) {
        return false;
    }
    PROTECTED_IMAGES.iter().any(|n| line_lower.contains(n))
}

/// `taskkill` is allowed ONLY when it targets an exact verified PID
/// (`/PID <n>` with no `/IM`). This is the one primitive the ownership
/// helper may use after re-proving identity.
fn line_is_pid_scoped_taskkill(line_lower: &str) -> bool {
    line_lower.contains("taskkill") && line_lower.contains("/pid")
}

#[test]
fn no_name_based_process_killing_in_tooling() {
    let root = repo_root();
    assert!(
        root.join("testing").is_dir() && root.join("gui").is_dir(),
        "guard ran against an unexpected layout (cwd?): {}",
        root.display()
    );

    let mut violations: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in collect_tooling_files(&root) {
        let Ok(content) = std::fs::read_to_string(&file) else {
            continue;
        };
        let rel = file
            .strip_prefix(&root)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        for (idx, line) in content.lines().enumerate() {
            let lower = line.to_ascii_lowercase();
            // A `taskkill /F /T /PID <n>` line is the one allowed kill
            // primitive (exact verified PID); it can never match the /IM
            // pattern, so confirm it is genuinely PID-scoped and move on.
            // Everything else falls through to the checks below.
            if line_is_pid_scoped_taskkill(&lower) {
                continue;
            }
            let mut reasons: Vec<&str> = Vec::new();
            if let Some(reason) = line_matches_forbidden(&lower) {
                reasons.push(reason);
            }
            if line_targets_protected_image_with_kill(&lower) {
                reasons.push("kill line references a protected image name");
            }
            if !reasons.is_empty() {
                violations.entry(rel.clone()).or_default().push(format!(
                    "line {}: {}",
                    idx + 1,
                    reasons.join("; ")
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "QA SAFETY GATE: name-based process killing found in test/dev tooling. \
         Cleanup must only ever target processes the harness itself launched, \
         identified by exact PID + creation time (see \
         testing/native/process_ownership.py). Owner-controlled processes \
         (Codex, OpenCode, Antigravity, IDEs, other WebView2 apps) must never \
         be reachable from automation. Violations:\n{}",
        violations
            .into_iter()
            .map(|(file, lines)| format!("  {file}\n    {}", lines.join("\n    ")))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn native_harnesses_use_the_ownership_helper() {
    let root = repo_root();
    // Every native harness that launches cure-gui must go through the
    // shared ownership helper, so pre-launch cleanup is always
    // identity-verified and the port is always fail-fast.
    let harnesses = [
        (
            "testing/native/native-e2e.py",
            vec!["process_ownership", "pre_launch_cleanup"],
        ),
        (
            "testing/native/native-agent-audit.py",
            vec!["process_ownership", "pre_launch_cleanup"],
        ),
        (
            "gui/devtools/ui01a-native-verify.py",
            vec!["process_ownership", "pre_launch_cleanup"],
        ),
        (
            "gui/devtools/ui02-native-verify.py",
            vec!["process_ownership", "pre_launch_cleanup"],
        ),
        (
            "gui/devtools/ui02a-native-verify.py",
            vec!["process_ownership", "pre_launch_cleanup"],
        ),
    ];
    for (rel, markers) in harnesses {
        let path = root.join(rel);
        let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!("required harness missing: {rel} ({e})");
        });
        for marker in markers {
            assert!(
                content.contains(marker),
                "{rel} must use the shared ownership helper ({marker}); \
                 inline process cleanup is how the Codex kill happened"
            );
        }
    }
}

#[test]
fn ownership_helper_never_kills_by_name() {
    // The helper itself is the one place with termination primitives; prove
    // it only ever invokes taskkill in the /PID form.
    let root = repo_root();
    let helper = root.join("testing/native/process_ownership.py");
    let content =
        std::fs::read_to_string(&helper).expect("testing/native/process_ownership.py must exist");
    for line in content.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("taskkill") {
            assert!(
                lower.contains("/pid") && !lower.contains("/im"),
                "process_ownership.py must only use `taskkill /F /T /PID`, found: {line}"
            );
        }
        assert!(
            !lower.contains("stop-process"),
            "process_ownership.py must not use Stop-Process (found: {line})"
        );
    }
}
