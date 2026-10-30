(function () {
  "use strict";

  /* The mock replaces the Tauri API outright, so a build that ships it
     never reaches the real backend.

     The guard below is the opposite case: a shell that has the real API
     must not be silently swapped for sample data, because that would make
     a test look like it exercised the engine when it did not. The public
     demo sets `__CURE_FORCE_MOCK` to say, deliberately, that this build
     is the sample-data one. */
  if (window.__TAURI__ && !window.__CURE_FORCE_MOCK) {
    console.warn("[mock-tauri] real window.__TAURI__ detected; mock disabled");
    return;
  }

  const listeners = Object.create(null);
  const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

  /* Note on the runtime's low-level bridge. The Tauri runtime injects
     `window.__TAURI_INTERNALS__` into every window, and its `invoke`
     property is non-writable and non-configurable — it cannot be
     replaced from a page, and assigning to it in strict mode throws.
     That route is therefore closed in the *build*, not here: the public
     demo is compiled without an invoke handler, so every command is
     refused by the runtime whichever door it comes through. */

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
          evidence: [
            {
              level: "Observed",
              detail:
                "-20 command path is a trusted install location (Program Files/System32)",
            },
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
            evidence: [
              {
                level: "Observed",
                detail:
                  "+30 command path sits in a temp/downloads/public drop zone",
              },
              {
                level: "Observed",
                detail: "+25 entry name looks randomly generated",
              },
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
            evidence: [
              {
                level: "Observed",
                detail:
                  "-20 command path is a trusted install location (Program Files/System32)",
              },
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
            evidence: [
              {
                level: "Observed",
                detail: "+10 executable runs directly from a user profile folder",
              },
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
            evidence: [
              {
                level: "Observed",
                detail:
                  "+30 command path sits in a temp/downloads/public drop zone",
              },
              { level: "Observed", detail: "+40 Invalid Signature" },
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
            evidence: [
              {
                level: "Observed",
                detail:
                  "+25 PowerShell invoked with encoded command or hidden window",
              },
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
            evidence: [
              {
                level: "Observed",
                detail:
                  "+30 command path sits in a temp/downloads/public drop zone",
              },
              { level: "Observed", detail: "Known Malware Hash" },
            ],
          };
      }
      if (window.__CURE_MOCK_ALL_SAFE) {
        item.risk = "Safe";
        item.score = 0;
        item.evidence = [
          {
            level: "Observed",
            detail:
              "-20 command path is a trusted install location (Program Files/System32)",
          },
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
      // __CURE_MOCK_LEGACY_EVIDENCE simulates a pre-evidence payload:
      // findings carry no evidence list at all, so the UI must show its
      // explicit "Legacy report: evidence not recorded" state.
      evidence: window.__CURE_MOCK_LEGACY_EVIDENCE
        ? []
        : item.evidence || [],
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

  // ---- storage workstation mock ------------------------------------------
  //
  // Deterministic, clearly-labelled SAMPLE DATA for the browser test harness.
  // Every number here is invented; none of it came from a real machine, and
  // the shell that loads this file says so in the wordmark.
  //
  //   __CURE_MOCK_STORAGE_ERROR      make storage_scan reject
  //   __CURE_MOCK_STORAGE_DELAY_MS   how long the scan pretends to take
  //   __CURE_MOCK_STORAGE_EMPTY      report a machine with nothing to reclaim
  //   __CURE_MOCK_STORAGE_PARTIAL    report an incomplete scan (unreadable roots)
  //   __CURE_MOCK_STORAGE_LOCKED     report one locked file at execution time
  //   __CURE_MOCK_STORAGE_STALE      make one queued item go stale before execute
  if (window.__CURE_MOCK_STORAGE_ERROR === undefined)
    window.__CURE_MOCK_STORAGE_ERROR = false;
  if (window.__CURE_MOCK_STORAGE_DELAY_MS === undefined)
    window.__CURE_MOCK_STORAGE_DELAY_MS = 900;
  if (window.__CURE_MOCK_STORAGE_EMPTY === undefined)
    window.__CURE_MOCK_STORAGE_EMPTY = false;
  if (window.__CURE_MOCK_STORAGE_PARTIAL === undefined)
    window.__CURE_MOCK_STORAGE_PARTIAL = false;
  if (window.__CURE_MOCK_STORAGE_LOCKED === undefined)
    window.__CURE_MOCK_STORAGE_LOCKED = false;
  if (window.__CURE_MOCK_STORAGE_STALE === undefined)
    window.__CURE_MOCK_STORAGE_STALE = false;

  const KB = 1024;
  const MB = 1024 * 1024;
  const GB = 1024 * 1024 * 1024;

  /* Deterministic ages in seconds-since-epoch, anchored so captures are
     byte-stable across runs. */
  const MOCK_NOW = 1760000000;
  const daysAgo = (d) => MOCK_NOW - d * 86400;

  const SAMPLE_FILES = [
    {
      path: "C:\\Users\\bob\\AppData\\Local\\Temp\\sess-8f21a0.tmp",
      name: "sess-8f21a0.tmp",
      size: 18 * MB,
      modified: daysAgo(0.2),
      cls: "user_temp",
      clsLabel: "User temp files",
      tier: "safe_to_review",
      reasons: [
        "in a location the operating system treats as temporary",
        "regenerable by the program that wrote it; individual files are still listed before anything is removed",
      ],
    },
    {
      path: "C:\\Users\\bob\\AppData\\Local\\Temp\\chrome\\CrRendererChild\\index.dat",
      name: "index.dat",
      size: 6 * MB,
      modified: daysAgo(1.1),
      cls: "user_temp",
      clsLabel: "User temp files",
      tier: "safe_to_review",
      reasons: ["in a location the operating system treats as temporary"],
    },
    {
      path: "C:\\Users\\bob\\AppData\\Local\\Google\\Chrome\\User Data\\Default\\Cache\\Cache_Data\\f_00001",
      name: "f_00001",
      size: 41 * MB,
      modified: daysAgo(0.4),
      cls: "browser_cache",
      clsLabel: "Browser caches",
      tier: "safe_to_review",
      reasons: [
        "a named cache directory inside a browser profile",
        "regenerable by the program that wrote it; individual files are still listed before anything is removed",
      ],
    },
    {
      path: "C:\\Users\\bob\\AppData\\Local\\Microsoft\\Edge\\User Data\\Profile 2\\Cache\\data_2",
      name: "data_2",
      size: 27 * MB,
      modified: daysAgo(0.9),
      cls: "browser_cache",
      clsLabel: "Browser caches",
      tier: "safe_to_review",
      reasons: ["a named cache directory inside a browser profile"],
    },
    {
      path: "C:\\Windows\\Temp\\mpv_9c1e.tmp",
      name: "mpv_9c1e.tmp",
      size: 212 * MB,
      modified: daysAgo(3.4),
      cls: "windows_temp",
      clsLabel: "Windows temp files",
      tier: "safe_to_review",
      reasons: ["in a location the operating system treats as temporary"],
    },
    {
      path: "C:\\Users\\bob\\AppData\\Local\\CrashDumps\\chrome.exe.10428.dmp",
      name: "chrome.exe.10428.dmp",
      size: 640 * MB,
      modified: daysAgo(11),
      cls: "crash_dumps",
      clsLabel: "Crash dumps",
      tier: "safe_to_review",
      reasons: ["diagnostic output regenerated by the next failure"],
    },
    {
      path: "C:\\Windows\\Logs\\CBS\\CBS.log",
      name: "CBS.log",
      size: 96 * MB,
      modified: daysAgo(0.7),
      cls: "diagnostic_logs",
      clsLabel: "Diagnostic logs",
      tier: "safe_to_review",
      reasons: ["diagnostic output regenerated by the next failure"],
    },
    {
      path: "C:\\$Recycle.Bin\\S-1-5-21-1004336348-1177238915-682003330-1001\\$R3KQ7ZP.tmp",
      name: "$R3KQ7ZP.tmp",
      size: 1.4 * GB,
      modified: daysAgo(6),
      cls: "recycle_bin",
      clsLabel: "Recycle Bin",
      tier: "safe_to_review",
      reasons: ["a deleted-item payload the shell itself already moved here"],
    },
    {
      path: "C:\\$Recycle.Bin\\S-1-5-21-1004336348-1177238915-682003330-1001\\$R9WD2LM.iso",
      name: "$R9WD2LM.iso",
      size: 3.1 * GB,
      modified: daysAgo(19),
      cls: "recycle_bin",
      clsLabel: "Recycle Bin",
      tier: "safe_to_review",
      reasons: ["a deleted-item payload the shell itself already moved here"],
    },
    {
      path: "C:\\Users\\bob\\Downloads\\driver_pack_2024.msi",
      name: "driver_pack_2024.msi",
      size: 512 * MB,
      modified: daysAgo(41),
      cls: "user_selected",
      clsLabel: "Selected location",
      tier: "review_carefully",
      reasons: [
        "the user chose this location; intent is not known",
        "classified as review carefully",
      ],
    },
    {
      path: "C:\\Users\\bob\\Downloads\\quarterly-archive.zip",
      name: "quarterly-archive.zip",
      size: 2.2 * GB,
      modified: daysAgo(88),
      cls: "user_selected",
      clsLabel: "Selected location",
      tier: "review_carefully",
      reasons: ["the user chose this location; intent is not known"],
    },
    {
      path: "C:\\Users\\bob\\project\\node_modules\\left-pad\\index.js",
      name: "index.js",
      size: 3 * KB,
      modified: daysAgo(220),
      cls: "dev_cache",
      clsLabel: "Developer build caches",
      tier: "review_carefully",
      reasons: ["developer build output — regenerated by the toolchain"],
    },
    {
      path: "C:\\Users\\bob\\project\\node_modules\\moment\\package.json",
      name: "package.json",
      size: 2 * KB,
      modified: daysAgo(220),
      cls: "dev_cache",
      clsLabel: "Developer build caches",
      tier: "review_carefully",
      reasons: ["developer build output — regenerated by the toolchain"],
    },
    {
      path: "C:\\Program Files\\Vendor Suite\\runtime.dll",
      name: "runtime.dll",
      size: 88 * MB,
      modified: daysAgo(300),
      cls: "other",
      clsLabel: "Other",
      tier: "protected",
      reasons: ["inside an installed application directory"],
    },
    {
      path: "C:\\Windows\\WinSxS\\amd64_placeholder.manifest",
      name: "amd64_placeholder.manifest",
      size: 4 * KB,
      modified: daysAgo(500),
      cls: "other",
      clsLabel: "Other",
      tier: "protected",
      reasons: ["`windows/winsxs` is required by Windows itself"],
    },
    {
      path: "C:\\Users\\bob\\AppData\\Local\\RetiredVendor\\cache.bin",
      name: "cache.bin",
      size: 148 * MB,
      modified: daysAgo(430),
      cls: "abandoned_app_data",
      clsLabel: "Possibly unused app data",
      tier: "unknown",
      reasons: [
        "newest file inside was modified 430 days ago",
        "no installed program lists this location",
        "application data is never a deletion recommendation; age alone proves nothing",
      ],
    },
  ];

  const SAMPLE_DUPLICATES = [
    {
      id: "9f2a1c4d7e8b0a61:5368709120:3",
      sha256: "9f2a1c4d7e8b0a6138bd4f2e9a7c0516d8e3f4b2a6c9d0e1f2a3b4c5d6e7f80",
      size: 5 * 1024 * MB,
      reclaim: 5 * 1024 * MB,
      copies: [
        {
          path: "C:\\Users\\bob\\Videos\\archive-2023.mp4",
          modified: daysAgo(210),
          kept: true,
          tier: "review_carefully",
          why: "kept: the most recent copy, or the one outside a disposable location",
        },
        {
          path: "C:\\Users\\bob\\Downloads\\archive-2023.mp4",
          modified: daysAgo(210),
          kept: false,
          tier: "review_carefully",
          why: "user-visible file in a downloads folder",
        },
        {
          path: "D:\\MediaBackup\\archive-2023.mp4",
          modified: daysAgo(210),
          kept: false,
          tier: "protected",
          why: "this engine will not propose removing this copy",
        },
      ],
    },
    {
      id: "3c8b7a2e1d5f9046:2147483648:2",
      sha256: "3c8b7a2e1d5f9046112233445566778899aabbccddeeff0011223344556677",
      size: 2 * 1024 * MB,
      reclaim: 2 * 1024 * MB,
      copies: [
        {
          path: "C:\\Users\\bob\\AppData\\Local\\Temp\\staging-video.tmp",
          modified: daysAgo(1.4),
          kept: true,
          tier: "safe_to_review",
          why: "kept: the most recent copy, or the one outside a disposable location",
        },
        {
          path: "C:\\Users\\bob\\AppData\\Local\\Temp\\staging-video-copy.tmp",
          modified: daysAgo(1.6),
          kept: false,
          tier: "safe_to_review",
          why: "in a location the operating system treats as temporary",
        },
      ],
    },
  ];

  const SAMPLE_ROOTS = [
    {
      label: "User temp",
      cls: "user_temp",
      resolved: true,
      bytes: 24 * MB,
      files: 2,
    },
    {
      label: "Windows temp",
      cls: "windows_temp",
      resolved: true,
      bytes: 212 * MB,
      files: 1,
    },
    {
      label: "Google Chrome cache",
      cls: "browser_cache",
      resolved: true,
      bytes: 41 * MB,
      files: 1,
    },
    {
      label: "Microsoft Edge cache",
      cls: "browser_cache",
      resolved: true,
      bytes: 27 * MB,
      files: 1,
    },
    {
      label: "Recycle Bin",
      cls: "recycle_bin",
      resolved: true,
      bytes: 4.5 * 1024 * MB,
      files: 2,
    },
    {
      label: "Crash dumps",
      cls: "crash_dumps",
      resolved: true,
      bytes: 640 * MB,
      files: 1,
    },
    {
      label: "Diagnostic logs",
      cls: "diagnostic_logs",
      resolved: true,
      bytes: 96 * MB,
      files: 1,
    },
    {
      label: "Previous Windows install",
      cls: "windows_old",
      resolved: false,
      reason: "not present on this machine",
      bytes: 0,
      files: 0,
    },
    {
      label: "Windows Update cache",
      cls: "windows_update_cache",
      resolved: false,
      reason: "not present on this machine",
      bytes: 0,
      files: 0,
    },
    {
      label: "Downloads",
      cls: "user_selected",
      resolved: true,
      bytes: 2.7 * 1024 * MB,
      files: 2,
    },
  ];

  function mockScanFiles() {
    return SAMPLE_FILES.map(function (f) {
      return {
        id: f.path.toLowerCase().replace(/\\/g, "/"),
        path: f.path,
        name: f.name,
        parent: f.path.slice(0, f.path.lastIndexOf("\\")),
        extension: (f.name.split(".").pop() || "").toLowerCase(),
        size_logical: f.size,
        size_allocated: f.size,
        modified_unix: f.modified,
        accessed_unix: null,
        created_unix: f.modified,
        hard_link_count: 1,
        is_read_only: false,
        is_hidden: false,
        class: f.cls,
        class_label: f.clsLabel,
        tier: f.tier,
        tier_label: TIER_TEXT[f.tier],
        bulk_eligible: f.tier === "safe_to_review",
        reasons: f.reasons.slice(),
      };
    });
  }

  /* The folder list the engine measured.

     It is derived from the sample files the same way the real walk derives
     it from the disk: every directory that holds a measured file, and the
     directories above it, each carrying the bytes of everything below it.
     Without this the map has nothing to drill into, and the drill-down
     would only ever be exercised against a real machine. */
  function mockScanFolders(files) {
    const rows = new Map();
    const add = (path, file) => {
      let row = rows.get(path);
      if (!row) {
        row = {
          id: path.toLowerCase().replace(/\\/g, "/"),
          path: path,
          name: path.slice(path.lastIndexOf("\\") + 1),
          depth: 0,
          size_allocated: 0,
          size_logical: 0,
          file_count: 0,
          dir_count: 0,
          class: file.class,
          tier: file.tier,
          incomplete: false,
        };
        rows.set(path, row);
      }
      /* The most cautious member decides the folder's tier, exactly as the
         engine does for a category. */
      const order = ["safe_to_review", "review_carefully", "unknown", "protected"];
      if (order.indexOf(file.tier) > order.indexOf(row.tier)) row.tier = file.tier;
      row.size_allocated += file.size_allocated;
      row.size_logical += file.size_logical;
      row.file_count += 1;
      row.dir_count += 1;
      return row;
    };
    for (const file of files) {
      let path = file.parent;
      let depth = 0;
      while (path && path.length > 3) {
        const row = add(path, file);
        row.depth = depth;
        const i = path.lastIndexOf("\\");
        if (i <= 2) break;
        path = path.slice(0, i);
        depth += 1;
      }
    }
    return Array.from(rows.values());
  }

  const TIER_TEXT = {
    safe_to_review: "Safe to review",
    review_carefully: "Review carefully",
    protected: "Protected",
    unknown: "Unknown",
  };

  let mockStorageScanId = "mock-scan-0001";
  /* The approved plan, exactly as the real backend keeps it: the UI never
     sends a selection to execute, it sends the digest of the plan it saw. */
  let mockApprovedPlan = null;
  /* The latest synthetic measurement, kept only so a demo case file can be
     rendered from the same numbers the board just showed. */
  let mockLastScan = null;

  function buildMockStorageScan(depth) {
    mockStorageScanId += "-r";
    const files = window.__CURE_MOCK_STORAGE_EMPTY ? [] : mockScanFiles();
    const byClass = new Map();
    for (const f of files) {
      const row = byClass.get(f.class) || {
        key: f.class,
        label: f.class_label,
        tier: f.tier,
        item_count: 0,
        bytes_allocated: 0,
        bytes_logical: 0,
      };
      /* The most cautious member wins, exactly as the engine does. */
      if (TIER_TEXT[f.tier] && row.tier !== f.tier) {
        if (["protected", "unknown", "review_carefully", "safe_to_review"].indexOf(f.tier) >
          ["protected", "unknown", "review_carefully", "safe_to_review"].indexOf(row.tier)) {
          row.tier = f.tier;
        }
      }
      row.item_count += 1;
      row.bytes_allocated += f.size_allocated;
      row.bytes_logical += f.size_logical;
      byClass.set(f.class, row);
    }

    const dupeReclaim = window.__CURE_MOCK_STORAGE_EMPTY
      ? 0
      : SAMPLE_DUPLICATES.reduce((s, d) => s + d.reclaim, 0);
    const safe = files
      .filter((f) => f.tier === "safe_to_review")
      .reduce((s, f) => s + f.size_allocated, 0);
    const careful = files
      .filter((f) => f.tier === "review_carefully")
      .reduce((s, f) => s + f.size_allocated, 0);
    const prot = files
      .filter((f) => f.tier === "protected" || f.tier === "unknown")
      .reduce((s, f) => s + f.size_allocated, 0);

    const partial = window.__CURE_MOCK_STORAGE_PARTIAL;
    const folders = mockScanFolders(files);
    const failures = partial
      ? [
          {
            path: "C:\\System Volume Information",
            reason: "access denied",
          },
          {
            path: "C:\\ProgramData\\Vendor\\cache",
            reason: "access denied",
          },
        ]
      : [];

    const result = {
      scan_id: mockStorageScanId,
      finished_unix: 1760000000,
      duration_ms: window.__CURE_MOCK_STORAGE_DELAY_MS,
      headline: {
        /* The headline is the union of the removable files. The duplicate
           figure is a subset of it and is never added on top. */
        safe_bytes: safe,
        careful_bytes: careful,
        duplicate_bytes: dupeReclaim,
        duplicate_bulk_bytes: dupeReclaim,
        certainty: partial ? "lower_bound" : "measured",
        certainty_label: partial ? "At least" : "Measured",
        incomplete: !!partial,
        safe_bytes_label: humanBytes(safe),
        careful_bytes_label: humanBytes(careful),
      },
      files_total: files.length,
      tier_counts: {
        safe_to_review: files.filter((f) => f.tier === "safe_to_review").length,
        review_carefully: files.filter((f) => f.tier === "review_carefully").length,
        protected: files.filter((f) => f.tier === "protected").length,
        unknown: files.filter((f) => f.tier === "unknown").length,
      },
      protected_bytes: prot,
      duplicate_reclaimable_bytes: dupeReclaim,
      summaries: Array.from(byClass.values()),
      coverage: {
        completeness: partial
          ? "Partial — some locations could not be read"
          : "Complete for the roots scanned",
        files_seen: files.length,
        dirs_seen: files.length * 3,
        permission_denied_dirs: partial ? 2 : 0,
        permission_denied_files: 0,
        reparse_points_skipped: 3,
        skipped_links: [],
        locked_or_vanished: 1,
        duration_ms: window.__CURE_MOCK_STORAGE_DELAY_MS,
        cancelled: false,
        volume_total_bytes: 512 * 1024 * MB,
        volume_free_bytes: 88 * 1024 * MB,
        volume_label: "SampleVolume",
        failures: failures,
        roots: SAMPLE_ROOTS.map((r) => ({
          path: "C:\\" + r.label.replace(/[^a-z0-9]+/gi, "\\"),
          label: r.label,
          class: r.cls,
          is_catalogue: true,
          resolved: r.resolved,
          reason: r.reason || "",
          files_seen: r.files,
          bytes_allocated: r.bytes,
          incomplete: false,
        })),
      },
      duplicate_coverage: {
        files_considered: files.length,
        size_buckets: 4,
        partial_hash_groups: 2,
        full_hash_groups: 2,
        verified_sets: window.__CURE_MOCK_STORAGE_EMPTY ? 0 : SAMPLE_DUPLICATES.length,
        skipped_oversize: 1,
        skipped_hardlinked: 0,
        skipped_changed_during_scan: 0,
        skipped_unreadable: 0,
        bytes_hashed: 4 * 1024 * MB,
        truncated: false,
        summary: "Checked " + files.length + " files in full.",
      },
      files: files,
      folders: folders,
      large_files: files,
      large_folders: folders
        .slice()
        .sort((a, b) => b.size_allocated - a.size_allocated)
        .slice(0, 12),
      duplicates: window.__CURE_MOCK_STORAGE_EMPTY
        ? []
        : SAMPLE_DUPLICATES.map(function (d) {
            return {
              id: d.id,
              size_allocated: d.size,
              sha256: d.sha256,
              reclaimable_bytes: d.reclaim,
              protected_copies: d.copies.filter((c) => c.tier === "protected").length,
              copies: d.copies.map(function (c) {
                return {
                  id: c.path.toLowerCase().replace(/\\/g, "/"),
                  path: c.path,
                  name: c.path.slice(c.path.lastIndexOf("\\") + 1),
                  parent: c.path.slice(0, c.path.lastIndexOf("\\")),
                  size_allocated: d.size,
                  modified_unix: c.modified,
                  tier: c.tier,
                  tier_label: TIER_TEXT[c.tier],
                  class_label: "Selected location",
                  is_kept_candidate: c.kept,
                  reasons: [c.why],
                };
              }),
            };
          }),
      abandoned: window.__CURE_MOCK_STORAGE_EMPTY
        ? []
        : [
            {
              path: "C:\\Users\\bob\\AppData\\Local\\RetiredVendor",
              app_name: "RetiredVendor",
              size_allocated: 148 * MB,
              file_count: 912,
              days_since_change: 430,
              tier: "unknown",
              tier_label: "Unknown",
              reasons: [
                "newest file inside was modified 430 days ago",
                "no installed program lists this location",
              ],
              caveat:
                "No file here has been deleted or modified by C.U.R.E. An application that is still installed but idle, keeps state on another volume, or holds a licence tied to this folder will all look exactly the same to this measurement.",
              removable: false,
            },
          ],
    };
    result.investigation = buildMockInvestigation(result);
    mockLastScan = result;
    return result;
  }

  /* Deterministic, synthetic investigation model. It mirrors the shape of
     core::storage::detective so the demo and the product render the same
     screens from the same contracts, without touching a real disk. */
  function buildMockInvestigation(scan) {
    const measured = (scan.summaries || []).reduce(function (sum, s) {
      return sum + s.bytes_allocated;
    }, 0);
    const measuredLogical = (scan.summaries || []).reduce(function (sum, s) {
      return sum + s.bytes_logical;
    }, 0);
    const slices = (scan.summaries || []).map(function (s) {
      const band =
        s.tier === "safe_to_review"
          ? "regenerable"
          : s.tier === "review_carefully"
            ? "review"
            : s.tier;
      return {
        key: s.key,
        label: s.label,
        tier: s.tier,
        tier_label: TIER_TEXT[s.tier] || s.tier,
        band: band,
        bytes_allocated: s.bytes_allocated,
        bytes_logical: s.bytes_logical,
        bytes_allocated_label: humanBytes(s.bytes_allocated),
        item_count: s.item_count,
        share: measured > 0 ? s.bytes_allocated / measured : 0,
        lead_id: "cat-" + s.key,
      };
    });

    const SIGNIFICANCE = ["ordinary", "notable", "major"];
    function significanceOf(bytes) {
      if (bytes >= 512 * MB && measured > 0 && bytes / measured >= 0.15) {
        return "major";
      }
      if (bytes >= 128 * MB) return "notable";
      return "ordinary";
    }
    function bandLabel(band) {
      return band === "regenerable"
        ? "Regenerable"
        : band === "review"
          ? "Needs review"
          : band === "protected"
            ? "Protected"
            : "Unknown";
    }
    function riskFor(tier) {
      return tier === "safe_to_review"
        ? "safe"
        : tier === "review_carefully"
          ? "review_required"
          : tier === "protected"
            ? "protected"
            : "unknown";
    }

    const leads = slices.map(function (s, i) {
      const signific = significanceOf(s.bytes_allocated);
      const removable = s.tier === "safe_to_review" || s.tier === "review_carefully";
      return {
        id: "cat-" + s.key,
        kind: "category",
        title: s.label + " in the sample locations",
        location: s.label,
        location_label: s.label,
        class: s.key,
        tier: s.tier,
        within: "scanned-roots",
        scope_note: "A category, not a single file.",
        labels: [bandLabel(s.band), s.tier === "safe_to_review" ? "Safe to review" : s.tier === "review_carefully" ? "Review carefully" : s.tier === "protected" ? "Protected" : "Unknown"],
        bytes_allocated: s.bytes_allocated,
        bytes_logical: s.bytes_logical,
        bytes_allocated_label: humanBytes(s.bytes_allocated),
        bytes_logical_label: humanBytes(s.bytes_logical),
        item_count: s.item_count,
        share: measured > 0 ? s.bytes_allocated / measured : 0,
        significance: signific,
        significance_note:
          signific === "major"
            ? "This is the largest concentration in the sample measurement."
            : signific === "notable"
              ? "Worth a look before the brush clears the rest."
              : "Ordinary size in the sample measurement.",
        origin: {
          label: "Found by this measurement",
          confidence: "high",
          evidence:
            "The demo scan counted these items directly; no path was read from this machine.",
        },
        risk: riskFor(s.tier),
        risk_note:
          s.tier === "safe_to_review"
            ? "Sample data only; a real scan would still require your approval."
            : s.tier === "review_carefully"
              ? "Check the contents before selecting anything."
              : "This category is never offered for removal.",
        action_kind: removable ? "review_and_queue" : "not_offered",
        action_label: removable ? "Review in Storage Workstation" : "Not offered for removal",
        consequence: removable
          ? "Nothing happens until you approve a specific plan."
          : "The product will keep this category out of every plan.",
        evidence: [
          "Synthetic sample measurement, " + humanBytes(measured) + " in total.",
          s.item_count + " sample items in this category.",
        ],
      };
    });

    leads.sort(function (a, b) {
      const bySig =
        SIGNIFICANCE.indexOf(b.significance) - SIGNIFICANCE.indexOf(a.significance);
      return bySig || b.bytes_allocated - a.bytes_allocated;
    });

    const dupes = scan.duplicate_reclaimable_bytes || 0;
    if (dupes > 0) {
      leads.push({
        id: "duplicate-1",
        kind: "duplicate_set",
        title: "Identical copies in the sample set",
        location: "Sample locations",
        location_label: "Duplicate sets",
        class: null,
        tier: "safe_to_review",
        within: "scanned-roots",
        scope_note: "Verified by full-content hash in the demo data.",
        labels: ["Regenerable", "Byte-identical"],
        bytes_allocated: dupes,
        bytes_logical: dupes,
        bytes_allocated_label: humanBytes(dupes),
        item_count: (scan.duplicates || []).length,
        share: measured > 0 ? dupes / measured : 0,
        significance: significanceOf(dupes),
        significance_note: "One copy can stay; the extra copies are the finding.",
        origin: {
          label: "Found by duplicate verification",
          confidence: "high",
          evidence: "Synthetic copies share a content hash.",
        },
        risk: "safe",
        risk_note: "Keep at least one copy; the product never selects them all.",
        action_kind: "review_and_queue",
        action_label: "Review in Storage Workstation",
        consequence: "Nothing is removed until you choose which copy to keep.",
        evidence: ["Verified duplicate sets in the sample measurement."],
      });
    }

    const dormant = scan.abandoned || [];
    for (const a of dormant) {
      leads.push({
        id: "abandoned-" + a.path,
        kind: "dormant_app",
        title: a.app_name + " has gone quiet",
        location: a.path,
        location_label: a.app_name,
        class: null,
        tier: a.tier,
        within: "scanned-roots",
        scope_note: "A hint, not a verdict.",
        labels: ["Unknown", "Idle"],
        bytes_allocated: a.size_allocated,
        bytes_logical: a.size_allocated,
        bytes_allocated_label: humanBytes(a.size_allocated),
        bytes_logical_label: humanBytes(a.size_allocated),
        item_count: a.file_count,
        share: measured > 0 ? a.size_allocated / measured : 0,
        significance: significanceOf(a.size_allocated),
        significance_note:
          "Idle is not proof: an installed app can be quiet for months.",
        origin: {
          label: "Found by this measurement",
          confidence: "medium",
          evidence: a.reasons.slice(),
        },
        risk: "unknown",
        risk_note: a.caveat,
        action_kind: "not_offered",
        action_label: "Not offered for removal",
        consequence: "The product will not remove an unknown idle location.",
        evidence: a.reasons.slice(),
      });
    }

    const notable = leads
      .filter(function (l) {
        return l.significance !== "ordinary";
      })
      .map(function (l) {
        return l.id;
      });
    const majorLead = leads.find(function (l) {
      return l.significance === "major";
    });

    const safe = scan.headline.safe_bytes || 0;
    const careful = scan.headline.careful_bytes || 0;
    const candidate = safe + careful;
    const certainty = scan.headline.incomplete ? "lower_bound" : "measured";

    return {
      investigation_id: "demo-inv-" + scan.scan_id,
      scan_id: scan.scan_id,
      generated_unix: 1760000000,
      measured_bytes: measured,
      map: {
        measured_bytes: measured,
        measured_bytes_label: humanBytes(measured),
        measured_logical_bytes: measuredLogical,
        measured_logical_label: humanBytes(measuredLogical),
        candidate_bytes: candidate,
        candidate_label: humanBytes(candidate),
        safe_candidate_bytes: safe,
        safe_candidate_label: humanBytes(safe),
        careful_candidate_bytes: careful,
        careful_candidate_label: humanBytes(careful),
        protected_bytes: scan.protected_bytes || 0,
        protected_label: humanBytes(scan.protected_bytes || 0),
        unknown_bytes: slices.reduce(function (sum, s) {
          return sum + (s.tier === "unknown" ? s.bytes_allocated : 0);
        }, 0),
        unknown_label: humanBytes(
          slices.reduce(function (sum, s) {
            return sum + (s.tier === "unknown" ? s.bytes_allocated : 0);
          }, 0)
        ),
        slices: slices,
        certainty: certainty,
        completeness: scan.coverage ? scan.coverage.completeness : "",
      },
      headline: {
        measured_bytes: measured,
        measured_label: humanBytes(measured),
        candidate_bytes: candidate,
        candidate_label: humanBytes(candidate),
        candidate_note:
          "Of the storage this scan measured, " +
          humanBytes(safe) +
          " is regenerable and " +
          humanBytes(careful) +
          " needs an individual decision.",
        lead_count: leads.length,
        notable_count: notable.length,
        completeness: scan.coverage ? scan.coverage.completeness : "",
      },
      leads: leads,
      notable_leads: notable,
      major_lead: majorLead ? majorLead.id : null,
      lead_count: leads.length,
      coverage_notes: scan.coverage ? [scan.coverage.completeness] : [],
      evidence: {
        files_measured: scan.files_total,
        folders_measured: scan.coverage ? scan.coverage.dirs_seen : 0,
        duplicate_sets_verified: (scan.duplicates || []).length,
        duplicate_coverage_note: scan.duplicate_coverage
          ? scan.duplicate_coverage.summary
          : "",
        dormant_apps: dormant.length,
        permission_denied: scan.coverage
          ? scan.coverage.permission_denied_dirs + scan.coverage.permission_denied_files
          : 0,
        links_skipped: scan.coverage ? scan.coverage.reparse_points_skipped : 0,
        duration_ms: scan.duration_ms,
      },
    };
  }

  function buildMockCaseFile() {
    const scan = mockLastScan || buildMockStorageScan("standard");
    const inv = scan.investigation;
    const cats = (inv.leads || []).filter(function (l) {
      return l.kind === "category";
    });
    const status = "review_pending";
    return {
      case_id: "demo-case-" + String(scan.scan_id).replace(/[^a-z0-9]+/gi, "-"),
      title: "THE MISSING STORAGE CASE",
      status: status,
      status_label: "Review pending",
      generated_unix: 1760000000,
      measured_bytes: inv.measured_bytes,
      measured_label: humanBytes(inv.measured_bytes),
      candidate_bytes: cats.reduce(function (s, l) {
        return s + (l.action_kind === "review_and_queue" ? l.bytes_allocated : 0);
      }, 0),
      candidate_label: humanBytes(
        cats.reduce(function (s, l) {
          return s + (l.action_kind === "review_and_queue" ? l.bytes_allocated : 0);
        }, 0)
      ),
      files_removed: null,
      bytes_removed_estimate: null,
      bytes_removed_label: null,
      verified_recovered_bytes: null,
      verified_recovered_label: null,
      categories: (inv.map.slices || []).map(function (s) {
        return {
          key: s.key,
          label: s.label,
          band: s.band,
          band_label:
            s.band === "regenerable"
              ? "Regenerable"
              : s.band === "review"
                ? "Needs review"
                : s.band === "protected"
                  ? "Protected"
                  : "Unknown",
          bytes_allocated: s.bytes_allocated,
          bytes_label: humanBytes(s.bytes_allocated),
          item_count: s.item_count,
          share: inv.measured_bytes > 0 ? s.bytes_allocated / inv.measured_bytes : 0,
          significance:
            s.bytes_allocated >= 512 * MB &&
            inv.measured_bytes > 0 &&
            s.bytes_allocated / inv.measured_bytes >= 0.15
              ? "major"
              : s.bytes_allocated >= 128 * MB
                ? "notable"
                : "ordinary",
        };
      }),
      notable_leads: (inv.notable_leads || []).slice(0, 4).map(function (id) {
        const l = inv.leads.find(function (x) {
          return x.id === id;
        });
        return l ? l.title : "";
      }),
      lead_count: inv.lead_count,
      notable_count: inv.notable_leads.length,
      completeness: inv.map.partial ? "partial" : "complete",
      summary: null,
      summary_withheld: false,
      privacy: "standard",
      demo: !!window.__CURE_FORCE_MOCK,
    };
  }

  function humanBytes(n) {
    if (n >= 1024 * 1024 * 1024) return (n / (1024 * 1024 * 1024)).toFixed(2) + " GB";
    if (n >= 1024 * 1024) return (n / (1024 * 1024)).toFixed(1) + " MB";
    if (n >= 1024) return Math.round(n / 1024) + " KB";
    return n + " B";
  }

  function storagePreviewMock(args) {
    const paths = (args && (args.paths || args.paths_)) || [];
    const files = mockScanFiles();
    const items = [];
    const refused = [];
    for (const p of paths) {
      const f = files.find((x) => x.path.toLowerCase() === String(p).toLowerCase());
      if (!f) {
        refused.push({
          path: p,
          name: String(p).split(/[\\/]/).pop(),
          size_allocated: 0,
          size_label: "0 B",
          tier: "unknown",
          tier_label: "Unknown",
          class_label: "Other",
          reasons: ["not present in the current scan"],
          stale: true,
        });
        continue;
      }
      const stale =
        window.__CURE_MOCK_STORAGE_STALE && items.length === 0 ? true : false;
      if (f.tier === "protected" || f.tier === "unknown") {
        refused.push({
          path: f.path,
          name: f.name,
          size_allocated: f.size_allocated,
          size_label: humanBytes(f.size_allocated),
          tier: f.tier,
          tier_label: f.tier_label,
          class_label: f.class_label,
          reasons: f.reasons.concat(["refused: " + f.tier_label.toLowerCase()]),
          stale: stale,
        });
        continue;
      }
      items.push({
        path: f.path,
        name: f.name,
        size_allocated: stale ? 0 : f.size_allocated,
        size_label: humanBytes(stale ? 0 : f.size_allocated),
        tier: f.tier,
        tier_label: f.tier_label,
        class_label: f.class_label,
        reasons: f.reasons,
        stale: stale,
      });
    }
    const estimated = items.reduce((s, i) => s + i.size_allocated, 0);
    const careful = items.filter((i) => i.tier === "review_carefully").length;
    let consequence =
      "Nothing in this selection can be removed. See the refused list for why.";
    if (items.length) {
      consequence =
        "Removes " +
        items.length +
        (items.length === 1 ? " file" : " files") +
        " permanently, about " +
        humanBytes(estimated) +
        ". This is a direct delete: there is no quarantine copy and no undo.";
      if (careful) {
        consequence +=
          " " + careful + " of them are user files that were selected individually.";
      }
    }
    const plan = {
      scan_id: mockStorageScanId,
      plan_id: "plan-mock-" + items.length + "-" + refused.length,
      items: items,
      refused: refused,
      estimated_bytes: estimated,
      estimated_label: humanBytes(estimated),
      certainty: "measured",
      certainty_label: "Measured",
      consequence: consequence,
      confirmation_phrase: "REMOVE",
    };
    mockApprovedPlan = plan;
    return Promise.resolve({
      scan_id: mockStorageScanId,
      plan_id: "plan-mock-" + items.length + "-" + refused.length,
      items: items,
      refused: refused,
      estimated_bytes: estimated,
      estimated_label: humanBytes(estimated),
      certainty: "measured",
      certainty_label: "Measured",
      consequence: consequence,
      confirmation_phrase: "REMOVE",
    });
  }

  function storageExecuteMock(args) {
    /* The backend keeps the approved plan. The UI sends back the digest of
       the plan it approved and the phrase that approves it; it cannot ask
       for a different selection. */
    const plan = mockApprovedPlan;
    if (!plan) {
      return Promise.reject(
        new Error("There is no approved plan. Review the removal preview first."),
      );
    }
    if (args && args.planId && args.planId !== plan.plan_id) {
      return Promise.reject(
        new Error(
          "That approval is for a different plan. Review the removal preview again.",
        ),
      );
    }
    if (args && args.confirmPhrase !== "REMOVE") {
      return Promise.reject(new Error("Type REMOVE to confirm."));
    }
    /* Real removal takes measurable time. Returning instantly would hide the
       working pose and make it impossible to observe that nothing is
       fabricated while an operation is in flight. */
    return delay(window.__CURE_MOCK_STORAGE_EXEC_MS).then(function () {
      return finishStorageExecute(args, plan);
    });
  }

  function finishStorageExecute(args, plan) {
    const files = mockScanFiles();
    window.__CURE_LAST_STORAGE_CALL = JSON.parse(JSON.stringify(args || {}));
    const deleted = [];
    const refused = [];
    const failures = [];
    let bytes = 0;
    let estimated = 0;

    const items = plan.items || [];
    for (const item of items) {
      const p = item.path;
      const f = files.find((x) => x.path.toLowerCase() === String(p).toLowerCase());
      if (!f || f.tier === "protected" || f.tier === "unknown") {
        refused.push({
          path: p,
          name: String(p).split(/[\\/]/).pop(),
          size_allocated: 0,
          size_label: "0 B",
          tier: "unknown",
          tier_label: "Unknown",
          class_label: "Other",
          reasons: ["refused: not deletable"],
          stale: true,
        });
        continue;
      }
      if (window.__CURE_MOCK_STORAGE_LOCKED && failures.length === 0 &&
        f.name.indexOf("CBS.log") >= 0) {
        failures.push({
          path: f.path,
          reason: "locked by another process",
        });
        estimated += f.size_allocated;
        continue;
      }
      deleted.push(f.path);
      bytes += f.size_allocated;
      estimated += f.size_allocated;
    }

    const freeBefore = 88 * 1024 * MB;
    const freeAfter = freeBefore + bytes;
    return {
      scan_id: mockStorageScanId,
      plan_id: plan.plan_id,
      planned: items.length,
      attempted: deleted.length + failures.length,
      deleted: deleted.length,
      refused: refused.length,
      failed: failures.length,
      bytes_removed_estimate: bytes,
      bytes_removed_label: humanBytes(bytes),
      bytes_uncertain: 0,
      bytes_uncertain_label: "0 B",
      bytes_estimated: estimated,
      bytes_estimated_label: humanBytes(estimated),
      finished_unix: MOCK_NOW,
      volumes: [
        {
          root: "C:\\",
          free_before: freeBefore,
          free_after: freeAfter,
          delta: bytes,
          delta_label: "+" + humanBytes(bytes) + " free",
          files_removed: deleted.length,
        },
      ],
      note:
        deleted.length +
        " file(s) confirmed removed. The " +
        humanBytes(bytes) +
        " figure is what those files occupied on disk before they were deleted. " +
        "The volume's free-space change is a separate observation of the whole drive; " +
        "other software writing at the same time affects it.",
      failures: failures,
      refused_items: refused,
    };
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
          evidence: [
            { level: "Observed", detail: "Unsigned binary" },
            { level: "Observed", detail: "Random name" },
          ],
        },
        {
          name: "unknown_tool.exe",
          pid: 8080,
          exe_path: "C:\\Users\\test\\Downloads\\unknown_tool.exe",
          score: 41,
          risk: "HighRisk",
          evidence: [
            { level: "Observed", detail: "Profile exe" },
            { level: "Observed", detail: "Unsigned Binary" },
          ],
        },
        {
          name: "helper.dll",
          pid: 11234,
          exe_path: "C:\\ProgramData\\helper.dll",
          score: 22,
          risk: "Suspicious",
          evidence: [{ level: "Observed", detail: "Unsigned Binary" }],
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

  const MOCK_AGENT_WORKSPACE = "C:\\Projects\\cure-demo-app";

  function mockAgentSession() {
    const now = Math.floor(Date.now() / 1000);
    const events = [
      {
        seq: 1,
        at_unix: now - 84,
        kind: "created",
        path: "src/lib/agent-radar.ts",
        from_path: null,
        size_before: null,
        size_after: 4820,
        source: "directory_watcher",
        confidence: "correlated",
        agents: ["opencode"],
        note: "Filesystem change observed while a supported agent process was present. Correlation is not attribution.",
      },
      {
        seq: 2,
        at_unix: now - 67,
        kind: "modified",
        path: "src/App.tsx",
        from_path: null,
        size_before: 6144,
        size_after: 7188,
        source: "directory_watcher",
        confidence: "correlated",
        agents: ["opencode"],
        note: "Filesystem change observed while a supported agent process was present. Correlation is not attribution.",
      },
      {
        seq: 3,
        at_unix: now - 51,
        kind: "modified",
        path: "package.json",
        from_path: null,
        size_before: 1890,
        size_after: 1962,
        source: "directory_watcher",
        confidence: "correlated",
        agents: ["opencode"],
        note: "Filesystem change observed while a supported agent process was present. Correlation is not attribution.",
      },
      {
        seq: 4,
        at_unix: now - 38,
        kind: "renamed",
        path: "docs/agent-notes.md",
        from_path: "docs/notes.md",
        size_before: 2048,
        size_after: 2048,
        source: "directory_watcher",
        confidence: "correlated",
        agents: ["opencode"],
        note: "Rename was observed from the watcher's old/new pair. Correlation is not attribution.",
      },
      {
        seq: 5,
        at_unix: now - 24,
        kind: "removed",
        path: "src/legacy/panel.tsx",
        from_path: null,
        size_before: 3911,
        size_after: null,
        source: "directory_watcher",
        confidence: "observed",
        agents: [],
        note: "Filesystem change observed after the agent process sample no longer contained a supported agent.",
      },
      {
        seq: 6,
        at_unix: now - 9,
        kind: "modified",
        path: ".gitignore",
        from_path: null,
        size_before: 410,
        size_after: 438,
        source: "snapshot_reconciliation",
        confidence: "correlated",
        agents: ["opencode"],
        note: "Change recovered by final snapshot reconciliation. Correlation is not attribution.",
      },
    ];
    return {
      id: "session-demo-1",
      workspace: MOCK_AGENT_WORKSPACE,
      started_unix: now - 96,
      ended_unix: null,
      active: true,
      files_at_start: 284,
      events: events,
      agents_seen: ["opencode"],
      coverage: {
        baseline_captured: true,
        baseline_truncated: false,
        baseline_files: 284,
        unreadable_dirs: 0,
        skipped_links: 0,
        watcher_started: true,
        overflow_batches: 0,
        dropped_events: 0,
        snapshot_reconciled: false,
        complete: true,
        note: "",
      },
    };
  }

  const mockAgent = {
    session: null,
    last: null,
    windows: null,
    snapshotAt: null,
    snapshotEntries: 0,
  };

  function mockAgentRadar() {
    const opencodeProcess = {
      pid: 6214,
      name: "opencode.exe",
      exe_path: "C:\\Users\\demo\\AppData\\Local\\Programs\\opencode\\opencode.exe",
      started_unix: Math.floor(Date.now() / 1000) - 420,
      identity: "Verified",
      identity_label: "verified tool identity (canonical path / verified signer)",
    };
    const active = mockAgent.session && mockAgent.session.active;
    return {
      generated_unix: Math.floor(Date.now() / 1000),
      detected_count: 1,
      supported_agents: [
        {
          id: "opencode",
          label: "OpenCode",
          detected: true,
          process_count: 1,
          state: active ? "session_connected" : "idle",
          state_label: active ? "Present during session" : "Present / idle",
          state_note: active
            ? "The process was present in the latest sample. Presence does not prove code generation."
            : "The process is present but has not been correlated with a recorded change in this session.",
          integration_note:
            "Supported local integration: process image name only. No session API is consumed.",
          processes: [opencodeProcess],
        },
        {
          id: "codex_cli",
          label: "Codex CLI",
          detected: false,
          process_count: 0,
          state: "not_detected",
          state_label: "Not detected",
          state_note: "No process with a documented Codex CLI image name appeared in this sample.",
          integration_note:
            "Supported local integration: process image name only. No session API is consumed.",
          processes: [],
        },
      ],
      processes: [opencodeProcess],
      monitoring: active ? mockAgent.session : null,
      windows: mockAgent.windows,
      workspace: active ? MOCK_AGENT_WORKSPACE : mockAgent.last ? mockAgent.last.workspace : null,
      notes: [
        "A running process does not prove that an agent is generating code or executing tasks.",
        "Synthetic demo data. No process on this machine is inspected.",
      ],
    };
  }

  function mockAgentCaseFile(summary, includeSummary) {
    const session = mockAgent.last || mockAgent.session;
    if (!session) throw new Error("Run and stop a monitoring session before building a case file.");
    /* The demo note is intentionally plain: a note with path-like or
       credential-like text is withheld by the real sanitizer, and this
       sample only mirrors that boundary when it is obvious. */
    const looksPrivate = /(?:[A-Za-z]:\\|\\Users\\|@|token|password|secret)/i.test(summary || "");
    const note = includeSummary && summary && !looksPrivate ? String(summary).slice(0, 160) : null;
    const ends = session.ended_unix || Math.floor(Date.now() / 1000);
    const counts = session.events.reduce(
      function (acc, e) {
        acc.total += 1;
        acc[e.kind] = (acc[e.kind] || 0) + 1;
        return acc;
      },
      { total: 0 },
    );
    return {
      case_id: "8f2a1c7d",
      kind: "agent_session",
      title: "AGENT SESSION CASE",
      generated_unix: ends,
      agent_label: "OpenCode",
      agent_presence: "A supported agent process was observed during the session.",
      observation_seconds: Math.max(1, ends - session.started_unix),
      observation_label: Math.max(1, ends - session.started_unix) + "s",
      changes_total: counts.total,
      created: counts.created || 0,
      modified: counts.modified || 0,
      removed: counts.removed || 0,
      renamed: counts.renamed || 0,
      storage_delta_bytes: 4097,
      storage_delta_label: "+4.0 KB",
      storage_note: "Net metadata size change across observed events, not verified free space.",
      confidence_label: "Correlated, not attributed",
      confidence_note:
        "Changes happened while a supported agent process was present. Presence does not establish causation.",
      completeness: session.coverage.complete
        ? "Complete within recorder limits"
        : "Incomplete coverage",
      coverage_note: session.coverage.complete
        ? "Baseline and watcher coverage were complete within the recorder's documented limits."
        : session.coverage.note,
      observed_events: 1,
      correlated_events: 5,
      unknown_events: 0,
      windows_added: mockAgent.windows ? (mockAgent.windows.added || []).length : 0,
      windows_removed: mockAgent.windows ? (mockAgent.windows.removed || []).length : 0,
      windows_changed: mockAgent.windows ? (mockAgent.windows.changed || []).length : 0,
      windows_note: mockAgent.windows
        ? "Windows persistence differences are review leads, not malware verdicts."
        : "No Windows persistence comparison was included.",
      reconciliation_used: true,
      summary: note,
      summary_withheld: !!(includeSummary && summary && looksPrivate),
      demo: true,
    };
  }

  function mockWindowsItem(status, name, source, hint) {
    return {
      status: status,
      source: source,
      source_label:
        source === "registry_run"
          ? "Registry Run key"
          : source === "startup_folder"
            ? "Startup folder"
            : source === "scheduled_task"
              ? "Scheduled task"
              : "Windows service",
      name: name,
      location_before: null,
      location_after: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
      command_before: null,
      command_after: "C:\\Projects\\cure-demo-app\\tools\\preview.exe --agent-demo",
      review_hints: [hint],
      evidence: "Observed in the supported persistence inventory. This is a review lead.",
    };
  }


  
  window.__TAURI__ = {
    core: {
      invoke(command, args) {
        (window.__CURE_MOCK_COMMANDS || (window.__CURE_MOCK_COMMANDS = [])).push(
          command,
        );
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
          case "storage_available_roots":
            return Promise.resolve([
              {
                path: "C:\\",
                label: "Drive C",
                exists: true,
                is_drive: true,
                total_bytes: 512 * 1024 * 1024 * 1024,
                free_bytes: 88 * 1024 * 1024 * 1024,
                volume_label: "SampleVolume",
                selectable: true,
              },
              {
                path: "D:\\",
                label: "Drive D",
                exists: true,
                is_drive: true,
                total_bytes: 2048 * 1024 * 1024 * 1024,
                free_bytes: 1024 * 1024 * 1024 * 1024,
                volume_label: "BackupVolume",
                selectable: true,
              },
              {
                path: "C:\\Users\\bob",
                label: "Profile",
                exists: true,
                is_drive: false,
                total_bytes: 512 * 1024 * 1024 * 1024,
                free_bytes: 88 * 1024 * 1024 * 1024,
                volume_label: "SampleVolume",
                selectable: true,
              },
            ]);
          case "storage_scan":
            if (window.__CURE_MOCK_STORAGE_ERROR)
              return Promise.reject(
                new Error("The storage scan could not complete. Nothing was changed."),
              );
            window.__CURE_STORAGE_DONE = false;
            // Real progress events, matching the shape the engine emits, so
            // the UI's progress handling is exercised rather than skipped.
            //
            // The counters are derived from the sample data, so the figures
            // on screen while the scan runs are the same figures the scan
            // reports when it finishes. A demo that counted up to numbers
            // the result did not contain would be showing sample data that
            // does not exist.
            const sample = window.__CURE_MOCK_STORAGE_EMPTY ? [] : mockScanFiles();
            const sampleBytes = sample.reduce((s, f) => s + f.size_allocated, 0);
            const sampleDirs = sample.length * 3;
            const steps = 6;
            let step = 0;
            const tick = () => {
              step += 1;
              const walking = step <= 3;
              const done = step / steps;
              emit("storage-progress", {
                scan_id: "mock-scan-progress",
                phase: step >= steps ? "finished" : walking ? "walking" : "hashing",
                root_label: step % 2 === 0 ? "Recycle Bin" : "User temp",
                current_path: "C:\\Users\\bob\\AppData\\Local\\Temp\\f" + step,
                dirs_seen: walking ? Math.round(sampleDirs * done) : 0,
                files_seen: walking
                  ? Math.round(sample.length * done)
                  : Math.round(sample.length * done),
                bytes_seen: Math.round(sampleBytes * done),
                elapsed_ms: step * 120,
                eta_ms: step >= 2 ? (steps - step) * 120 : null,
              });
              if (step < steps) {
                setTimeout(tick, window.__CURE_MOCK_STORAGE_DELAY_MS / steps);
              }
            };
            tick();
            return delay(window.__CURE_MOCK_STORAGE_DELAY_MS).then(function () {
              window.__CURE_STORAGE_DONE = true;
              return buildMockStorageScan(args && args.depth);
            });
          case "storage_cancel":
            return Promise.resolve();
          case "storage_preview":
            return storagePreviewMock(args);
          case "storage_execute":
            return storageExecuteMock(args);
          case "storage_reveal":
            window.__CURE_LAST_REVEAL = args && args.path;
            return Promise.resolve(args && args.path);
          case "storage_case_file":
            return delay(60).then(function () {
              return buildMockCaseFile();
            });
          case "storage_save_case_file":
            /* Structural boundary, not a UI preference: the public demo
               has no code path that writes a file to disk. The dev harness
               gets a synthetic path so it can still exercise the request. */
            if (window.__CURE_FORCE_MOCK) {
              return Promise.reject(
                new Error(
                  "This is the public demo. Case files are preview-only and are never written.",
                ),
              );
            }
            return Promise.resolve(
              "C:\\mock\\case-files\\" + String((args && args.name) || "case") + ".png",
            );
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
                    evidence: [],
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
          case "agent_radar":
            return delay(120).then(function () {
              return mockAgentRadar();
            });
          case "flight_start": {
            const requested = String((args && args.workspace) || "").trim();
            if (!requested) {
              return Promise.reject(new Error("Choose an existing workspace directory to monitor."));
            }
            if (mockAgent.session && mockAgent.session.active) {
              return Promise.reject(
                new Error("A monitoring session is already active. Stop it before starting another."),
              );
            }
            const session = mockAgentSession();
            session.workspace = requested;
            mockAgent.session = session;
            mockAgent.last = session;
            setTimeout(function () {
              emit("flight-update", {});
            }, 250);
            return delay(320).then(function () {
              return session;
            });
          }
          case "flight_status":
            return delay(40).then(function () {
              return mockAgent.session || mockAgent.last;
            });
          case "flight_stop": {
            const session = mockAgent.session || mockAgent.last;
            if (!session) {
              return Promise.reject(new Error("No monitoring session is active."));
            }
            const ended = Math.floor(Date.now() / 1000);
            session.active = false;
            session.ended_unix = Math.max(ended, session.started_unix + 1);
            session.coverage.snapshot_reconciled = true;
            mockAgent.session = null;
            mockAgent.last = session;
            setTimeout(function () {
              emit("flight-update", {});
            }, 60);
            return delay(260).then(function () {
              return session;
            });
          }
          case "windows_change_capture": {
            const prior = mockAgent.snapshotAt;
            mockAgent.snapshotAt = Math.floor(Date.now() / 1000);
            mockAgent.snapshotEntries = 128;
            mockAgent.windows = null;
            return delay(280).then(function () {
              return {
                saved_at_unix: mockAgent.snapshotAt,
                entry_count: mockAgent.snapshotEntries,
                previous_saved_at_unix: prior,
                path: "C:\\mock\\data\\agents\\windows-snapshot.json",
                note: prior
                  ? "Trusted snapshot replaced. Future comparisons use this timestamp."
                  : "First trusted snapshot captured. Future comparisons use this timestamp.",
              };
            });
          }
          case "windows_change_compare": {
            if (!mockAgent.snapshotAt) {
              const empty = {
                compared_unix: Math.floor(Date.now() / 1000),
                snapshot_saved_at_unix: null,
                entries_before: 0,
                entries_after: 128,
                added: [],
                removed: [],
                changed: [],
                complete: false,
                note: "No trusted snapshot is stored. Capture a snapshot before making changes.",
              };
              mockAgent.windows = empty;
              return delay(160).then(function () {
                return empty;
              });
            }
            const added = [
              mockWindowsItem(
                "added",
                "CureDemoPreview",
                "registry_run",
                "New Run entry references a path outside Program Files. Review the executable before trusting it.",
              ),
            ];
            const changed = [
              mockWindowsItem(
                "changed",
                "AcmeAgentHelper",
                "startup_folder",
                "The launcher command changed. Verify the new target; a change alone is not a verdict.",
              ),
            ];
            const result = {
              compared_unix: Math.floor(Date.now() / 1000),
              snapshot_saved_at_unix: mockAgent.snapshotAt,
              entries_before: mockAgent.snapshotEntries,
              entries_after: 130,
              added: added,
              removed: [],
              changed: changed,
              complete: true,
              note: "Differences are review leads, not malware verdicts.",
            };
            mockAgent.windows = result;
            return delay(240).then(function () {
              return result;
            });
          }
          case "agent_case_file":
            return delay(120).then(function () {
              return mockAgentCaseFile(
                String((args && args.summary) || ""),
                !!(args && args.includeSummary),
              );
            });
                    case "export_agent_case_file":
            return delay(50).then(function () {
              return "C:\\Users\\demo\\.cure\\exports\\case-demo." + (args && args.format || "md");
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
