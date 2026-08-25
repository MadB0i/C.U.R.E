(function () {
  "use strict";

  if (window.__TAURI__) {
    console.warn("[mock-tauri] real window.__TAURI__ detected; mock disabled");
    return;
  }

  const listeners = Object.create(null);
  const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

  function emit(name, payload) {
    const set = listeners[name];
    if (!set) return;
    for (const cb of set) {
      try {
        cb({ event: name, id: 0, payload });
      } catch (err) {
        console.error("[mock-tauri] listener error", err);
      }
    }
  }

  // Tunable knobs for dev/testing (only defaulted, never clobbered, so test
  // harnesses can preset them via addInitScript before this file loads):
  //   __CURE_MOCK_ITEM_COUNT   number of fake entries scanned per run
  //   __CURE_MOCK_FOOTER_ERRORS  when true, footer commands reject (error paths)
  //   __CURE_MOCK_ALL_SAFE     when true, every fake entry scores Safe (ALL CLEAR)
  if (window.__CURE_MOCK_ITEM_COUNT === undefined) window.__CURE_MOCK_ITEM_COUNT = 8;
  if (window.__CURE_MOCK_FOOTER_ERRORS === undefined) window.__CURE_MOCK_FOOTER_ERRORS = false;
  if (window.__CURE_MOCK_ALL_SAFE === undefined) window.__CURE_MOCK_ALL_SAFE = false;
  window.__CURE_SCAN_DONE = false;

  const PASCAL_SOURCE = {
    "startup-folder": "StartupFolder",
    "scheduled-task": "ScheduledTask",
    "registry-run": "RegistryRun",
  };

  const NAME_PREFIXES = [
    "AcmeTray",
    "CloudSync",
    "UpdateTask",
    "TelemetrySvc",
    "BackupAgent",
    "SyncHelper",
    "LicenseCheck",
    "CacheWarm",
    "LogShipper",
    "IndexBoost",
    "HealthProbe",
    "PatchMgr",
  ];

  const STARTUP_DIR =
    "C:\\Users\\bob\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\\";

  function makeMockItems(n) {
    const items = [];
    for (let i = 0; i < n; i++) {
      const base =
        NAME_PREFIXES[i % NAME_PREFIXES.length] + (i % 3 === 0 ? String(i) : "");
      let item;
      switch (i % 6) {
        case 0:
          item = {
            source: "startup-folder",
            name: base + ".bat",
            command: "C:\\Users\\bob\\AppData\\Local\\Temp\\" + base.toLowerCase() + ".exe -q",
            location: STARTUP_DIR + base + ".bat",
            risk: "HighRisk",
            score: 55,
            reasons: [
              "+30 command path sits in a temp/downloads/public drop zone",
              "+25 entry name looks randomly generated",
            ],
          };
          break;
        case 1:
          item = {
            source: "scheduled-task",
            name: "Microsoft\\Windows\\Maintenance\\" + base,
            command: "C:\\Windows\\System32\\" + base.toLowerCase() + ".exe /quiet",
            location: "C:\\Windows\\System32\\Tasks\\Microsoft\\Windows\\Maintenance\\" + base,
            risk: "Safe",
            score: 0,
            reasons: [
              "-20 command path is a trusted install location (Program Files/System32)",
            ],
          };
          break;
        case 2:
          item = {
            source: "registry-run",
            name: base + "Autorun",
            command: "\"C:\\Program Files\\Vendor\\" + base + ".exe\" /bg",
            location: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
            risk: "Safe",
            score: 10,
            reasons: ["+10 executable runs directly from a user profile folder"],
          };
          break;
        case 3:
          item = {
            source: "scheduled-task",
            name: "EvilCorp\\" + base + "Persist",
            command: "C:\\Users\\Public\\Downloads\\" + base.toLowerCase() + "_cli.exe --silent",
            location: "C:\\Windows\\System32\\Tasks\\EvilCorp\\" + base + "Persist.xml",
            risk: "HighRisk",
            score: 70,
            reasons: [
              "+30 command path sits in a temp/downloads/public drop zone",
              "+40 Invalid Signature",
            ],
          };
          break;
        case 4:
          item = {
            source: "startup-folder",
            name: base + "Check.lnk",
            command: "powershell.exe -WindowStyle Hidden -File C:\\tools\\" + base.toLowerCase() + ".ps1",
            location: STARTUP_DIR + base + "Check.lnk",
            risk: "Suspicious",
            score: 30,
            reasons: ["+25 PowerShell invoked with encoded command or hidden window"],
          };
          break;
        default:
          item = {
            source: "registry-run",
            name: base + "Helper",
            command: "\"C:\\Users\\bob\\AppData\\Local\\Temp\\" + base.toLowerCase() + ".exe\" /bg",
            location: "HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
            risk: "HighRisk",
            score: 30,
            reasons: [
              "+30 command path sits in a temp/downloads/public drop zone",
              "Known Malware Hash",
            ],
          };
      }
      if (window.__CURE_MOCK_ALL_SAFE) {
        item.risk = "Safe";
        item.score = 0;
        item.reasons = [
          "-20 command path is a trusted install location (Program Files/System32)",
        ];
      }
      item.id = "mock-item-" + i;
      items.push(item);
    }
    return items;
  }

  function toScored(item) {
    return {
      entry: {
        id: item.id,
        source: PASCAL_SOURCE[item.source] || "StartupFolder",
        name: item.name,
        command: item.command,
        location: item.location,
      },
      score: item.score,
      risk: item.risk,
      reasons: item.reasons || [],
    };
  }

  let scanRuns = 0;

  // ---- disk cleanup mock --------------------------------------------------

  //   __CURE_MOCK_CLEANUP_FAILURES  when true, run_cleanup reports one locked file
  //   __CURE_MOCK_CLEANUP_DELAY_MS  how long run_cleanup pretends to take
  if (window.__CURE_MOCK_CLEANUP_FAILURES === undefined) {
    window.__CURE_MOCK_CLEANUP_FAILURES = false;
  }
  if (window.__CURE_MOCK_CLEANUP_DELAY_MS === undefined) {
    window.__CURE_MOCK_CLEANUP_DELAY_MS = 750;
  }
  if (window.__CURE_MOCK_OVERLAY_HITS === undefined) {
    window.__CURE_MOCK_OVERLAY_HITS = 1;
  }

  const CLEANUP_CATEGORIES = [
    { key: "temp", label: "Temp files", item_count: 1242, total_bytes: 6871947674 },
    { key: "browser_cache", label: "Browser caches", item_count: 2, total_bytes: 283115776 },
    { key: "recycle_bin", label: "Recycle Bin", item_count: 0, total_bytes: 0 },
    { key: "windows_old", label: "Windows.old", item_count: 1, total_bytes: 32851861504 },
  ];

  const CLEANUP_DOWNLOADS = [
    {
      path: "C:\\Users\\bob\\Downloads\\setup_toolkit.exe",
      name: "setup_toolkit.exe",
      size_bytes: 48923136,
      age_days: 63,
    },
    {
      path: "C:\\Users\\bob\\Downloads\\driver_pack_2024.msi",
      name: "driver_pack_2024.msi",
      size_bytes: 12582912,
      age_days: 41,
    },
    {
      path: "C:\\Users\\bob\\Downloads\\old-installer.exe",
      name: "old-installer.exe",
      size_bytes: 2097152,
      age_days: 122,
    },
  ];

  function snapshotCleanupMock() {
    return {
      categories: CLEANUP_CATEGORIES.map((c) => ({ ...c })),
      downloads: CLEANUP_DOWNLOADS.map((d) => ({ ...d })),
      total_bytes:
        CLEANUP_CATEGORIES.reduce((sum, c) => sum + c.total_bytes, 0) +
        CLEANUP_DOWNLOADS.reduce((sum, d) => sum + d.size_bytes, 0),
    };
  }

  function scanCleanupMock() {
    return delay(420).then(snapshotCleanupMock);
  }

  function runCleanupMock(args) {
    window.__CURE_LAST_CLEANUP_CALL = JSON.parse(JSON.stringify(args || {}));
    return delay(window.__CURE_MOCK_CLEANUP_DELAY_MS).then(() => {
      // real tauri maps camelCase JS keys to snake_case rust params; accept both
      const catSel = args.categories || [];
      const dlSel = args.download_paths || args.downloadPaths || [];
      if (window.__CURE_MOCK_CLEANUP_FAILURES) {
        const freed =
          catSel.reduce((sum, key) => {
            const cat = CLEANUP_CATEGORIES.find((c) => c.key === key);
            return sum + (cat ? Math.round(cat.total_bytes * 0.9) : 0);
          }, 0) +
          dlSel.reduce((sum, p) => {
            const dl = CLEANUP_DOWNLOADS.find((d) => d.path === p);
            return sum + (dl ? dl.size_bytes : 0);
          }, 0);
        return {
          attempted: catSel.length + dlSel.length,
          deleted: Math.max(0, catSel.length - 1) + dlSel.length,
          failed: 1,
          bytes_freed: freed,
          failures: [
            {
              path: "C:\\Windows\\Temp\\locked-by-running-process.tmp",
              reason:
                "The process cannot access the file because it is being used by another process. (os error 32)",
            },
          ],
        };
      }
      const freed =
        catSel.reduce((sum, key) => {
          const cat = CLEANUP_CATEGORIES.find((c) => c.key === key);
          return sum + (cat ? cat.total_bytes : 0);
        }, 0) +
        dlSel.reduce((sum, p) => {
          const dl = CLEANUP_DOWNLOADS.find((d) => d.path === p);
          return sum + (dl ? dl.size_bytes : 0);
        }, 0);
      return {
        attempted: catSel.length + dlSel.length,
        deleted: catSel.length + dlSel.length,
        failed: 0,
        bytes_freed: freed,
        failures: [],
      };
    });
  }

  async function runAutoScan() {
    scanRuns += 1;
    window.__CURE_SCAN_DONE = false;
    await delay(300);
    emit("scan-progress", { stage: "registry", message: "Reading Run / RunOnce autoruns" });
    await delay(550);
    emit("scan-progress", { stage: "startup", message: "Walking the per-user Startup folder" });
    await delay(500);
    emit("scan-progress", { stage: "tasks", message: "Parsing scheduled task definitions" });

    const n = Math.max(1, Number(window.__CURE_MOCK_ITEM_COUNT) || 8);
    const items = makeMockItems(n);
    emit("scan-progress", {
      stage: "scoring",
      message: "Risk-scoring " + n + " persistence entr" + (n === 1 ? "y" : "ies"),
    });
    await delay(600);

    // same pacing math as the Rust backend: 5000ms target split across items,
    // clamped to 15..250ms so tiny scans don't crawl and huge ones don't stall
    const perItem = Math.min(250, Math.max(15, Math.round(5000 / n)));

    const cleaned = [];
    const review = [];
    let safe = 0;
    for (const item of items) {
      emit("scan-progress", {
        stage: "item-scanned",
        name: item.name,
        source: item.source,
        location: item.location,
        risk: item.risk,
        score: item.score,
      });
      await delay(perItem);
      if (item.risk === "HighRisk") {
        if (item.source === "registry-run") {
          review.push(item); // registry values are never auto-cleaned
        } else {
          emit("scan-progress", {
            stage: "cleaning",
            message: "Auto-cleaning " + item.name + " (score " + item.score + ")",
          });
          await delay(40); // backend quarantines synchronously; keep a small beat for the ping
          cleaned.push(item);
        }
      } else if (item.risk === "Suspicious") {
        review.push(item);
      } else {
        safe += 1;
      }
    }

    await delay(500);
    emit("scan-progress", { stage: "done", message: "Scan complete" });
    window.__CURE_SCAN_DONE = true;
    console.info("[mock-tauri] run #" + scanRuns + ": " + n + " items @ " + perItem + "ms/item");

    var result = {
      total: n,
      high_risk_cleaned: cleaned.map(toScored),
      suspicious_for_review: review.map(toScored),
      safe,
    };

    if (window.__CURE_MOCK_SWEEP) {
      var sweepProcs = [
        { name: "suspicious_loader.exe", pid: 4242, exe_path: "C:\\Users\\test\\AppData\\Local\\Temp\\suspicious_loader.exe", score: 62, risk: "HighRisk", reasons: ["Unsigned binary", "Random name"] },
        { name: "unknown_tool.exe", pid: 8080, exe_path: "C:\\Users\\test\\Downloads\\unknown_tool.exe", score: 41, risk: "HighRisk", reasons: ["Profile exe", "Unsigned Binary"] },
        { name: "helper.dll", pid: 11234, exe_path: "C:\\ProgramData\\helper.dll", score: 22, risk: "Suspicious", reasons: ["Unsigned Binary"] },
      ];
      var sweepRansom = [
        { finding_type: "ransom-note", path: "C:\\Users\\test\\Documents\\DECRYPT_MY_FILES.txt", detail: "Matched pattern: DECRYPT — \"All your files have been encrypted by LockBit 3.0.\"", suspected_family: "LockBit", nomoreransom_url: "https://www.nomoreransom.org/" },
        { finding_type: "bulk-encryption", path: "C:\\Users\\test\\Pictures", detail: "17 files with unusual extension \".locked\" (avg age 3 days)", suspected_family: null, nomoreransom_url: null },
      ];
      sweepProcs.forEach(function(p) {
        emit("scan-progress", { stage: "process-flagged", name: p.name, pid: p.pid, risk: p.risk, score: p.score });
      });
      sweepRansom.forEach(function(r) {
        emit("scan-progress", { stage: "ransom-found", detail: r.detail });
      });
      result.process_findings = sweepProcs;
      result.ransom_findings = sweepRansom;
    }

    return result;
  }

  window.__TAURI__ = {
    core: {
      invoke(command, args) {
        switch (command) {
          case "dismiss_overlays": {
            const hits = window.__CURE_MOCK_OVERLAY_HITS || 0;
            const closed = [];
            for (let i = 0; i < hits; i++) {
              closed.push({
                title: i === 0 ? "SIMULATED RANSOM SCREEN" : "OVERLAY WINDOW " + (i + 1),
                process: i === 0 ? "fake-overlay.exe" : "overlay-" + (i + 1) + ".exe",
                signature: "unsigned",
                terminated: false,
              });
            }
            return delay(80).then(() => ({ checked: 12, closed: closed }));
          }
          case "run_auto_scan":
            return runAutoScan();
          case "quarantine_entry":
            return delay(450).then(
              () =>
                "moved C:\\" +
                String(args && args.name ? args.name : "entry") +
                " -> quarantine (mock)"
            );
          case "undo_entry":
            return delay(250);
          case "open_quarantine_folder":
            return delay(350).then(() => {
              if (window.__CURE_MOCK_FOOTER_ERRORS) {
                throw new Error("No quarantine folder yet — nothing has been auto-cleaned");
              }
              return "C:\\cure-mock\\data\\quarantine";
            });
          case "view_log":
            return delay(350).then(() => {
              if (window.__CURE_MOCK_FOOTER_ERRORS) {
                throw new Error("No scan log yet — run a scan first");
              }
              return "C:\\cure-mock\\data\\baseline.json";
            });
          case "exit_app":
            return delay(120).then(() => {
              if (!window.__CURE_MOCK_FOOTER_ERRORS) {
                console.info("[mock-tauri] exit_app invoked (dev harness stays open)");
              }
            });
          case "scan_cleanup":
            return scanCleanupMock();
          case "run_cleanup":
            return runCleanupMock(args);
          case "kill_high_risk_processes":
            return delay(350).then(function() {
              var procs = (args && args.processes) || [];
              var killed = [];
              var failed = [];
              procs.forEach(function(pair) {
                if (pair[0] === "fail-on-kill.exe") {
                  failed.push(pair[0] + " (access denied)");
                } else {
                  killed.push({ name: pair[0], pid: pair[1], exe_path: "", score: 0, risk: "HighRisk", reasons: [] });
                }
              });
              return { killed: killed, failed: failed };
            });
          default:
            return Promise.reject(new Error("mock-tauri: unknown command " + command));
        }
      },
    },
    event: {
      listen(name, callback) {
        (listeners[name] || (listeners[name] = new Set())).add(callback);
        return Promise.resolve(() => listeners[name].delete(callback));
      },
      emit(name, payload) {
        emit(name, payload);
        return Promise.resolve();
      },
    },
  };

  console.info("[mock-tauri] active — " + window.__CURE_MOCK_ITEM_COUNT + " mock items/run");
})();
