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

    return {
      scan_id: mockStorageScanId,
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
