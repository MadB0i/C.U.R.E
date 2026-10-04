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
  //   __CURE_MOCK_ALL_SAFE     when true, every fake entry scores Safe (no findings)
  if (window.__CURE_MOCK_ITEM_COUNT === undefined)
    window.__CURE_MOCK_ITEM_COUNT = 8;
  if (window.__CURE_MOCK_FOOTER_ERRORS === undefined)
    window.__CURE_MOCK_FOOTER_ERRORS = false;
  if (window.__CURE_MOCK_ALL_SAFE === undefined)
    window.__CURE_MOCK_ALL_SAFE = false;
  if (window.__CURE_MOCK_CANARY_ALERT === undefined)
    window.__CURE_MOCK_CANARY_ALERT = false;
  window.__CURE_SCAN_DONE = false;

  const PASCAL_SOURCE = {
    "startup-folder": "StartupFolder",
    "scheduled-task": "ScheduledTask",
    "registry-run": "RegistryRun",
    "windows-service": "WindowsService",
    "wmi-subscription": "WmiSubscription",
    "ifeo-debugger": "IfeoDebugger",
    "appinit-dlls": "AppInitDlls",
    "com-hijack": "ComHijack",
  };

  const MOCK_ATTACK = {
    StartupFolder: { id: "T1547.001", name: "Startup Folder" },
    ScheduledTask: { id: "T1053.005", name: "Scheduled Task" },
    RegistryRun: { id: "T1547.001", name: "Registry Run Keys" },
    WindowsService: { id: "T1543.003", name: "Windows Service" },
    WmiSubscription: { id: "T1546.003", name: "WMI Event Subscription" },
    IfeoDebugger: {
      id: "T1546.012",
      name: "Image File Execution Options Injection",
    },
    AppInitDlls: { id: "T1546.010", name: "AppInit DLLs" },
    ComHijack: { id: "T1546.015", name: "Component Object Model Hijacking" },
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
    if (window.__CURE_MOCK_ALL_CLEAR) {
      for (let i = 0; i < n; i++) {
        items.push({
          source: "scheduled-task",
          name: "Microsoft\\Windows\\Maintenance\\TelemetrySvc" + (i || ""),
          command: "C:\\Windows\\System32\\telemetrysvc" + i + ".exe /quiet",
          location:
            "C:\\Windows\\System32\\Tasks\\Microsoft\\Windows\\Maintenance\\TelemetrySvc" +
            i,
          risk: "Safe",
          score: 0,
          reasons: [
            "-20 command path is a trusted install location (Program Files/System32)",
          ],
        });
      }
      return items;
    }
    for (let i = 0; i < n; i++) {
      const base =
        NAME_PREFIXES[i % NAME_PREFIXES.length] +
        (i % 3 === 0 ? String(i) : "");
      let item;
      switch (i % 6) {
        case 0:
          item = {
            source: "startup-folder",
            name: base + ".bat",
            command:
              "C:\\Users\\bob\\AppData\\Local\\Temp\\" +
              base.toLowerCase() +
              ".exe -q",
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
            command:
              "C:\\Windows\\System32\\" + base.toLowerCase() + ".exe /quiet",
            location:
              "C:\\Windows\\System32\\Tasks\\Microsoft\\Windows\\Maintenance\\" +
              base,
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
            command: '"C:\\Program Files\\Vendor\\' + base + '.exe" /bg',
            location: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
            risk: "Safe",
            score: 10,
            reasons: [
              "+10 executable runs directly from a user profile folder",
            ],
          };
          break;
        case 3:
          item = {
            source: "scheduled-task",
            name: "EvilCorp\\" + base + "Persist",
            command:
              "C:\\Users\\Public\\Downloads\\" +
              base.toLowerCase() +
              "_cli.exe --silent",
            location:
              "C:\\Windows\\System32\\Tasks\\EvilCorp\\" + base + "Persist.xml",
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
            command:
              "powershell.exe -WindowStyle Hidden -File C:\\tools\\" +
              base.toLowerCase() +
              ".ps1",
            location: STARTUP_DIR + base + "Check.lnk",
            risk: "Suspicious",
            score: 30,
            reasons: [
              "+25 PowerShell invoked with encoded command or hidden window",
            ],
          };
          break;
        default:
          item = {
            source: "registry-run",
            name: base + "Helper",
            command:
              '"C:\\Users\\bob\\AppData\\Local\\Temp\\' +
              base.toLowerCase() +
              '.exe" /bg',
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
    const source = PASCAL_SOURCE[item.source] || "StartupFolder";
    return {
      entry: {
        id: item.id,
        source,
        name: item.name,
        command: item.command,
        location: item.location,
      },
      score: item.score,
      risk: item.risk,
      reasons: item.reasons || [],
      attack: MOCK_ATTACK[source] || { id: "", name: "" },
    };
  }

  let scanRuns = 0;

  // ---- quarantine mock (small in-memory store so the Quarantine view,
  //      quarantine buttons, and undo can be exercised in the harness) ----
  const __mockQuarantine = window.__CURE_MOCK_EMPTY_QUARANTINE
    ? []
    : [
        {
          id: "mock-q-seed-1",
          name: "acmetray0.bat",
          source: "startup-folder",
          original_path: STARTUP_DIR + "acmetray0.bat",
          quarantine_path:
            "C:\\cure-mock\\data\\quarantine\\mock-q-seed-1_acmetray0.bat",
          archived_at: "2026-10-04T03:00:00Z",
        },
      ];

  function snapshotQuarantine() {
    return __mockQuarantine.map((r) => ({ ...r }));
  }

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
    {
      key: "temp",
      label: "Temp files",
      item_count: 1242,
      total_bytes: 6871947674,
    },
    {
      key: "browser_cache",
      label: "Browser caches",
      item_count: 2,
      total_bytes: 283115776,
    },
    { key: "recycle_bin", label: "Recycle Bin", item_count: 0, total_bytes: 0 },
    {
      key: "windows_old",
      label: "Windows.old",
      item_count: 1,
      total_bytes: 32851861504,
    },
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
      const categories = args.categories || [];
      const downloads = args.download_paths || args.downloadPaths || [];
      const result = {
        attempted: 0,
        deleted: 0,
        failed: 0,
        bytes_freed: 0,
        failures: [],
      };
      // Model actual file counts and the subsequent rescan, not category counts.
      for (const cat of CLEANUP_CATEGORIES) {
        if (!categories.includes(cat.key) || !cat.item_count) continue;
        result.attempted += cat.item_count;
        const locked =
          window.__CURE_MOCK_CLEANUP_FAILURES && result.failed === 0;
        const remainingBytes = locked
          ? Math.ceil(cat.total_bytes / cat.item_count)
          : 0;
        result.deleted += cat.item_count - (locked ? 1 : 0);
        result.bytes_freed += cat.total_bytes - remainingBytes;
        cat.item_count = locked ? 1 : 0;
        cat.total_bytes = remainingBytes;
        if (locked) {
          result.failed = 1;
          result.failures.push({
            path: "C:\\Windows\\Temp\\locked-by-running-process.tmp",
            reason:
              "The process cannot access the file because it is being used by another process. (os error 32)",
          });
        }
      }
      for (let i = CLEANUP_DOWNLOADS.length - 1; i >= 0; i--) {
        if (!downloads.includes(CLEANUP_DOWNLOADS[i].path)) continue;
        result.attempted++;
        result.deleted++;
        result.bytes_freed += CLEANUP_DOWNLOADS[i].size_bytes;
        CLEANUP_DOWNLOADS.splice(i, 1);
      }
      window.__CURE_LAST_CLEANUP_RESULT = { ...result };
      return result;
    });
  }

  async function runAutoScan() {
    scanRuns += 1;
    if (window.__CURE_MOCK_SCAN_ERROR)
      throw new Error(
        "Collection failed. Retry the scan; no remediation was performed.",
      );
    window.__CURE_SCAN_DONE = false;
    await delay(300);
    emit("scan-progress", {
      stage: "registry",
      message: "Reading Run / RunOnce autoruns",
    });
    emit("scan-progress", {
      stage: "source-state",
      area: "Registry autoruns",
      state: "Checked",
      detail: "3 values",
    });
    await delay(550);
    emit("scan-progress", {
      stage: "startup",
      message: "Walking the per-user Startup folder",
    });
    await delay(500);
    emit("scan-progress", {
      stage: "tasks",
      message: "Parsing scheduled task definitions",
    });

    for (const [stage, message] of [
      ["startup-common", "Walking common Startup"],
      ["services", "Enumerating services"],
      ["wmi", "Querying WMI subscriptions"],
      ["ifeo", "Checking IFEO / AppInit"],
      ["com", "Inspecting COM registrations"],
    ]) {
      emit("scan-progress", { stage, message });
      await delay(120);
    }
    for (const row of window.__CURE_MOCK_COVERAGE || [
      { area: "Scheduled tasks", state: "Checked", detail: "8 files" },
      { area: "Services (auto-start)", state: "Checked", detail: "0 services" },
      { area: "WMI subscriptions", state: "Checked", detail: "0 entries" },
    ])
      emit("scan-progress", { stage: "source-state", ...row });
    const n = Math.max(1, Number(window.__CURE_MOCK_ITEM_COUNT) || 8);
    const items = makeMockItems(n);
    if (window.__CURE_MOCK_GUIDANCE_SOURCE)
      items[0].source = window.__CURE_MOCK_GUIDANCE_SOURCE;
    if (window.__CURE_MOCK_LONG_VALUES) {
      items[0].name = "<img src=x onerror=alert(1)>";
      items[0].command =
        "C:\\Evidence\\" +
        "long-folder-name\\".repeat(30) +
        "target.exe --inspect";
      items[0].location = items[0].command;
    }
    emit("scan-progress", {
      stage: "scoring",
      message:
        "Risk-scoring " + n + " persistence entr" + (n === 1 ? "y" : "ies"),
    });
    await delay(600);

    // same pacing math as the Rust backend: 5000ms target split across items,
    // clamped to 15..250ms so tiny scans don't crawl and huge ones don't stall
    const perItem = Math.min(250, Math.max(15, Math.round(5000 / n)));

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
      // Mirrors the real backend: NO automatic remediation. Every HighRisk /
      // Suspicious finding lands in the review queue; relocation happens
      // only through the explicit per-item confirm path (quarantine_entry).
      if (item.risk === "HighRisk" || item.risk === "Suspicious") {
        review.push(item);
      } else {
        safe += 1;
      }
    }

    emit("scan-progress", {
      stage: "process-scan",
      message: "Inspecting running processes",
    });
    await delay(200);
    emit("scan-progress", {
      stage: "ransom-detect",
      message: "Inspecting ransom indicators",
    });
    await delay(200);
    emit("scan-progress", { stage: "done", message: "Scan complete" });
    window.__CURE_SCAN_DONE = true;
    console.info(
      "[mock-tauri] run #" +
        scanRuns +
        ": " +
        n +
        " items @ " +
        perItem +
        "ms/item",
    );

    var result = {
      total: n,
      high_risk_cleaned: [], // real backend never auto-cleans; review-only
      suspicious_for_review: review.map(toScored),
      safe,
      source_states: [
        { area: "Registry autoruns", state: "Checked", detail: "3 values" },
        { area: "Scheduled tasks", state: "Checked", detail: "8 files" },
        {
          area: "Services (auto-start)",
          state: "Checked",
          detail: "0 services",
        },
        { area: "WMI subscriptions", state: "Checked", detail: "0 entries" },
      ],
      elevated: false,
    };

    if (window.__CURE_MOCK_COVERAGE)
      result.source_states = window.__CURE_MOCK_COVERAGE;
    if (window.__CURE_MOCK_SWEEP) {
      var sweepProcs = [
        {
          name: "suspicious_loader.exe",
          pid: 4242,
          exe_path:
            "C:\\Users\\test\\AppData\\Local\\Temp\\suspicious_loader.exe",
          score: 62,
          risk: "HighRisk",
          reasons: ["Unsigned binary", "Random name"],
        },
        {
          name: "unknown_tool.exe",
          pid: 8080,
          exe_path: "C:\\Users\\test\\Downloads\\unknown_tool.exe",
          score: 41,
          risk: "HighRisk",
          reasons: ["Profile exe", "Unsigned Binary"],
        },
        {
          name: "helper.dll",
          pid: 11234,
          exe_path: "C:\\ProgramData\\helper.dll",
          score: 22,
          risk: "Suspicious",
          reasons: ["Unsigned Binary"],
        },
      ];
      var sweepRansom = [
        {
          finding_type: "ransom-note",
          path: "C:\\Users\\test\\Documents\\DECRYPT_MY_FILES.txt",
          detail:
            'Matched pattern: DECRYPT — "All your files have been encrypted by LockBit 3.0."',
          suspected_family: "LockBit",
        },
        {
          finding_type: "bulk-encryption",
          path: "C:\\Users\\test\\Pictures",
          detail: '17 files with unusual extension ".locked" (avg age 3 days)',
          suspected_family: null,
        },
      ];
      sweepProcs.forEach(function (p) {
        emit("scan-progress", {
          stage: "process-flagged",
          name: p.name,
          pid: p.pid,
          risk: p.risk,
          score: p.score,
        });
      });
      sweepRansom.forEach(function (r) {
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
          case "list_overlay_candidates": {
            const hits = window.__CURE_MOCK_OVERLAY_HITS || 0;
            const candidates = [];
            for (let i = 0; i < hits; i++) {
              candidates.push({
                hwnd: 1000 + i,
                title:
                  i === 0
                    ? "SIMULATED RANSOM SCREEN"
                    : "OVERLAY WINDOW " + (i + 1),
                process:
                  i === 0 ? "fake-overlay.exe" : "overlay-" + (i + 1) + ".exe",
                path: "C:\\cure-mock\\overlay-" + (i + 1) + ".exe",
                pid: 9000 + i,
                signature: "unsigned",
                width: 1920,
                height: 1080,
                coverage_pct: 100.0,
              });
            }
            return delay(80).then(() => ({
              checked: 12,
              candidates: candidates,
            }));
          }
          case "close_overlay_window": {
            return delay(60).then(() => ({
              closed: true,
              terminated: !!(args && args.force),
            }));
          }
          case "allowlist_overlay": {
            return delay(40).then(
              () => "allowlisted (this binary only): " + (args && args.path),
            );
          }
          case "run_auto_scan":
            return runAutoScan();
          case "quarantine_entry": {
            window.__CURE_MOCK_QUARANTINE_CALLS =
              (window.__CURE_MOCK_QUARANTINE_CALLS || 0) + 1;
            if (window.__CURE_MOCK_ACTION_ERROR)
              return Promise.reject(
                new Error("File move refused. Original remains available."),
              );
            const qid = String(
              args && args.id ? args.id : "mock-q-" + Date.now(),
            );
            const qname = String(args && args.name ? args.name : "entry");
            if (!__mockQuarantine.some((r) => r.id === qid)) {
              __mockQuarantine.push({
                id: qid,
                name: qname,
                source: "startup-folder",
                original_path: STARTUP_DIR + qname,
                quarantine_path:
                  "C:\\cure-mock\\data\\quarantine\\" + qid + "_" + qname,
                archived_at: "2026-10-04T03:30:00Z",
                state: "Committed",
                file_size: 18304,
                sha256_hex:
                  "bd5b914a3b5d21dc498e99fc5f766f4a56b4afcb03e482fd931a21c5b24c91da",
                acl_captured: true,
                security_notes: [],
              });
            }
            return delay(450).then(
              () => "moved C:\\" + qname + " -> quarantine (mock)",
            );
          }
          case "undo_entry":
            return delay(250).then(() => {
              const qid = String(args && args.id ? args.id : "");
              const qi = __mockQuarantine.findIndex((r) => r.id === qid);
              if (qi !== -1) __mockQuarantine.splice(qi, 1);
            });
          case "list_quarantine":
            if (window.__CURE_MOCK_QUARANTINE_ERROR)
              return Promise.reject(
                new Error(
                  "Quarantine records unavailable. Choose Refresh to retry.",
                ),
              );
            return delay(200).then(snapshotQuarantine);
          case "entry_details":
            return delay(200).then(() => ({
              signature: "UNSIGNED",
              publisher: null,
              shortcut: null,
              task: null,
              target_path:
                "C:\\Users\\bob\\AppData\\Local\\Temp\\acmetray0.exe",
              sha256_hex:
                "bd5b914a3b5d21dc498e99fc5f766f4a56b4afcb03e482fd931a21c5b24c91da",
              file_size: 18304,
              modified_unix_secs: 1791077400,
            }));
          case "reveal_location":
            return delay(200).then(() => "C:\\cure-mock\\revealed");
          case "export_report":
            return delay(300).then(
              () => "C:\\cure-mock\\data\\cure-report-mock.txt",
            );
          case "start_incident_observation": {
            const dur = Number(
              (args &&
                (args.duration_secs !== undefined
                  ? args.duration_secs
                  : args.durationSecs)) ||
                30,
            );
            return delay(400).then(() => ({
              investigation_id: "INC-MOCK-1",
              started_at: new Date().toISOString(),
              duration_secs: dur,
              elevated: false,
              process_observation: "Available",
              window_observation: "Available",
              processes: [
                {
                  pid: 8412,
                  ppid: 1234,
                  name: "updater-mock.exe",
                  exe_path:
                    "C:\\Users\\bob\\AppData\\Local\\Mock\\updater-mock.exe",
                  command_line:
                    '"C:\\Users\\bob\\AppData\\Local\\Mock\\updater-mock.exe" --silent',
                  first_seen_ms: 2421,
                  last_seen_ms: 3200,
                  exited: true,
                  via_events: false,
                  pre_existing: false,
                },
              ],
              windows: [
                {
                  pid: 8412,
                  title: "Mock Update Error",
                  class_name: "MockDialog",
                  first_seen_ms: 2500,
                  last_seen_ms: 3242,
                  closed: true,
                  pre_existing: false,
                },
              ],
              correlations: [
                {
                  process_pid: 8412,
                  process_name: "updater-mock.exe",
                  finding_id: "mock-item-0",
                  finding_name: "AcmeTray0.bat",
                  finding_source: "StartupFolder",
                  level: "Direct",
                  evidence: [
                    "startup entry references this exact executable path",
                    "observed launch at pid 8412",
                  ],
                },
              ],
              timeline: [
                {
                  t_ms: 0,
                  wall_time: "18:42:01.000",
                  kind: "ObservationStarted",
                  text: "Login observation started",
                },
                {
                  t_ms: 2421,
                  wall_time: "18:42:03.421",
                  kind: "ProcessCreated",
                  text: "Process created: updater-mock.exe (pid 8412)",
                },
                {
                  t_ms: 2421,
                  wall_time: "18:42:03.421",
                  kind: "CorrelationNoted",
                  text: "DIRECT correlation: updater-mock.exe ↔ AcmeTray0.bat (StartupFolder)",
                },
                {
                  t_ms: 2500,
                  wall_time: "18:42:03.500",
                  kind: "WindowOpened",
                  text: 'Window opened: "Mock Update Error" (pid 8412, MockDialog)',
                },
                {
                  t_ms: 3200,
                  wall_time: "18:42:04.200",
                  kind: "ProcessExited",
                  text: "Process exited: updater-mock.exe (pid 8412)",
                },
                {
                  t_ms: 3242,
                  wall_time: "18:42:04.242",
                  kind: "WindowClosed",
                  text: 'Window closed: "Mock Update Error" after 742 ms',
                },
                {
                  t_ms: dur * 1000,
                  wall_time: "18:42:31.000",
                  kind: "ObservationEnded",
                  text: "Observation window ended",
                },
              ],
              verdict: "CauseIdentified",
              truncated: false,
            }));
          }
          case "export_incident_report":
            return delay(300).then(
              () => "C:\\cure-mock\\data\\cure-incident-mock.txt",
            );
          case "open_quarantine_folder":
            return delay(350).then(() => {
              if (window.__CURE_MOCK_FOOTER_ERRORS) {
                throw new Error(
                  "No quarantine folder yet — nothing quarantined",
                );
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
                console.info(
                  "[mock-tauri] exit_app invoked (dev harness stays open)",
                );
              }
            });
          case "scan_cleanup":
            return scanCleanupMock();
          case "run_cleanup":
            return runCleanupMock(args);
          case "kill_high_risk_processes":
            return delay(350).then(function () {
              var procs = (args && args.processes) || [];
              var killed = [];
              var failed = [];
              procs.forEach(function (pair) {
                if (pair[0] === "fail-on-kill.exe") {
                  failed.push(pair[0] + " (access denied)");
                } else {
                  killed.push({
                    name: pair[0],
                    pid: pair[1],
                    exe_path: "",
                    score: 0,
                    risk: "HighRisk",
                    reasons: [],
                  });
                }
              });
              return { killed: killed, failed: failed };
            });
          case "start_canary_guard":
            return delay(150).then(function () {
              if (window.__CURE_MOCK_CANARY_ALERT) {
                setTimeout(function () {
                  emit("canary-alert", {
                    kind: "burst-encryption",
                    folder: "C:\\Users\\bob\\Documents",
                    file: "8 files in 30s",
                    action: "burst",
                    at_secs: Math.floor(Date.now() / 1000),
                    severity: 1,
                  });
                }, 2000);
              }
              return "started";
            });
          case "stop_canary_guard":
            return delay(80).then(function () {
              return "stopped";
            });
          case "canary_status":
            return delay(20).then(function () {
              return { active: false, alert_count: 0 };
            });
          default:
            return Promise.reject(
              new Error("mock-tauri: unknown command " + command),
            );
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

  console.info(
    "[mock-tauri] active — " +
      window.__CURE_MOCK_ITEM_COUNT +
      " mock items/run",
  );
})();
