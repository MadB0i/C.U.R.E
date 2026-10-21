/* ==========================================================================
   STORAGE WORKSTATION — measurement, review and confirmed cleanup.

   Design rules this module keeps:
   · Nothing is drawn or summed before the engine reports real numbers.
     Before a scan the headline is an em dash and the map is an empty well.
   · Every number carries its provenance. Tiers are shown next to sizes, the
     headline says "at least" whenever any location was unreadable, and the
     coverage tab exists so a partial scan can never read as a clean one.
   · Selection is per item. There is no "select all" for anything that is not
     Safe to review, and no path to a delete button that does not go through
     the preview dialog.
   ========================================================================== */
(function () {
  "use strict";

  const TAU = window.__TAURI__;
  const invoke = TAU && TAU.core ? TAU.core.invoke : null;
  const listen = TAU && TAU.event ? TAU.event.listen : null;

  const $ = (id) => document.getElementById(id);

  /* ---------------------------------------------------------------------
     Modal plumbing.

     A modal dialog has to do three things, and this screen's two dialogs
     previously did none of them: keep the keyboard inside the dialog, make
     the page behind it unreachable, and give the focus back to whatever
     opened it. Without the first, a keyboard user can tab straight out of
     the "type REMOVE" dialog and reach the page behind it; without the
     second, a screen reader can read the page behind it; without the third,
     focus is lost entirely when the dialog closes.
     --------------------------------------------------------------------- */

  const modalState = new WeakMap();

  function focusablesIn(root) {
    return Array.from(
      root.querySelectorAll(
        'a[href], button:not([disabled]), input:not([disabled]), ' +
          'select:not([disabled]), textarea:not([disabled]), ' +
          '[tabindex]:not([tabindex="-1"])',
      ),
    ).filter((n) => n.getClientRects().length > 0);
  }

  /* The elements that make up the page *behind* a dialog.
     The dialog lives inside the app shell, so the whole shell cannot simply
     be made inert — that would make the dialog itself inert. Instead, every
     element on the path from the dialog to the document root contributes its
     other children. */
  function backgroundOf(overlay) {
    const bg = [];
    let node = overlay;
    while (node && node.parentElement) {
      const parent = node.parentElement;
      for (const child of parent.children) {
        if (child !== node && !child.contains(overlay)) bg.push(child);
      }
      node = parent;
    }
    return bg;
  }

  function openModal(overlay, options) {
    if (!overlay) return;
    const opts = options || {};
    const background = backgroundOf(overlay);
    const priorInert = background.map((n) => n.inert);
    background.forEach((n) => (n.inert = true));
    overlay.classList.remove("hidden");
    const restore = document.activeElement;

    const onKey = (ev) => {
      if (ev.key === "Escape") {
        ev.stopPropagation();
        ev.preventDefault();
        closeModal(overlay);
        return;
      }
      if (ev.key !== "Tab") return;
      const items = focusablesIn(overlay);
      if (!items.length) {
        ev.preventDefault();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      const active = document.activeElement;
      if (ev.shiftKey) {
        if (active === first || !overlay.contains(active)) {
          ev.preventDefault();
          last.focus();
        }
      } else if (active === last || !overlay.contains(active)) {
        ev.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKey, true);
    modalState.set(overlay, { onKey, background, priorInert, restore });
    if (opts.initial && opts.initial.focus) {
      setTimeout(() => opts.initial.focus(), 0);
    }
  }

  function closeModal(overlay) {
    if (!overlay) return;
    const s = modalState.get(overlay);
    overlay.classList.add("hidden");
    if (!s) return;
    modalState.delete(overlay);
    document.removeEventListener("keydown", s.onKey, true);
    s.background.forEach((n, i) => (n.inert = s.priorInert[i]));
    if (s.restore && s.restore.focus && document.contains(s.restore)) {
      s.restore.focus();
    }
  }


  /* LUMA's storage persona. Scoped to the storage host so a rescue finding
     never leaves her posing about a screen she is not on.

     Every state the workstation can ask for has a route here. A missing
     one is not a cosmetic gap: the failure path calls `failed()`, and a
     missing method there threw and left the screen stuck in the scanning
     pose with the engine's own error swallowed. */
  const luma = {
    host: null,
    get api() {
      if (!this.host && window.CureCompanion) {
        /* Claimed eagerly, not lazily: a host that has its own persona
           must never be overwritten by a global event that arrives first. */
        this.host = window.CureCompanion.onHost("storage");
      }
      return this.host;
    },
    idle: (detail) => luma.api && luma.api.idle(detail),
    scan: (detail) => luma.api && luma.api.scan("scan", detail),
    review: (count, detail) => luma.api && luma.api.review(count, detail),
    discovering: (count, detail) => luma.api && luma.api.discovering(count, detail),
    curious: (detail) => luma.api && luma.api.curious(detail),
    thinking: (detail) => luma.api && luma.api.thinking(detail),
    failed: (detail) => luma.api && luma.api.failed(detail),
    cancelled: (detail) => luma.api && luma.api.cancelled(detail),
    seal: (detail) => luma.api && luma.api.seal(detail),
    state: (name, title, detail) => luma.api && luma.api.cleanup(name, title, detail),
  };

  /* The phrase the engine requires. It is stated by the backend in the
     plan, so the UI never hard-codes a second copy of the rule. */
  function confirmPhrase() {
    const stated = state.lastPlan && state.lastPlan.confirmation_phrase;
    return (stated || "REMOVE").toUpperCase();
  }

  const el = {
    subline: $("sw-subline"),
    statusPill: $("sw-status-pill"),
    statusText: $("sw-status-text"),
    depthNote: $("sw-depth-note"),
    addRoot: $("sw-add-root"),
    scan: $("sw-scan"),
    cancel: $("sw-cancel"),
    rootsBar: $("sw-roots-bar"),
    progress: $("sw-progress"),
    progressPhase: $("sw-progress-phase"),
    progressDetail: $("sw-progress-detail"),
    progressEta: $("sw-progress-eta"),
    progressStages: $("sw-scan-stages"),
    countFiles: $("sw-count-files"),
    countDirs: $("sw-count-dirs"),
    countBytes: $("sw-count-bytes"),
    labelFiles: $("sw-label-files"),
    labelDirs: $("sw-label-dirs"),
    labelBytes: $("sw-label-bytes"),
    treemapWrap: document.querySelector(".sw-treemap-wrap"),
    headlineCard: $("sw-headline-card"),
    crumbs: $("sw-breadcrumbs"),
    mapTip: $("sw-map-tip"),
    mapBadge: $("sw-map-badge"),
    amount: $("sw-amount"),
    amountLabel: $("sw-amount-label"),
    amountNote: $("sw-amount-note"),
    careful: $("sw-careful"),
    protectedBytes: $("sw-protected"),
    free: $("sw-free"),
    treemap: $("sw-treemap"),
    canvas: $("sw-treemap-canvas"),
    treemapHint: $("sw-treemap-hint"),
    tierFilter: $("sw-tier-filter"),
    classes: $("sw-classes"),
    largeSort: $("sw-large-sort"),
    largeCount: $("sw-large-count"),
    largeBody: $("sw-large-body"),
    foldersBody: $("sw-folders-body"),
    coverageEmpty: $("sw-coverage-empty"),
    coverageScan: $("sw-coverage-scan"),
    dupeCoverage: $("sw-dupe-coverage"),
    dupeList: $("sw-dupe-list"),
    queueGroups: $("sw-queue-groups"),
    queueEmpty: $("sw-queue-empty"),
    queueBadge: $("sw-queue-badge"),
    previewBtn: $("sw-preview-btn"),
    queueClear: $("sw-queue-clear"),
    statRows: $("sw-stat-rows"),
    rootsBody: $("sw-roots-body"),
    failures: $("sw-failures"),
    revealRoot: $("sw-reveal-root"),
    inspector: $("sw-inspector"),
    inspectorTitle: $("sw-inspector-title"),
    inspectorBody: $("sw-inspector-body"),
    inspectorClose: $("sw-inspector-close"),
    previewOverlay: $("sw-preview-overlay"),
    previewConsequence: $("sw-preview-consequence"),
    previewList: $("sw-preview-list"),
    previewRefused: $("sw-preview-refused"),
    confirmInput: $("sw-confirm-input"),
    previewPhrase: $("sw-preview-phrase"),
    previewCancel: $("sw-preview-cancel"),
    previewConfirm: $("sw-preview-confirm"),
    receiptOverlay: $("sw-receipt-overlay"),
    receiptAmount: $("sw-receipt-amount"),
    receiptLabel: $("sw-receipt-label"),
    receiptRows: $("sw-receipt-rows"),
    receiptNotes: $("sw-receipt-notes"),
    receiptClose: $("sw-receipt-close"),
  };

  const DEPTH_NOTES = {
    quick:
      "Known cache, temporary and diagnostic locations. Fast, and rarely noisy.",
    deep: "Also inventories browser profiles, application data and developer caches. Slower, and reports far more that will not be recommended.",
  };

  const TIER_ORDER = ["safe_to_review", "review_carefully", "protected", "unknown"];
  const TIER_LABEL = {
    safe_to_review: "Safe to review",
    review_carefully: "Review carefully",
    protected: "Protected",
    unknown: "Unknown",
  };

  const state = {
    scan: null,
    depth: "quick",
    extraRoots: [],
    running: false,
    cancelled: false,
    tab: "map",
    tierFilter: "all",
    /* selection is a path set; the queue is derived from it */
    selected: new Set(),
    queue: new Map(),
    zoom: null,
    treemapCells: [],
    drill: null,
    lastPlan: null,
    progressTimer: null,
    /* true while the duplicate pass is reading bytes */
    thinking: false,
    /* what the walk measured so far, kept so the summary can keep showing
       it while the duplicate pass reports different counters */
    measured: { files: 0, dirs: 0, bytes: 0 },
  };

  // ---------------------------------------------------------------------
  // formatting
  // ---------------------------------------------------------------------

  function fmtBytes(n) {
    if (n === null || n === undefined) return "—";
    if (n >= 1099511627776) return (n / 1099511627776).toFixed(2) + " TB";
    if (n >= 1073741824) return (n / 1073741824).toFixed(2) + " GB";
    if (n >= 1048576) return (n / 1048576).toFixed(1) + " MB";
    if (n >= 1024) return Math.round(n / 1024) + " KB";
    return n + " B";
  }

  function fmtDate(unix) {
    if (!unix) return "unknown";
    const d = new Date(unix * 1000);
    if (isNaN(d.getTime())) return "unknown";
    return (
      d.toLocaleDateString(undefined, { year: "numeric", month: "short" }) +
      " " +
      d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
    );
  }

  function ageDays(unix) {
    if (!unix) return null;
    return Math.max(0, Math.floor((Date.now() / 1000 - unix) / 86400));
  }

  function fmtDuration(ms) {
    if (!ms && ms !== 0) return "—";
    if (ms < 1000) return ms + " ms";
    const s = ms / 1000;
    if (s < 60) return s.toFixed(1) + " s";
    const m = Math.floor(s / 60);
    return m + "m " + Math.round(s - m * 60) + "s";
  }

  /* Every string that originates in the engine reaches the DOM as text.
     innerHTML is used only with markup this file itself builds. */
  function node(tag, cls, text) {
    const n = document.createElement(tag);
    if (cls) n.className = cls;
    if (text !== undefined && text !== null) n.textContent = String(text);
    return n;
  }

  /* A bulleted list. The explicit role is needed because the markers are
     suppressed in CSS, which costs the element its implicit list semantics
     in some assistive technology. */
  function list(cls) {
    const n = document.createElement("ul");
    n.className = cls;
    n.setAttribute("role", "list");
    return n;
  }

  function reasonsList(reasons) {
    const ul = list("sw-reasons");
    for (const r of reasons || []) ul.appendChild(node("li", null, r));
    return ul;
  }

  function setStatus(text, kind) {
    if (!el.statusText) return;
    el.statusText.textContent = text;
    if (el.statusPill) {
      el.statusPill.classList.remove("is-ok", "is-warn", "is-busy", "is-err");
      if (kind) el.statusPill.classList.add(kind);
    }
  }

  // ---------------------------------------------------------------------
  // scan lifecycle
  // ---------------------------------------------------------------------

  function resetToIdle(message) {
    if (el.headlineCard) el.headlineCard.removeAttribute("data-scanning");
    state.scan = null;
    state.selected.clear();
    state.queue.clear();
    state.zoom = null;
    state.lastPlan = null;
    setStatus("Ready", null);
    if (el.subline) el.subline.textContent = message || "Nothing is measured until you run a scan.";
    if (el.amount) el.amount.textContent = "—";
    if (el.amountLabel) el.amountLabel.textContent = "Not measured";
    if (el.amountNote) {
      el.amountNote.textContent =
        "Run a scan to measure this machine. C.U.R.E reports only bytes it has actually measured.";
    }
    if (el.careful) el.careful.textContent = "—";
    if (el.protectedBytes) el.protectedBytes.textContent = "—";
    if (el.free) el.free.textContent = "—";
    if (el.classes) el.classes.replaceChildren();
    if (el.largeBody) el.largeBody.replaceChildren();
    if (el.largeCount) el.largeCount.textContent = "";
    if (el.dupeList) el.dupeList.replaceChildren();
    if (el.dupeCoverage) {
      el.dupeCoverage.textContent =
        "Duplicates are only reported after every copy has been read in full and hashed. Nothing is proposed on a size match alone.";
    }
    if (el.queueGroups) el.queueGroups.replaceChildren();
    if (el.queueEmpty) el.queueEmpty.classList.remove("hidden");
    if (el.queueBadge) el.queueBadge.textContent = "0";
    if (el.statRows) el.statRows.replaceChildren();
    if (el.rootsBody) el.rootsBody.replaceChildren();
    if (el.failures) el.failures.replaceChildren();
    if (el.previewBtn) el.previewBtn.disabled = true;
    if (el.inspector) el.inspector.classList.add("hidden");
    if (el.amount) el.amount.textContent = "—";
    drawTreemap();
    renderQueue();
  }

  /* The stages the engine can actually be in. There is no stage here that
     the backend cannot report, and no percentage that is not measured. */
  const SCAN_STAGE_ORDER = ["prepare", "walk", "hash", "final"];
  const SCAN_STAGE_LABEL = {
    prepare: "Preparing",
    walk: "Scanning locations",
    hash: "Verifying duplicates",
    final: "Finalizing results",
  };

  /* Map the engine's progress phase onto a stage. Unknown phases keep the
     last known stage rather than inventing one. */
  function stageForPhase(phase) {
    switch (phase) {
      case "hashing":
        return "hash";
      case "correlating":
      case "finished":
        return "final";
      case "cancelled":
        return "final";
      case "walking":
        return "walk";
      default:
        return null;
    }
  }

  function paintScanStages(active, outcome) {
    if (!el.progressStages) return;
    const items = Array.from(el.progressStages.querySelectorAll("li"));
    const activeIndex = Math.max(0, SCAN_STAGE_ORDER.indexOf(active));
    items.forEach((item, index) => {
      let state = "pending";
      if (index < activeIndex) state = "done";
      else if (index === activeIndex) {
        state = outcome === "cancelled" || outcome === "failed" ? outcome : "active";
      }
      item.setAttribute("data-state", state);
    });
  }

  /* While a scan is running the results summary must not claim that
     nothing has been measured: the engine is measuring, and it is reporting
     what it has measured so far. The reclaimable total is not known until
     the scan finishes, and the summary says so rather than guessing.

     The duplicate pass reports different counters from the walk — files
     hashed rather than files measured — so the summary keeps showing the
     walk's own figures and says what the pass is doing. Putting the hash
     counters in the headline would have made the number change meaning
     halfway through the scan. */
  function renderMeasuring(payload) {
    if (!payload) return;
    const phase = payload.phase || "walking";
    const hashing = phase === "hashing";
    if (hashing) {
      const measured = state.measured;
      if (el.headlineCard) el.headlineCard.setAttribute("data-scanning", "1");
      if (el.amount) el.amount.textContent = fmtBytes(measured.bytes);
      if (el.amountLabel) el.amountLabel.textContent = "Measured so far";
      if (el.amountNote) {
        el.amountNote.textContent =
          measured.files.toLocaleString() +
          (measured.files === 1 ? " file" : " files") +
          " measured. Duplicates are now being verified by reading every copy " +
          "in full and comparing SHA-256 digests; the reclaimable total is " +
          "worked out when the pass finishes.";
      }
      if (el.careful) el.careful.textContent = "—";
      if (el.protectedBytes) el.protectedBytes.textContent = "—";
      if (el.free) el.free.textContent = "—";
      return;
    }
    const files = payload.files_seen || 0;
    const folders = payload.dirs_seen || 0;
    const bytes = payload.bytes_seen || 0;
    state.measured = { files: files, dirs: folders, bytes: bytes };
    if (el.headlineCard) el.headlineCard.setAttribute("data-scanning", "1");
    if (el.amount) el.amount.textContent = fmtBytes(bytes);
    if (el.amountLabel) el.amountLabel.textContent = "Measured so far";
    if (el.amountNote) {
      el.amountNote.textContent =
        files.toLocaleString() +
        (files === 1 ? " file" : " files") +
        " and " +
        folders.toLocaleString() +
        (folders === 1 ? " folder" : " folders") +
        " measured. The reclaimable total is worked out when the scan finishes.";
    }
    if (el.careful) el.careful.textContent = "—";
    if (el.protectedBytes) el.protectedBytes.textContent = "—";
    if (el.free) el.free.textContent = "—";
  }

  function updateScanProgress(payload) {
    if (!payload) return;
    const phase = payload.phase || "walking";
    renderMeasuring(payload);
    const stage = stageForPhase(phase);
    if (stage) {
      paintScanStages(stage, phase === "cancelled" ? "cancelled" : null);
      if (el.progressPhase) {
        el.progressPhase.textContent =
          phase === "cancelled" ? "Cancelled" : SCAN_STAGE_LABEL[stage];
      }
    }
    /* The current location is the catalogue's own label for the root being
       walked. The full file path is deliberately not shown: it carries the
       user's name, and a label is what the operator needs. */
    if (el.progressDetail) {
      el.progressDetail.textContent = payload.root_label || "";
    }
    if (el.countFiles && el.countFiles.parentElement) {
      el.countFiles.parentElement.parentElement.setAttribute(
        "data-phase",
        phase === "hashing" ? "hash" : "walk",
      );
    }
    /* The counters name what they are counting. The duplicate pass counts
       files it has read in full, not files it has measured, and saying
       otherwise would be a small lie in the middle of the scan. */
    if (el.labelFiles) el.labelFiles.textContent = phase === "hashing" ? "Files hashed" : "Files";
    if (el.labelDirs) el.labelDirs.textContent = phase === "hashing" ? "Folders" : "Folders";
    if (el.labelBytes) el.labelBytes.textContent = phase === "hashing" ? "Bytes read" : "Measured";
    if (el.countFiles) {
      el.countFiles.textContent = (payload.files_seen || 0).toLocaleString();
    }
    if (el.countDirs) {
      el.countDirs.textContent = (payload.dirs_seen || 0).toLocaleString();
    }
    if (el.countBytes) {
      el.countBytes.textContent = fmtBytes(payload.bytes_seen || 0);
    }
    if (el.progressEta) {
      /* The engine only offers an estimate once it has measured more than
         one root, so there is no guess to show before that. */
      el.progressEta.textContent = payload.eta_ms
        ? "about " + fmtDuration(payload.eta_ms) + " left"
        : "";
    }
  }

  function setRunning(running) {
    state.running = running;
    if (el.scan) el.scan.disabled = running;
    if (el.cancel) el.cancel.classList.toggle("hidden", !running);
    if (el.progress) el.progress.classList.toggle("hidden", !running);
    if (el.addRoot) el.addRoot.disabled = running;
  }

  async function runScan() {
    if (state.running || !invoke) return;
    state.running = true;
    state.cancelled = false;
    setRunning(true);
    setStatus("Scanning", "is-busy");
    luma.scan("Measuring storage");
    if (el.subline) el.subline.textContent = "Walking the catalogue roots…";
    /* The topbar says a storage measurement is in flight, so the kind of
       scan on screen is never ambiguous. */
    if (window.CureTopbar) window.CureTopbar.storageScanStarted();
    /* The summary enters its measuring state before the first progress
       event arrives, not after it. Between "Run scan" and the first event
       there is a moment where the engine has started and the screen still
       says "NOT MEASURED — Run a scan to measure this machine", which is
       both wrong and visibly so. */
    renderMeasuring({
      phase: "prepare",
      files_seen: 0,
      dirs_seen: 0,
      bytes_seen: 0,
      root_label: "",
    });
    if (el.progressPhase) el.progressPhase.textContent = SCAN_STAGE_LABEL.prepare;
    if (el.progressDetail) el.progressDetail.textContent = "";
    if (el.progressEta) el.progressEta.textContent = "";
    if (el.countFiles) el.countFiles.textContent = "0";
    if (el.countDirs) el.countDirs.textContent = "0";
    if (el.countBytes) el.countBytes.textContent = fmtBytes(0);
    paintScanStages("prepare", false);
    /* The previous measurement no longer describes the machine: a new scan
       is a new measurement, and the old numbers are not carried forward. */
    state.scan = null;
    state.drill = null;
    state.treemapCells = [];

    try {
      const scan = await invoke("storage_scan", {
        depth: state.depth,
        roots: state.extraRoots.slice(),
        findDuplicates: true,
        findAbandoned: state.depth === "deep",
      });
      state.scan = scan;
      state.drill = null;
      renderBreadcrumbs();
      state.selected.clear();
      state.queue.clear();
      state.zoom = null;
      if (el.headlineCard) el.headlineCard.removeAttribute("data-scanning");
      if (scan && scan.headline) {
        const h = scan.headline;
        if (el.amount) el.amount.textContent = h.safe_bytes_label;
        if (el.amountLabel) el.amountLabel.textContent = h.certainty_label;
        if (el.amountNote) {
          el.amountNote.textContent = h.incomplete
            ? "At least this much is recoverable. Some locations could not be read, so this is a floor rather than a total — the Coverage tab lists them."
            : "Measured end to end across every root this scan covered.";
        }
        if (el.careful) el.careful.textContent = h.careful_bytes_label;
        if (el.protectedBytes) el.protectedBytes.textContent = fmtBytes(scan.protected_bytes);
        if (el.free) {
          el.free.textContent = scan.coverage && scan.coverage.volume_free_bytes
            ? fmtBytes(scan.coverage.volume_free_bytes)
            : "unknown";
        }
        if (el.subline) {
          el.subline.textContent =
            (scan.coverage ? scan.coverage.completeness : "") +
            " · " +
            (scan.coverage ? scan.coverage.files_seen : 0) +
            " files in " +
            fmtDuration(scan.duration_ms) +
            ".";
        }
      }
      if (scan && scan.coverage && scan.coverage.cancelled) {
        setStatus("Cancelled", "is-warn");
        paintScanStages("final", "cancelled");
        if (el.progress) el.progress.setAttribute("data-cancelled", "1");
        if (el.amountLabel) el.amountLabel.textContent = "Partial — scan stopped";
        if (el.amountNote) {
          el.amountNote.textContent =
            "The scan was stopped before it finished, so these totals cover only the locations that were read. Nothing was removed.";
        }
        luma.state("cancelled", "Scan stopped", "Partial results are shown; nothing was removed.");
      } else {
        setStatus("Measured", "is-ok");
        paintScanStages("final", null);
        /* LUMA reacts to the real outcome: a measured file count, or an
           honest "nothing here" when the machine genuinely has nothing. */
        const found = scan && scan.coverage ? scan.coverage.files_seen : 0;
        if (found > 0) {
          luma.state(
            "discovering",
            found.toLocaleString() +
              (found === 1 ? " file measured" : " files measured"),
            scan.headline.incomplete
              ? "Some locations could not be read, so the totals are a floor."
              : "Measured across every root this scan covered.",
          );
        } else {
          luma.idle("Measured — this machine reported no files in those locations.");
        }
      }
      publishScanToTopbar(scan);
      renderAll();
      revealTreemap();
    } catch (err) {
      setStatus("Failed", "is-err");
      paintScanStages("final", "failed");
      if (el.progress) el.progress.setAttribute("data-failed", "1");
      if (el.progressPhase) el.progressPhase.textContent = "Failed";
      if (el.progressDetail) el.progressDetail.textContent = clean(err);
      luma.failed("The scan did not complete: " + clean(err));
      if (el.subline) {
        el.subline.textContent = "The scan did not complete: " + clean(err);
      }
      /* A failed scan leaves nothing measured. The summary must say so
         rather than keeping the live figures of the attempt that broke —
         and the panels must not keep showing the previous scan's numbers
         as though they were current. */
      if (el.headlineCard) el.headlineCard.removeAttribute("data-scanning");
      if (el.amount) el.amount.textContent = "—";
      if (el.amountLabel) el.amountLabel.textContent = "Not measured";
      if (el.amountNote) {
        el.amountNote.textContent =
          "The scan did not complete, so nothing is measured. " + clean(err);
      }
      if (el.careful) el.careful.textContent = "—";
      if (el.protectedBytes) el.protectedBytes.textContent = "—";
      if (el.free) el.free.textContent = "—";
      state.scan = null;
      state.drill = null;
      state.zoom = null;
      renderBreadcrumbs();
      renderAll();
    } finally {
      setRunning(false);
      /* The topbar stops saying "measuring" whether the scan finished,
         was stopped or broke. */
      if (window.CureTopbar) window.CureTopbar.storageScanFinished();
    }
  }

  function clean(err) {
    if (!err) return "unknown error";
    if (typeof err === "string") return err;
    if (err.message) return String(err.message);
    return String(err);
  }

  async function cancelScan() {
    if (!invoke || !state.running) return;
    state.cancelled = true;
    setStatus("Stopping…", "is-warn");
    /* The animation stops now, not when the engine reports back. */
    if (el.progress) el.progress.setAttribute("data-cancelled", "1");
    paintScanStages("final", "cancelled");
    if (el.progressPhase) el.progressPhase.textContent = "Stopping…";
    /* The summary says what is true while the engine winds down: the
       figures so far are real, and they are not the whole machine. */
    if (el.amountLabel) el.amountLabel.textContent = "Stopping — figures so far";
    if (el.amountNote) {
      el.amountNote.textContent =
        "The scan was asked to stop. The figures above are the locations read so far, not the whole machine.";
    }
    try {
      await invoke("storage_cancel", {});
    } catch (_) {
      /* the flag is best effort; the scan reports its own cancellation */
    }
  }

  // ---------------------------------------------------------------------
  // rendering
  // ---------------------------------------------------------------------

  function renderAll() {
    renderTierFilter();
    renderClasses();
    renderLarge();
    renderFolders();
    renderDuplicates();
    renderQueue();
    renderCoverage();
    renderCoverageBadge();
    renderBreadcrumbs();
    drawTreemap();
  }

  function renderTierFilter() {
    if (!el.tierFilter || !state.scan) return;
    el.tierFilter.replaceChildren();
    /* The counts come from the whole measurement, not from the bounded
       list the interface was given, so the numbers never understate what
       was seen. */
    const tc = state.scan.tier_counts || {};
    const counts = {
      all: state.scan.files_total || 0,
      safe_to_review: tc.safe_to_review || 0,
      review_carefully: tc.review_carefully || 0,
      protected: tc.protected || 0,
      unknown: tc.unknown || 0,
    };
    const keys = ["all"].concat(TIER_ORDER);
    for (const key of keys) {
      if (key !== "all" && !counts[key]) continue;
      const li = node("li");
      const b = node("button", "sw-tier" + (state.tierFilter === key ? " is-on" : ""), key === "all" ? "All" : TIER_LABEL[key]);
      b.type = "button";
      b.setAttribute("aria-pressed", state.tierFilter === key ? "true" : "false");
      b.dataset.tier = key;
      b.appendChild(node("span", "sw-tier-n", counts[key]));
      li.appendChild(b);
      el.tierFilter.appendChild(li);
    }
  }

  function renderFolders() {
    if (!el.foldersBody || !state.scan) return;
    el.foldersBody.replaceChildren();
    const folders = (state.scan.large_folders || []).slice(0, 60);
    if (!folders.length) {
      const tr = node("tr");
      const td = node("td", "sw-empty", "No folders were measured.");
      td.colSpan = 4;
      tr.appendChild(td);
      el.foldersBody.appendChild(tr);
      return;
    }
    for (const f of folders) {
      const tr = node("tr");
      const name = node("td");
      /* The folder name opens the map at that folder, so the map is not a
         mouse-only surface: the same drill is reachable from the keyboard
         through this table. */
      const open = node("button", "sw-file-name", f.name || f.path);
      open.type = "button";
      open.addEventListener("click", () => drillToFolder(f));
      name.appendChild(open);
      name.appendChild(node("span", "dim", f.path));
      tr.appendChild(name);
      tr.appendChild(node("td", null, f.class || "other"));
      tr.appendChild(node("td", "num", (f.file_count || 0).toLocaleString()));
      tr.appendChild(node("td", "num", fmtBytes(f.size_allocated || 0)));
      el.foldersBody.appendChild(tr);
    }
  }

  /* Open the map at a folder, whatever level the map is on. The map tab is
     brought forward with it: the operator asked where the space went, and
     the answer is the map. */
  function drillToFolder(folder) {
    const classKey = folder.class;
    const all = (state.scan && state.scan.folders) || [];
    const owners = all.filter(
      (f) => f.path.toLowerCase() === (folder.path || "").toLowerCase(),
    );
    const label =
      (state.scan.summaries.find((s) => s.key === classKey) || {}).label ||
      folder.name ||
      folder.path;
    state.drill = {
      class: classKey,
      label: label,
      path: folder.path,
      level: owners.length ? "folder" : "files",
    };
    setTab("map");
    renderBreadcrumbs();
    drawTreemap();
  }

  function tierChip(tier) {
    return node("span", "tier-chip tier-" + tier, TIER_LABEL[tier] || tier);
  }

  function renderClasses() {
    if (!el.classes || !state.scan) return;
    el.classes.replaceChildren();
    const rows = state.scan.summaries.filter((s) => s.item_count > 0);
    if (!rows.length) {
      el.classes.appendChild(node("li", "sw-empty", "No files were measured."));
      return;
    }
    const peak = rows.reduce((m, s) => Math.max(m, s.bytes_allocated), 0) || 1;
    for (const s of rows) {
      const li = node("li", "sw-class");
      li.dataset.class = s.key;
      const head = node("div", "sw-class-head");
      head.appendChild(node("span", "sw-class-name", s.label));
      head.appendChild(tierChip(s.tier));
      const bar = node("span", "sw-class-bar");
      bar.setAttribute("aria-hidden", "true");
      const fill = node("i");
      fill.style.width = Math.max(1.5, (s.bytes_allocated / peak) * 100) + "%";
      fill.style.background = tierColor(s.tier);
      bar.appendChild(fill);
      const value = node("span", "sw-class-value");
      value.textContent = fmtBytes(s.bytes_allocated) + " · " + s.item_count + (s.item_count === 1 ? " file" : " files");
      li.append(head, bar, value);
      el.classes.appendChild(li);
    }
  }

  function tierColor(tier) {
    const css = getComputedStyle(document.documentElement);
    const token =
      tier === "safe_to_review"
        ? "--cure-400"
        : tier === "review_carefully"
          ? "--warning"
          : tier === "protected"
            ? "--text-3"
            : "--text-2";
    return (css.getPropertyValue(token) || "#8b93a3").trim();
  }

  function visibleFiles() {
    if (!state.scan) return [];
    if (state.tierFilter === "all") return state.scan.files;
    return state.scan.files.filter((f) => f.tier === state.tierFilter);
  }

  function renderLarge() {
    if (!el.largeBody || !state.scan) return;
    el.largeBody.replaceChildren();
    const files = visibleFiles().slice();
    const mode = el.largeSort ? el.largeSort.value : "size";
    if (mode === "age") {
      files.sort((a, b) => (a.modified_unix || Infinity) - (b.modified_unix || Infinity));
    } else if (mode === "name") {
      files.sort((a, b) => a.name.localeCompare(b.name));
    } else {
      files.sort((a, b) => b.size_allocated - a.size_allocated);
    }
    const shown = files.slice(0, 500);
    if (el.largeCount) {
      const total = state.scan.files_total || files.length;
      el.largeCount.textContent =
        shown.length === files.length && total === files.length
          ? shown.length + (shown.length === 1 ? " file" : " files")
          : "showing " +
            shown.length +
            " of " +
            total.toLocaleString() +
            " measured files";
    }
    if (!shown.length) {
      const tr = node("tr");
      const td = node("td", "sw-empty", "No files matched.");
      td.colSpan = 6;
      tr.appendChild(td);
      el.largeBody.appendChild(tr);
      return;
    }
    for (const f of shown) {
      const tr = node("tr");
      tr.dataset.path = f.path;

      const nameCell = node("td");
      const nameBtn = node("button", "sw-file-name", f.name);
      nameBtn.type = "button";
      nameBtn.addEventListener("click", () => openInspector(f));
      const parent = node("span", "sw-file-parent", f.parent);
      nameCell.append(nameBtn, parent);

      const classCell = node("td", "sw-muted", f.class_label);
      const tierCell = node("td");
      tierCell.appendChild(tierChip(f.tier));

      const days = ageDays(f.modified_unix);
      const modCell = node("td", "sw-muted");
      modCell.textContent = days === null ? "unknown" : fmtDate(f.modified_unix) + " · " + days + "d ago";

      const sizeCell = node("td", "num sw-num", fmtBytes(f.size_allocated));

      const actionCell = node("td");
      actionCell.appendChild(queueButton(f));

      tr.append(nameCell, classCell, tierCell, modCell, sizeCell, actionCell);
      el.largeBody.appendChild(tr);
    }
  }

  /* A queue control exists only where the tier permits a removal request.
     Protected and Unknown rows get an explanation instead of a button. */
  function queueButton(f) {
    if (f.tier === "protected" || f.tier === "unknown") {
      const s = node("span", "sw-no-action", "Not offered");
      s.title =
        f.tier === "protected"
          ? "Protected locations are never proposed for removal."
          : "Uncertain locations are never proposed for removal.";
      return s;
    }
    const on = state.queue.has(f.path);
    const b = node("button", "sw-queue-btn" + (on ? " is-on" : ""), on ? "Queued" : "Queue");
    b.type = "button";
    b.setAttribute("aria-pressed", on ? "true" : "false");
    b.dataset.path = f.path;
    b.dataset.tier = f.tier;
    b.addEventListener("click", () => toggleQueue(f.path, f.tier));
    return b;
  }

  function toggleQueue(path, tier) {
    if (state.queue.has(path)) state.queue.delete(path);
    else state.queue.set(path, { path, tier });
    renderQueue();
    renderLarge();
    renderFolders();
    drawTreemap();
    luma.review(state.queue.size, "Queued for removal");
  }

  function renderDuplicates() {
    if (!el.dupeList || !state.scan) return;
    el.dupeList.replaceChildren();
    if (state.scan.duplicate_coverage) {
      el.dupeCoverage.textContent =
        state.scan.duplicate_coverage.summary +
        " Every reported group below was verified by reading each copy in full and comparing SHA-256 digests.";
    }
    const sets = state.scan.duplicates || [];
    if (!sets.length) {
      el.dupeList.appendChild(
        node(
          "p",
          "sw-empty",
          "No byte-identical files were verified in this scan. Files that merely share a size are never reported here.",
        ),
      );
      return;
    }
    for (const set of sets) {
      const card = node("section", "sw-dupe");
      const head = node("header", "sw-dupe-head");
      head.appendChild(node("h3", "sw-dupe-title", set.copies.length + " identical copies"));
      head.appendChild(node("span", "sw-dupe-size", fmtBytes(set.reclaimable_bytes) + " reclaimable"));
      const digest = node("code", "sw-dupe-hash", set.sha256.slice(0, 16) + "…");
      digest.title = "Full SHA-256: " + set.sha256;
      head.appendChild(digest);
      card.appendChild(head);

      const copies = list("sw-dupe-copies");
      for (const c of set.copies) {
        const li = node("li", "sw-dupe-copy" + (c.is_kept_candidate ? " is-kept" : ""));
        const main = node("div", "sw-dupe-copy-main");
        main.appendChild(node("span", "sw-dupe-copy-name", c.name));
        main.appendChild(node("span", "sw-dupe-copy-path", c.parent));
        const facts = node("div", "sw-dupe-copy-facts");
        facts.appendChild(tierChip(c.tier));
        const days = ageDays(c.modified_unix);
        facts.appendChild(node("span", "sw-muted", days === null ? "date unknown" : days + "d old"));
        const reveal = node("button", "sw-link", "Show in Explorer");
        reveal.type = "button";
        reveal.addEventListener("click", () => revealIn(c.path));
        facts.appendChild(reveal);
        if (c.is_kept_candidate) {
          facts.appendChild(node("span", "sw-kept-tag", "kept"));
        } else if (c.tier !== "protected" && c.tier !== "unknown") {
          const on = state.queue.has(c.path);
          const b = node("button", "sw-queue-btn" + (on ? " is-on" : ""), on ? "Queued" : "Queue extra");
          b.type = "button";
          b.setAttribute("aria-pressed", on ? "true" : "false");
          b.addEventListener("click", () => toggleQueue(c.path, c.tier));
          facts.appendChild(b);
        }
        li.append(main, facts);
        if (c.reasons && c.reasons.length) {
          li.appendChild(reasonsList(c.reasons));
        }
        copies.appendChild(li);
      }
      card.appendChild(copies);
      el.dupeList.appendChild(card);
    }
  }

  function renderQueue() {
    if (!el.queueGroups || !state.queue) return;
    el.queueGroups.replaceChildren();
    const count = state.queue.size;
    if (el.queueBadge) el.queueBadge.textContent = String(count);
    if (el.queueEmpty) el.queueEmpty.classList.toggle("hidden", count > 0);
    if (el.previewBtn) el.previewBtn.disabled = count === 0 || !state.scan;

    if (!count) return;

    /* Group by tier so the consequence of the bulk action is visible before
       the operator commits to it, not hidden in a flat list. */
    const groups = { safe_to_review: [], review_carefully: [] };
    for (const [path, meta] of state.queue) {
      (groups[meta.tier] || (groups[meta.tier] = [])).push({ path, meta });
    }
    for (const tier of ["safe_to_review", "review_carefully"]) {
      const items = groups[tier];
      if (!items || !items.length) continue;
      const section = node("section", "sw-queue-group");
      const head = node("header", "sw-queue-group-head");
      head.appendChild(node("h3", "sw-queue-group-title", TIER_LABEL[tier]));
      head.appendChild(node("span", "sw-queue-group-n", items.length + (items.length === 1 ? " item" : " items")));
      head.appendChild(tierChip(tier));
      section.appendChild(head);
      if (tier === "review_carefully") {
        section.appendChild(
          node(
            "p",
            "sw-queue-warn",
            "These are real files you selected individually. They are not regenerable — review each one.",
          ),
        );
      }
      const ul = list("sw-queue-list");
      for (const item of items) {
        const li = node("li", "sw-queue-item");
        const name = node("span", "sw-queue-name", item.path.split(/[\\/]/).pop() || item.path);
        const path = node("span", "sw-queue-path", item.path);
        const drop = node("button", "sw-link sw-drop", "Remove from queue");
        drop.type = "button";
        drop.addEventListener("click", () => toggleQueue(item.path, item.meta.tier));
        li.append(name, path, drop);
        ul.appendChild(li);
      }
      section.appendChild(ul);
      el.queueGroups.appendChild(section);
    }
  }

  function renderCoverage() {
    /* Before a scan the coverage tab has nothing to report, and three
       empty headings read as a broken page rather than an untouched one. */
    if (el.coverageEmpty) {
      el.coverageEmpty.classList.toggle("hidden", !!state.scan);
    }
    if (!state.scan || !el.statRows) return;
    const c = state.scan.coverage || {};
    el.statRows.replaceChildren();
    const rows = [
      ["Result", c.completeness || "—"],
      ["Files measured", (c.files_seen || 0).toLocaleString()],
      ["Folders walked", (c.dirs_seen || 0).toLocaleString()],
      ["Time", fmtDuration(c.duration_ms)],
      ["Folders that refused access", (c.permission_denied_dirs || 0).toLocaleString()],
      ["Files that refused access", (c.permission_denied_files || 0).toLocaleString()],
      ["Links skipped (never followed)", (c.reparse_points_skipped || 0).toLocaleString()],
      ["Locked or vanished mid-scan", (c.locked_or_vanished || 0).toLocaleString()],
      ...(c.notes || []).map((n) => ["Scan note", n]),
      ["Volume", (c.volume_label || "unknown") + (c.volume_total_bytes ? " · " + fmtBytes(c.volume_total_bytes) + " total, " + fmtBytes(c.volume_free_bytes) + " free" : "")],
    ];
    for (const [k, v] of rows) {
      const wrap = node("div");
      wrap.appendChild(node("dt", null, k));
      wrap.appendChild(node("dd", null, v));
      el.statRows.appendChild(wrap);
    }

    if (el.rootsBody) {
      el.rootsBody.replaceChildren();
      for (const r of c.roots || []) {
        const tr = node("tr");
        tr.appendChild(node("td", null, r.label));
        tr.appendChild(node("td", "sw-muted", r.class.replace(/_/g, " ")));
        const result = node("td");
        if (r.resolved) {
          result.appendChild(node("span", "sw-badge sw-badge-ok", r.incomplete ? "Measured (partial)" : "Measured"));
          if (r.reason) result.appendChild(node("span", "sw-badge-note", r.reason));
        } else {
          result.appendChild(node("span", "sw-badge", "Not present"));
          result.appendChild(node("span", "sw-badge-note", r.reason));
        }
        tr.appendChild(result);
        tr.appendChild(node("td", "num sw-num", (r.files_seen || 0).toLocaleString()));
        tr.appendChild(node("td", "num sw-num", r.resolved ? fmtBytes(r.bytes_allocated) : "—"));
        el.rootsBody.appendChild(tr);
      }
    }

    if (el.failures) {
      el.failures.replaceChildren();
      const failures = (c.failures || []).slice(0, 200);
      if (!failures.length) {
        el.failures.appendChild(node("li", "sw-empty", "Nothing was skipped. Every location in the roots above was read."));
      } else {
        for (const f of failures) {
          const li = node("li", "sw-failure");
          li.appendChild(node("span", "sw-failure-path", f.path));
          li.appendChild(node("span", "sw-failure-reason", f.reason));
          el.failures.appendChild(li);
        }
        if (failures.length < (c.failures || []).length) {
          el.failures.appendChild(node("li", "sw-empty", "and " + (c.failures.length - failures.length) + " more"));
        }
      }
    }
  }

  // ---------------------------------------------------------------------
  // treemap
  // ---------------------------------------------------------------------

  /* Squarified treemap (Bruls, Huizing & van Wijk).
     A naive slice-and-dice layout gives the largest category the whole
     width, which turns a real distribution into one big block plus a
     fringe. Squarifying keeps every tile's aspect ratio near 1, so the map
     stays readable as the distribution gets more skewed. */
  /* Worst aspect ratio in a candidate row, as a ratio of squared aspects.
     Everything is converted to AREA first: mixing raw values with pixel
     dimensions makes the comparison meaningless and silently degenerates
     into "lay everything in one strip". */
  function worstRatio(row, side, scale) {
    if (!row.length || side <= 0) return Infinity;
    let sum = 0;
    let min = Infinity;
    let max = 0;
    /* `row` holds items; the ratio is a property of their areas alone. */
    for (const it of row) {
      const a = it.value * scale;
      sum += a;
      if (a < min) min = a;
      if (a > max) max = a;
    }
    if (sum <= 0 || min <= 0) return Infinity;
    const s2 = sum * sum;
    return Math.max((side * side * max) / s2, s2 / (side * side * min));
  }

  function layoutRow(row, x, y, w, h, scale, out) {
    const sum = row.reduce((s, it) => s + it.value, 0);
    if (!sum || w <= 0 || h <= 0) return;
    if (w >= h) {
      /* Lay the row out as a vertical strip of width `d`. */
      const d = (sum * scale) / h;
      let cy = y;
      for (const item of row) {
        const seg = (item.value * scale) / d;
        if (seg > 0.5) {
          out.push({ item: item, x: x, y: cy, w: d, h: seg });
          cy += seg;
        }
      }
    } else {
      const d = (sum * scale) / w;
      let cx = x;
      for (const item of row) {
        const seg = (item.value * scale) / d;
        if (seg > 0.5) {
          out.push({ item: item, x: cx, y: y, w: seg, h: d });
          cx += seg;
        }
      }
    }
  }

  function squarify(items, x, y, w, h, scale, out) {
    const sorted = items.slice().sort((a, b) => b.value - a.value);
    let row = [];
    let i = 0;
    let rx = x;
    let ry = y;
    let rw = w;
    let rh = h;

    while (i < sorted.length) {
      const side = Math.min(rw, rh);
      const current = worstRatio(row, side, scale);
      const next = worstRatio(row.concat([sorted[i]]), side, scale);
      if (row.length === 0 || next <= current) {
        row.push(sorted[i]);
        i += 1;
        continue;
      }
      layoutRow(row, rx, ry, rw, rh, scale, out);
      const sum = row.reduce((s, v) => s + v.value, 0);
      if (rw >= rh) {
        rx += (sum * scale) / rh;
        rw -= (sum * scale) / rh;
      } else {
        ry += (sum * scale) / rw;
        rh -= (sum * scale) / rw;
      }
      row = [];
      if (rw < 0.5 || rh < 0.5) return;
    }
    if (row.length) layoutRow(row, rx, ry, rw, rh, scale, out);
  }

  function layoutCells(x, y, w, h, items) {
    const total = items.reduce((s, i) => s + i.value, 0);
    if (!total || w < 8 || h < 8) return [];
    const out = [];
    squarify(items, x, y, w, h, (w * h) / total, out);
    return out;
  }

  /* ---- the map drill-down ---------------------------------------------
     The map starts at the level the operator can act on — the categories the
     engine classified. From there it descends into the folders those
     categories are made of, and from a folder into the files inside it,
     using only what the engine measured. Only the top of each level is
     drawn, so a parent and its child never both claim the same bytes. */
  function folderParent(path) {
    const i = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
    return i > 0 ? path.slice(0, i) : "";
  }

  function topLevelOf(folders) {
    const present = new Set(folders.map((f) => f.path.toLowerCase()));
    return folders.filter((f) => !present.has(folderParent(f.path).toLowerCase()));
  }

  function foldersForClass(classKey) {
    const all = (state.scan && state.scan.folders) || [];
    return topLevelOf(all.filter((f) => f.class === classKey))
      .sort((a, b) => (b.size_allocated || 0) - (a.size_allocated || 0))
      .slice(0, 60);
  }

  function foldersUnder(path) {
    const all = (state.scan && state.scan.folders) || [];
    const prefix = path.toLowerCase() + "\\";
    const inside = all.filter((f) => f.path.toLowerCase().startsWith(prefix));
    return topLevelOf(inside)
      .sort((a, b) => (b.size_allocated || 0) - (a.size_allocated || 0))
      .slice(0, 60);
  }

  /* The files measured inside one folder. The engine bounded the file list
     it sends, so this is what was measured, not a promise about the disk. */
  function filesUnder(path) {
    const all = (state.scan && state.scan.files) || [];
    const prefix = path.toLowerCase() + "\\";
    return all
      .filter((f) => (f.parent || "").toLowerCase() === path.toLowerCase())
      .sort((a, b) => (b.size_allocated || 0) - (a.size_allocated || 0))
      .slice(0, 120);
  }

  function drillItems() {
    if (!state.drill) return null;
    if (state.drill.level === "files") {
      return filesUnder(state.drill.path).map((f) => ({ record: f, kind: "file" }));
    }
    if (state.drill.level === "folder") {
      const sub = foldersUnder(state.drill.path);
      if (sub.length) return sub.map((f) => ({ record: f, kind: "folder" }));
      return filesUnder(state.drill.path).map((f) => ({ record: f, kind: "file" }));
    }
    return foldersForClass(state.drill.class).map((f) => ({ record: f, kind: "folder" }));
  }

  /* Publish the measurement to the topbar, so it can say which kind of
     scan it is reporting. */
  function publishScanToTopbar(scan) {
    if (!window.CureTopbar || !scan) return;
    window.CureTopbar.storageScan({
      at: (scan.finished_unix || 0) * 1000,
      bytes: scan.headline ? scan.headline.safe_bytes : 0,
      files: scan.coverage ? scan.coverage.files_seen : 0,
      incomplete: scan.headline ? !!scan.headline.incomplete : false,
    });
  }

  /* A partial measurement must never look like a complete one. The map
     carries the difference: what was read, and what could not be. */
  function renderCoverageBadge() {
    if (!el.mapBadge) return;
    const scan = state.scan;
    if (!scan || !scan.headline) {
      el.mapBadge.classList.add("hidden");
      el.mapBadge.replaceChildren();
      return;
    }
    const coverage = scan.coverage || {};
    if (!scan.headline.incomplete) {
      el.mapBadge.classList.remove("hidden", "is-partial");
      el.mapBadge.classList.add("is-complete");
      el.mapBadge.replaceChildren();
      el.mapBadge.appendChild(
        node("span", "sw-map-badge-label", "Complete for the roots scanned"),
      );
      el.mapBadge.appendChild(
        node(
          "span",
          "dim",
          (coverage.files_seen || 0).toLocaleString() +
            " files measured across every root this scan covered.",
        ),
      );
      return;
    }
    el.mapBadge.classList.remove("hidden", "is-complete");
    el.mapBadge.classList.add("is-partial");
    el.mapBadge.replaceChildren();
    el.mapBadge.appendChild(node("span", "sw-map-badge-label", "Partial coverage"));
    el.mapBadge.appendChild(
      node(
        "span",
        "dim",
        (coverage.files_seen || 0).toLocaleString() +
          " files measured. " +
          (coverage.permission_denied_dirs || 0) +
          " location(s) could not be read, so the totals are a floor rather than a total.",
      ),
    );
  }

  function renderBreadcrumbs() {
    if (!el.crumbs) return;
    el.crumbs.replaceChildren();
    el.crumbs.classList.remove("hidden");

    if (!state.drill) {
      /* The trail is always there, so the map always says where it is
         looking and always offers a way back. */
      const here = node("span", "sw-crumb-current", "All categories");
      el.crumbs.appendChild(here);
      return;
    }

    const root = node("button", "sw-crumb sw-crumb-root", "All categories");
    root.type = "button";
    root.addEventListener("click", () => {
      state.drill = null;
      renderBreadcrumbs();
      drawTreemap();
    });
    el.crumbs.appendChild(root);

    const cat = node("button", "sw-crumb", state.drill.label || state.drill.class);
    cat.type = "button";
    cat.addEventListener("click", () => {
      state.drill = { class: state.drill.class, label: state.drill.label, level: "category" };
      renderBreadcrumbs();
      drawTreemap();
    });
    el.crumbs.appendChild(cat);

    if (state.drill.path) {
      const sep = node("span", "sw-crumb-sep", "/");
      el.crumbs.appendChild(sep);
      const here = node("span", "sw-crumb-current", folderName(state.drill.path));
      el.crumbs.appendChild(here);
    }
    /* Where the map is looking, in full, so the operator can tell two
       same-named folders apart. */
    const where = node("span", "sw-crumb-path", state.drill.path || "");
    el.crumbs.appendChild(where);
  }

  function folderName(path) {
    if (!path) return "";
    const i = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
    return i >= 0 ? path.slice(i + 1) : path;
  }

  /* The map draws one shape of item, whether it came from the category
     summary, from the folder list or from the files inside a folder. */
  function mapItems() {
    const drilled = drillItems();
    if (drilled && drilled.length) {
      return drilled.map((entry) => {
        const f = entry.record;
        const isFile = entry.kind === "file";
        return {
          label: f.name || f.path,
          tier: f.tier,
          class: f.class,
          bytes_allocated: f.size_allocated || 0,
          file_count: isFile ? 1 : f.file_count || 0,
          path: f.path || "",
          kind: entry.kind,
          record: f,
        };
      });
    }
    let items = (state.scan && state.scan.summaries) || [];
    items = items.filter((s) => s.item_count > 0 && s.bytes_allocated > 0);
    if (state.tierFilter !== "all") {
      items = items.filter((s) => s.tier === state.tierFilter);
    }
    return items.map((s) => ({
      label: s.label,
      tier: s.tier,
      class: s.key,
      bytes_allocated: s.bytes_allocated || 0,
      file_count: s.item_count || 0,
      path: "",
      kind: "category",
    }));
  }

  function drawTreemap() {
    const canvas = el.canvas;
    const wrap = el.treemap;
    if (!canvas || !wrap) return;
    const rect = wrap.getBoundingClientRect();
    if (rect.width < 40 || rect.height < 40) {
      state.treemapCells = [];
      return;
    }
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    canvas.width = Math.round(rect.width * dpr);
    canvas.height = Math.round(rect.height * dpr);
    canvas.style.width = rect.width + "px";
    canvas.style.height = rect.height + "px";
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, rect.width, rect.height);

    const pad = 10;
    const W = rect.width - pad * 2;
    const H = rect.height - pad * 2;

    if (!state.scan) {
      /* Before measurement the well is visibly empty. A decorative
         treemap here would be a lie about what the engine has seen. */
      ctx.fillStyle = "rgba(255,255,255,0.025)";
      ctx.fillRect(pad, pad, W, H);
      ctx.strokeStyle = "rgba(255,255,255,0.08)";
      ctx.strokeRect(pad + 0.5, pad + 0.5, W - 1, H - 1);
      ctx.fillStyle = tierColor("protected");
      ctx.font = '600 12px "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif';
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText("Nothing measured yet", pad + W / 2, pad + H / 2);
      state.treemapCells = [];
      return;
    }

    const items = mapItems();
    if (!items.length) {
      ctx.fillStyle = "rgba(255,255,255,0.025)";
      ctx.fillRect(pad, pad, W, H);
      ctx.strokeStyle = "rgba(255,255,255,0.08)";
      ctx.strokeRect(pad + 0.5, pad + 0.5, W - 1, H - 1);
      ctx.fillStyle = tierColor("protected");
      ctx.font = '600 12px "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif';
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(
        state.tierFilter === "all" ? "No files were measured" : "Nothing in this tier",
        pad + W / 2,
        pad + H / 2,
      );
      state.treemapCells = [];
      return;
    }

    const total = items.reduce((s, i) => s + (i.bytes_allocated || 0), 0);
    const cells = layoutCells(
      pad,
      pad,
      W,
      H,
      items.map((s) => ({ value: s.bytes_allocated, class: s, total })),
    );
    state.treemapCells = cells;
    for (const cell of cells) {
      const s = cell.item.class;
      ctx.fillStyle = withAlpha(tierColor(s.tier), cell.item.value > 0 ? 0.3 : 0.1);
      ctx.fillRect(cell.x + 1, cell.y + 1, Math.max(0, cell.w - 2), Math.max(0, cell.h - 2));
      ctx.strokeStyle = "rgba(10,12,18,0.75)";
      ctx.lineWidth = 1;
      ctx.strokeRect(cell.x + 1.5, cell.y + 1.5, Math.max(0, cell.w - 3), Math.max(0, cell.h - 3));
      if (cell.w > 78 && cell.h > 34) {
        ctx.fillStyle = "rgba(233,236,241,0.95)";
        ctx.font = '600 11px "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif';
        ctx.textAlign = "left";
        ctx.textBaseline = "top";
        ctx.fillText(clip(s.label, Math.floor(cell.w / 6.6)), cell.x + 7, cell.y + 7);
        ctx.fillStyle = "rgba(233,236,241,0.62)";
        ctx.font = '500 10px "Cascadia Code", Consolas, monospace';
        ctx.fillText(fmtBytes(s.bytes_allocated), cell.x + 7, cell.y + 23);
        if (s.file_count && cell.w > 96) {
          ctx.fillStyle = "rgba(233,236,241,0.45)";
          ctx.font = '500 10px "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif';
          ctx.fillText(
            s.file_count.toLocaleString() + (s.file_count === 1 ? " file" : " files"),
            cell.x + 7,
            cell.y + 36,
          );
        }
      }
    }
  }

  /* The measured map is revealed with one sweep, so the result arrives as
     a result rather than as a repaint. */
  function revealTreemap() {
    if (!el.treemapWrap) return;
    el.treemapWrap.removeAttribute("data-reveal");
    /* Force a reflow so the animation restarts on a repeat scan. */
    void el.treemapWrap.getBoundingClientRect();
    el.treemapWrap.setAttribute("data-reveal", "1");
  }

  function withAlpha(hex, alpha) {
    const h = (hex || "#8b93a3").replace("#", "");
    const full = h.length === 3 ? h.split("").map((c) => c + c).join("") : h;
    const r = parseInt(full.slice(0, 2), 16) || 0;
    const g = parseInt(full.slice(2, 4), 16) || 0;
    const b = parseInt(full.slice(4, 6), 16) || 0;
    return "rgba(" + r + "," + g + "," + b + "," + alpha + ")";
  }

  function clip(text, max) {
    if (!text) return "";
    return text.length <= max ? text : text.slice(0, Math.max(1, max - 1)) + "…";
  }

  function cellAt(ev) {
    if (!state.treemapCells.length) return null;
    const rect = el.canvas.getBoundingClientRect();
    const x = ev.clientX - rect.left;
    const y = ev.clientY - rect.top;
    for (const cell of state.treemapCells) {
      if (x >= cell.x && x <= cell.x + cell.w && y >= cell.y && y <= cell.y + cell.h) {
        return cell;
      }
    }
    return null;
  }

  function treemapClick(ev) {
    const cell = cellAt(ev);
    if (!cell) return;
    const item = cell.item.class;
    if (item.kind === "category") {
      /* One level down: the folders this category is made of. */
      state.drill = { class: item.class, label: item.label, level: "category" };
      renderBreadcrumbs();
      drawTreemap();
      return;
    }
    if (item.kind === "folder") {
      /* Deeper: the folders inside this one, or the files inside it when
         there are none. */
      const children = foldersUnder(item.path);
      state.drill = {
        class: item.class,
        label: item.label,
        path: item.path,
        level: children.length ? "folder" : "files",
      };
      renderBreadcrumbs();
      drawTreemap();
      return;
    }
    if (item.kind === "file") {
      openInspector(item.record);
      return;
    }
    state.tierFilter = item.tier;
    renderTierFilter();
    renderClasses();
    renderLarge();
    renderFolders();
    drawTreemap();
    openCategoryInspector(item);
  }

  /* Hovering a tile says what the tile is: the full path, the measured
     size, how many files are in it, how it was classified, and how much of
     the level it accounts for. */
  function treemapHover(ev) {
    if (!el.mapTip) return;
    const cell = cellAt(ev);
    if (!cell) {
      el.mapTip.classList.add("hidden");
      return;
    }
    const item = cell.item.class;
    const total = cell.item.total || 0;
    const share = total > 0 ? Math.round((item.bytes_allocated / total) * 100) : 0;
    const lines = [
      item.path || item.label,
      fmtBytes(item.bytes_allocated) +
        (item.file_count
          ? " · " + item.file_count.toLocaleString() +
            (item.file_count === 1 ? " file" : " files")
          : ""),
      (TIER_LABEL[item.tier] || item.tier) +
        (item.class ? " · " + item.class.replace(/_/g, " ") : ""),
    ];
    if (share > 0 && share < 100) {
      lines.push(share + "% of what is shown here");
    }
    el.mapTip.replaceChildren();
    for (const line of lines) {
      el.mapTip.appendChild(node("span", "sw-map-tip-line", line));
    }
    el.mapTip.classList.remove("hidden");
    const wrap = el.treemap.getBoundingClientRect();
    const x = ev.clientX - wrap.left;
    const y = ev.clientY - wrap.top;
    el.mapTip.style.left = Math.min(Math.max(8, x + 14), Math.max(8, wrap.width - 250)) + "px";
    el.mapTip.style.top = Math.max(8, y - 12) + "px";
  }

  // ---------------------------------------------------------------------
  // inspector
  // ---------------------------------------------------------------------

  function openInspector(file) {
    if (!el.inspector) return;
    el.inspector.classList.remove("hidden");
    el.inspectorTitle.textContent = file.name;
    el.inspectorBody.replaceChildren();
    const facts = node("dl", "sw-facts");
    const add = (k, v) => {
      const w = node("div");
      w.appendChild(node("dt", null, k));
      w.appendChild(node("dd", null, v));
      facts.appendChild(w);
    };
    add("Category", file.class_label);
    add("Tier", TIER_LABEL[file.tier] || file.tier);
    add("Size on disk", fmtBytes(file.size_allocated));
    add("Apparent size", fmtBytes(file.size_logical));
    if (file.size_logical !== file.size_allocated) {
      add("Note", "Sparse or compressed: the on-disk figure is what removal would actually return.");
    }
    add("Modified", fmtDate(file.modified_unix));
    add("Created", file.created_unix ? fmtDate(file.created_unix) : "unknown");
    add("Last accessed", file.accessed_unix ? fmtDate(file.accessed_unix) + " (not treated as evidence)" : "unavailable");
    add("Hard links", file.hard_link_count === null || file.hard_link_count === undefined ? "unknown" : String(file.hard_link_count));
    add("Attributes", [file.is_read_only ? "read-only" : null, file.is_hidden ? "hidden" : null].filter(Boolean).join(", ") || "none");
    add("Folder", file.parent);
    el.inspectorBody.appendChild(facts);

    const why = node("section", "sw-inspector-why");
    why.appendChild(node("h3", "section-title", "Why this tier"));
    why.appendChild(reasonsList(file.reasons));
    el.inspectorBody.appendChild(why);

    const actions = node("div", "sw-inspector-actions");
    const reveal = node("button", "btn btn-ghost", "Show in Explorer");
    reveal.type = "button";
    reveal.addEventListener("click", () => revealIn(file.path));
    actions.appendChild(reveal);
    if (file.tier !== "protected" && file.tier !== "unknown") {
      actions.appendChild(queueButton(file));
    }
    el.inspectorBody.appendChild(actions);
  }

  function openCategoryInspector(summary) {
    if (!el.inspector) return;
    el.inspector.classList.remove("hidden");
    el.inspectorTitle.textContent = summary.label;
    el.inspectorBody.replaceChildren();
    const facts = node("dl", "sw-facts");
    const add = (k, v) => {
      const w = node("div");
      w.appendChild(node("dt", null, k));
      w.appendChild(node("dd", null, v));
      facts.appendChild(w);
    };
    add("Files", (summary.item_count || 0).toLocaleString());
    add("On disk", fmtBytes(summary.bytes_allocated));
    add("Apparent", fmtBytes(summary.bytes_logical));
    add("Worst tier present", TIER_LABEL[summary.tier] || summary.tier);
    el.inspectorBody.appendChild(facts);
    el.inspectorBody.appendChild(
      node(
        "p",
        "dim",
        "A category reports its most cautious member. Use the Largest files tab to act on individual files.",
      ),
    );
  }

  function closeInspector() {
    if (el.inspector) el.inspector.classList.add("hidden");
  }

  async function revealIn(path) {
    if (!invoke) return;
    try {
      await invoke("storage_reveal", { path });
    } catch (err) {
      setStatus("Could not open Explorer", "is-err");
      if (el.subline) el.subline.textContent = clean(err);
    }
  }

  // ---------------------------------------------------------------------
  // tabs
  // ---------------------------------------------------------------------

  function setTab(tab) {
    state.tab = tab;
    const map = {
      map: ["sw-tab-map", "sw-panel-map"],
      large: ["sw-tab-large", "sw-panel-large"],
      dupes: ["sw-tab-dupes", "sw-panel-dupes"],
      queue: ["sw-tab-queue", "sw-panel-queue"],
      coverage: ["sw-tab-coverage", "sw-panel-coverage"],
    };
    for (const key of Object.keys(map)) {
      const [tabId, panelId] = map[key];
      const t = $(tabId);
      const p = $(panelId);
      if (!t || !p) continue;
      const on = key === tab;
      t.classList.toggle("is-on", on);
      t.setAttribute("aria-selected", on ? "true" : "false");
      /* Roving tabindex: only the selected tab is in the tab order, so a
         keyboard user reaches the tab strip in one stop. */
      t.setAttribute("tabindex", on ? "0" : "-1");
      p.classList.toggle("hidden", !on);
    }
    if (tab === "map") drawTreemap();
    /* Opening the coverage report after a partial scan is the moment worth
       a curious look: some places could not be read. */
    if (tab === "coverage" && state.scan) {
      const c = state.scan.coverage || {};
      const unread = (c.permission_denied_dirs || 0) + (c.permission_denied_files || 0);
      if (unread > 0) {
        luma.curious(
          unread.toLocaleString() +
            " location(s) could not be read; the coverage tab lists them",
        );
      }
    }
  }

  // ---------------------------------------------------------------------
  // preview + execute
  //
  // The backend keeps the approved plan. The UI sends back the plan's
  // digest and the phrase that approves it; it never sends a selection,
  // so it cannot ask for something other than what was previewed.
  // ---------------------------------------------------------------------

  async function openPreview() {
    if (!state.scan || !state.queue.size || !invoke) return;
    const paths = Array.from(state.queue.keys());
    let plan;
    try {
      plan = await invoke("storage_preview", { paths });
    } catch (err) {
      setStatus("Preview failed", "is-err");
      if (el.subline) el.subline.textContent = clean(err);
      return;
    }
    state.lastPlan = plan;

    el.previewConsequence.textContent = plan.consequence;
    el.previewList.replaceChildren();
    for (const item of plan.items) {
      el.previewList.appendChild(planRow(item));
    }
    if (!plan.items.length) {
      el.previewList.appendChild(node("li", "sw-empty", "Nothing in the selection can be removed."));
    }
    el.previewRefused.replaceChildren();
    for (const item of plan.refused) {
      el.previewRefused.appendChild(planRow(item));
    }
    if (!plan.refused.length) {
      el.previewRefused.appendChild(node("li", "sw-empty", "Nothing was refused."));
    }

    el.confirmInput.value = "";
    el.previewConfirm.disabled = true;
    el.previewPhrase.textContent =
      "Type " + (plan.confirmation_phrase || "REMOVE") + " to approve this plan.";
    openModal(el.previewOverlay, { initial: el.confirmInput });
    luma.state("review", "Removal preview", plan.consequence);
  }

  function planRow(item) {
    const li = node("li", "sw-plan-row");
    const main = node("div", "sw-plan-main");
    main.appendChild(node("span", "sw-plan-name", item.name));
    main.appendChild(node("span", "sw-plan-size", item.size_label));
    const tags = node("div", "sw-plan-tags");
    tags.appendChild(tierChip(item.tier));
    tags.appendChild(node("span", "sw-muted", item.class_label));
    li.append(main, tags);
    li.appendChild(reasonsList(item.reasons));
    return li;
  }

  function closePreview() {
    closeModal(el.previewOverlay);
  }

  async function confirmRemoval() {
    if (!state.scan || !state.lastPlan || !invoke) return;
    const btn = el.previewConfirm;
    btn.disabled = true;
    btn.textContent = "Removing…";
    setStatus("Removing", "is-busy");
    /* The working pose is entered before the call, not after: while the
       engine is still running there are no bytes to report, and nothing on
       screen may imply otherwise. */
    luma.state(
      "cleanup",
      "Removing confirmed files",
      "Waiting for the engine receipt.",
    );
    try {
      const receipt = await invoke("storage_execute", {
        planId: state.lastPlan.plan_id,
        confirmPhrase: el.confirmInput.value,
      });
      closePreview();
      state.queue.clear();
      showReceipt(receipt);
    } catch (err) {
      setStatus("Removal stopped", "is-err");
      luma.failed("Removal stopped: " + clean(err));
      el.previewConsequence.textContent = clean(err);
    } finally {
      btn.disabled = true;
      btn.textContent = "Remove permanently";
    }
  }

  function showReceipt(receipt) {
    /* The receipt says four different things and the labels say which is
       which: what was planned, what was removed, what the files occupied
       before they were removed, and what the volume did. None of them is
       "verified reclaimed". */
    const parts = [];
    parts.push(receipt.deleted + " of " + receipt.attempted + " files removed");
    if (receipt.refused) parts.push(receipt.refused + " refused");
    if (receipt.failed) parts.push(receipt.failed + " failed");
    const receiptLabel = parts.join(" · ");

    el.receiptAmount.textContent = receipt.bytes_removed_label;
    el.receiptLabel.textContent = receiptLabel;
    el.receiptRows.replaceChildren();
    const rows = [
      ["Approved in the plan", String(receipt.planned) + " files"],
      ["Removed", String(receipt.deleted) + " files"],
      ["Refused at execution", String(receipt.refused)],
      ["Could not be removed", String(receipt.failed)],
      [
        "Size of the removed files",
        receipt.bytes_removed_label + " (measured before removal)",
      ],
      ["Estimate before removal", receipt.bytes_estimated_label],
    ];
    if (receipt.bytes_uncertain) {
      rows.push([
        "Uncertain reclaim",
        receipt.bytes_uncertain_label + " (link count could not be read)",
      ]);
    }
    for (const v of receipt.volumes || []) {
      rows.push([
        "Free space on " + v.root,
        v.delta_label + " · " + v.files_removed + " files removed here",
      ]);
    }
    for (const [k, v] of rows) {
      const w = node("div");
      w.appendChild(node("dt", null, k));
      w.appendChild(node("dd", null, v));
      el.receiptRows.appendChild(w);
    }
    el.receiptNotes.replaceChildren();
    if (receipt.note) {
      el.receiptNotes.appendChild(node("li", "sw-note", receipt.note));
    }
    for (const f of receipt.failures || []) {
      const li = node("li", "sw-plan-row");
      li.appendChild(node("span", "sw-plan-name", f.path));
      li.appendChild(node("span", "sw-plan-size", f.reason));
      el.receiptNotes.appendChild(li);
    }
    for (const r of receipt.refused_items || []) {
      const li = node("li", "sw-plan-row");
      li.appendChild(node("span", "sw-plan-name", r.name));
      li.appendChild(node("span", "sw-plan-size", (r.reasons || []).join("; ")));
      el.receiptNotes.appendChild(li);
    }
    if (
      !(receipt.failures || []).length &&
      !(receipt.refused_items || []).length &&
      !receipt.note
    ) {
      el.receiptNotes.appendChild(node("li", "sw-empty", "Nothing failed."));
    }
    openModal(el.receiptOverlay, { initial: el.receiptClose });
    /* LUMA quotes the receipt verbatim rather than paraphrasing it. */
    luma.state("success", "Space reclaimed", receiptLabel);
    setStatus("Removed", "is-ok");
    resetAfterReceipt();
  }

  function resetAfterReceipt() {
    state.scan = null;
    state.zoom = null;
    state.lastPlan = null;
    if (el.amount) el.amount.textContent = "—";
    if (el.amountLabel) el.amountLabel.textContent = "Not measured";
    if (el.amountNote) {
      el.amountNote.textContent =
        "The scan that produced these numbers no longer describes this machine. Run a fresh scan before making another decision.";
    }
    if (el.careful) el.careful.textContent = "—";
    if (el.protectedBytes) el.protectedBytes.textContent = "—";
    if (el.free) el.free.textContent = "—";
    if (el.classes) el.classes.replaceChildren();
    if (el.largeBody) el.largeBody.replaceChildren();
    if (el.dupeList) el.dupeList.replaceChildren();
    if (el.statRows) el.statRows.replaceChildren();
    if (el.rootsBody) el.rootsBody.replaceChildren();
    if (el.failures) el.failures.replaceChildren();
    renderQueue();
    drawTreemap();
    if (el.subline) {
      el.subline.textContent = "Cleanup finished. Run a fresh scan to measure the result.";
    }
  }

  function closeReceipt() {
    closeModal(el.receiptOverlay);
    if (el.receiptOverlay) el.receiptOverlay.classList.add("hidden");
  }

  // ---------------------------------------------------------------------
  // roots
  // ---------------------------------------------------------------------

  async function addRoot() {
    if (!invoke) return;
    let options = [];
    try {
      options = await invoke("storage_available_roots", {});
    } catch (err) {
      setStatus("Could not list locations", "is-err");
      if (el.subline) el.subline.textContent = clean(err);
      return;
    }
    const existing = new Set(state.extraRoots.map((p) => p.toLowerCase()));
    const usable = options.filter((o) => o.exists && !existing.has(o.path.toLowerCase()));
    if (!usable.length) {
      setStatus("Nothing new to add", "is-warn");
      return;
    }
    if (usable.length === 1) {
      addToRoots(usable[0].path, usable[0].label);
      return;
    }
    renderRootPicker(usable);
  }

  /* A folder the operator names. The picker offers drives and well-known
     locations; a project on another drive is only known to the operator. */
  function renderCustomRoot() {
    const wrap = node("span", "sw-custom-root");
    const input = document.createElement("input");
    input.type = "text";
    input.className = "sw-custom-root-input";
    input.placeholder = "Folder path, e.g. D:\\Projects";
    input.setAttribute("aria-label", "Folder to add to this scan");
    const button = node("button", "sw-chip", "Add");
    button.type = "button";
    const submit = async () => {
      const value = input.value.trim();
      if (!value || !invoke) return;
      button.disabled = true;
      try {
        const option = await invoke("storage_validate_root", { path: value });
        input.value = "";
        addToRoots(option.path, option.label);
      } catch (err) {
        setStatus("Could not add that folder", "is-err");
        if (el.subline) el.subline.textContent = clean(err);
      } finally {
        button.disabled = false;
      }
    };
    button.addEventListener("click", submit);
    input.addEventListener("keydown", (ev) => {
      if (ev.key !== "Enter") return;
      ev.preventDefault();
      submit();
    });
    wrap.append(input, button);
    return wrap;
  }

  function renderRootPicker(options) {
    el.rootsBar.classList.remove("hidden");
    el.rootsBar.replaceChildren();
    const label = node("span", "sw-roots-label", "Add:");
    el.rootsBar.appendChild(label);
    for (const o of options.slice(0, 14)) {
      const b = node("button", "sw-chip", o.is_drive ? o.label + " (" + o.path + ")" : o.label);
      b.type = "button";
      b.title = o.path + (o.volume_label ? " · " + o.volume_label : "");
      b.addEventListener("click", () => addToRoots(o.path, o.label));
      el.rootsBar.appendChild(b);
    }
    const custom = renderCustomRoot();
    if (custom) el.rootsBar.appendChild(custom);
    const close = node("button", "sw-link", "Done");
    close.type = "button";
    close.addEventListener("click", () => {
      el.rootsBar.classList.add("hidden");
      el.rootsBar.replaceChildren();
    });
    el.rootsBar.appendChild(close);
  }

  function addToRoots(path) {
    if (state.extraRoots.some((p) => p.toLowerCase() === path.toLowerCase())) return;
    state.extraRoots.push(path);
    renderRootChips();
    luma.review(state.extraRoots.length, "Locations added to this scan");
  }

  function renderRootChips() {
    if (!state.extraRoots.length) {
      el.rootsBar.classList.add("hidden");
      el.rootsBar.replaceChildren();
      return;
    }
    el.rootsBar.classList.remove("hidden");
    el.rootsBar.replaceChildren();
    el.rootsBar.appendChild(node("span", "sw-roots-label", "In this scan:"));
    for (const p of state.extraRoots) {
      const b = node("button", "sw-chip is-on", p);
      b.type = "button";
      b.title = "Remove this location from the next scan";
      b.addEventListener("click", () => {
        state.extraRoots = state.extraRoots.filter((x) => x !== p);
        renderRootChips();
      });
      el.rootsBar.appendChild(b);
    }
  }

  // ---------------------------------------------------------------------
  // wiring
  // ---------------------------------------------------------------------

  function wire() {
    if (el.depthNote) el.depthNote.textContent = DEPTH_NOTES[state.depth];
    const depthButtons = Array.from(document.querySelectorAll("[data-depth]"));
    const pickDepth = (btn) => {
      if (state.running) return;
      state.depth = btn.dataset.depth;
      depthButtons.forEach((b) => {
        const on = b === btn;
        b.classList.toggle("is-on", on);
        b.setAttribute("aria-checked", on ? "true" : "false");
        /* Roving tabindex: only the checked radio is in the tab order. */
        b.setAttribute("tabindex", on ? "0" : "-1");
      });
      if (el.depthNote) el.depthNote.textContent = DEPTH_NOTES[state.depth];
    };
    depthButtons.forEach((btn) => {
      btn.addEventListener("click", () => pickDepth(btn));
      btn.addEventListener("keydown", (ev) => {
        const i = depthButtons.indexOf(btn);
        let next = -1;
        if (ev.key === "ArrowRight" || ev.key === "ArrowDown")
          next = (i + 1) % depthButtons.length;
        else if (ev.key === "ArrowLeft" || ev.key === "ArrowUp")
          next = (i - 1 + depthButtons.length) % depthButtons.length;
        if (next < 0) return;
        ev.preventDefault();
        pickDepth(depthButtons[next]);
        depthButtons[next].focus();
      });
    });

    if (el.scan) el.scan.addEventListener("click", runScan);
    if (el.coverageScan) el.coverageScan.addEventListener("click", runScan);
    if (el.cancel) el.cancel.addEventListener("click", cancelScan);
    if (el.addRoot) el.addRoot.addEventListener("click", addRoot);
    if (el.largeSort) el.largeSort.addEventListener("change", renderLarge);
    if (el.previewBtn) el.previewBtn.addEventListener("click", openPreview);
    if (el.queueClear) {
      el.queueClear.addEventListener("click", () => {
        state.queue.clear();
        renderQueue();
        renderLarge();
    renderFolders();
        drawTreemap();
      });
    }
    if (el.inspectorClose) el.inspectorClose.addEventListener("click", closeInspector);
    if (el.previewCancel) el.previewCancel.addEventListener("click", closePreview);
    if (el.previewConfirm) el.previewConfirm.addEventListener("click", confirmRemoval);
    if (el.receiptClose) el.receiptClose.addEventListener("click", closeReceipt);
    if (el.revealRoot) {
      el.revealRoot.addEventListener("click", async () => {
        /* The real action: show the folder the app keeps its own records in.
           A button that only changes the mascot's caption is a lie. */
        try {
          const shown = await invoke("storage_reveal", { path: "" });
          setStatus("Records folder opened", "is-ok");
          if (el.subline) el.subline.textContent = shown;
        } catch (err) {
          setStatus("Could not open the records folder", "is-err");
          if (el.subline) el.subline.textContent = clean(err);
        }
      });
    }
    if (el.confirmInput) {
      el.confirmInput.addEventListener("input", () => {
        el.previewConfirm.disabled =
          el.confirmInput.value.trim().toUpperCase() !== confirmPhrase();
      });
    }
    if (el.tierFilter) {
      el.tierFilter.addEventListener("click", (ev) => {
        const btn = ev.target.closest("[data-tier]");
        if (!btn) return;
        state.tierFilter = btn.dataset.tier;
        renderTierFilter();
        renderClasses();
        renderLarge();
    renderFolders();
        drawTreemap();
      });
    }
    if (el.canvas) {
      el.canvas.addEventListener("click", treemapClick);
      el.canvas.addEventListener("mousemove", treemapHover);
      el.canvas.addEventListener("mouseleave", () => {
        if (el.mapTip) el.mapTip.classList.add("hidden");
      });
    }
    ["sw-tab-map", "sw-tab-large", "sw-tab-dupes", "sw-tab-queue", "sw-tab-coverage"].forEach((id) => {
      const b = $(id);
      if (!b) return;
      b.addEventListener("click", () => setTab(b.id.replace("sw-tab-", "")));
    });
    /* Keyboard: arrows move between tabs, per the ARIA tabs pattern. */
    const tabIds = ["sw-tab-map", "sw-tab-large", "sw-tab-dupes", "sw-tab-queue", "sw-tab-coverage"];
    tabIds.forEach((id) => {
      const b = $(id);
      if (!b) return;
      b.addEventListener("keydown", (ev) => {
        const i = tabIds.indexOf(id);
        let next = -1;
        if (ev.key === "ArrowRight") next = (i + 1) % tabIds.length;
        else if (ev.key === "ArrowLeft") next = (i - 1 + tabIds.length) % tabIds.length;
        else if (ev.key === "Home") next = 0;
        else if (ev.key === "End") next = tabIds.length - 1;
        if (next < 0) return;
        ev.preventDefault();
        const target = $(tabIds[next]);
        target.focus();
        setTab(target.id.replace("sw-tab-", ""));
      });
    });
    document.addEventListener("keydown", (ev) => {
      if (ev.key === "Escape") {
        /* The modal helper handles Escape for the two dialogs it owns, so
           the page behind them never reacts while one is open. */
        if (!el.receiptOverlay.classList.contains("hidden")) return;
        if (!el.previewOverlay.classList.contains("hidden")) return;
        closeInspector();
      }
    });

    if ("ResizeObserver" in window && el.treemap) {
      let queued = false;
      new ResizeObserver(() => {
        if (queued) return;
        queued = true;
        requestAnimationFrame(() => {
          queued = false;
          drawTreemap();
        });
      }).observe(el.treemap);
    } else {
      window.addEventListener("resize", drawTreemap);
    }
    document.addEventListener("visibilitychange", () => {
      if (!document.hidden) drawTreemap();
    });

    if (listen) {
      listen("storage-progress", (event) => {
        const p = event && event.payload ? event.payload : null;
        if (!p) return;
        /* The duplicate pass reads every byte it is comparing. That is a
           genuine "working it out" moment, so the companion takes the
           thinking pose while it happens. */
        if (p.phase === "hashing") {
          luma.thinking("Verifying duplicates byte for byte");
        } else if (p.phase === "walking" && !state.thinking) {
          luma.scan("Walking the catalogue");
        }
        updateScanProgress(p);
        state.thinking = p.phase === "hashing";
      }).catch(() => {});
    }

    /* Opening the workstation performs no work, so LUMA holds a settled pose
       here until a real measurement starts. */
    luma.idle("Ready to measure");
    resetToIdle();
    setTab("map");
  }

  window.CureStorageWorkstation = {
    idle: function () {
      luma.idle("Ready to measure");
      resetToIdle();
      setTab(state.tab);
    },
    /* Settle the companion without discarding the measurement. A queue the
       operator built is worth keeping across a trip to another screen. */
    settle: function () {
      if (state.scan) {
        luma.idle("Measurement kept");
      } else {
        luma.idle("Ready to measure");
      }
    },
    measuring: function () {
      setRunning(true);
      setStatus("Scanning", "is-busy");
      luma.scan("Measuring storage");
    },
    measured: function () {
      setRunning(false);
      setStatus("Measured", "is-ok");
    },
    cleanup: function (stateName, title, detail) {
      luma.state(stateName, title, detail);
    },
    review: function () {
      luma.review(state.queue.size, "Queued for removal");
    },
    redraw: drawTreemap,
    isRunning: function () {
      return state.running;
    },
    runScan: runScan,
    state: state,
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", wire);
  } else {
    wire();
  }
})();
