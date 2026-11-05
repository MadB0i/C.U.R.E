(function () {
  "use strict";

  const REDUCED = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  const TAU = window.__TAURI__;
  if (!TAU || !TAU.core || !TAU.event) {
    document.getElementById("fallback").classList.remove("hidden");
    return;
  }
  document.getElementById("app").classList.remove("hidden");

  const invoke = TAU.core.invoke;
  const listen = TAU.event.listen;

  const statusPill = document.getElementById("status-pill");
  const statusText = document.getElementById("status-text");
  const logList = document.getElementById("log");
  const feedCountEl = document.getElementById("feed-count");
  const scanView = document.getElementById("scan-view");
  const resultsView = document.getElementById("results-view");
  const landingView = document.getElementById("landing-view");

  // ---- UI V2 session state (all derived from real backend responses) ----
  // lastSummary/lastScanAt/scanDurationMs are set only by runScan success.
  // eventLog holds session events (scan progress, canary alerts, actions).
  let lastSummary = null;
  let lastScanAt = null;
  let scanDurationMs = null;
  let scanStartedAt = 0;
  let scanPhase = "idle"; // idle | running | done
  // Areas that have reported during the current scan — drives the Overview
  // dial's live layer count. Cleared when a scan starts.
  const scanReportedAreas = new Set();
  const eventLog = [];
  const canarySessionAlerts = [];
  let canaryTriggered = false;

  function logEvent(kind, text) {
    eventLog.push({ at: new Date(), kind: kind || "info", text: String(text) });
    if (eventLog.length > 300) eventLog.splice(0, eventLog.length - 300);
    appendEventLogRow(eventLog[eventLog.length - 1]);
    renderOverviewTimeline();
  }

  let scanToken = 0;
  let itemFeedCount = 0;
  // Items successfully quarantined this session (explicit confirm path
  // only — the backend never auto-cleans). Decremented on scoped undo.
  let sessionQuarantined = 0;
  function refreshQuarantinedTile() {
    const el = document.getElementById("stat-cleaned");
    if (el) el.textContent = String(sessionQuarantined);
  }

  function escHtml(s) {
    return String(s)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  function setPill(state, text) {
    statusPill.className = "pill " + state;
    statusText.textContent = text;
    /* UI-03: the X-Ray corner tag and field state are owned by CureSweep's
       own lifecycle (xray.js). This pill used to write them too, so a
       storage measurement or a process action could leave the radar
       claiming "Collecting" with no collection running. One writer per
       panel: the pill reports the session, the radar reports the scan. */
  }

  /* Topbar session context. Scan detail lives on the pages that produce it;
     the global shell carries only the live clock, the theme control and the
     session pill. The Storage Workstation and Detective Mode publish their
     measurement state through CureTopbar so the pill can name a running
     measurement without printing a scan summary into the header. */
  let lastStorageScan = null;
  let storageScanRunning = false;

  window.CureTopbar = {
    storageScan: function (record) {
      lastStorageScan = record;
      storageScanRunning = false;
      restorePill();
    },
    storageScanStarted: function () {
      storageScanRunning = true;
      setPill("scanning", "Measuring storage");
    },
    storageScanFinished: function () {
      storageScanRunning = false;
      restorePill();
    },
  };

  /* Semantic status model — the single source of session-state truth.
     The pill, the Overview hero and the Overview metrics all render from
     this, so no two controls can ever disagree about the session. A
     functioning subsystem is availability, never a health claim: "Not
     assessed" is stated until a scan has actually reported. */
  function sessionStatus() {
    if (storageScanRunning)
      return { pill: "scanning", label: "Measuring storage", tone: "scanning" };
    if (scanPhase === "running" || scanPhase === "preparing")
      return {
        pill: "scanning",
        label: "Collecting evidence",
        tone: "scanning",
      };
    if (scanPhase === "failed")
      return { pill: "error", label: "Operation failed", tone: "bad" };
    if (!lastSummary)
      return { pill: "idle", label: "Not assessed", tone: "idle" };
    const t = troubleCounts(lastSummary);
    const partial = coverageIncomplete(lastSummary);
    if (t.critical > 0)
      return {
        pill: "danger",
        label: partial ? "Attention required · Partial coverage" : "Attention required",
        tone: "bad",
      };
    if (t.findings > 0)
      return {
        pill: "warn",
        label: partial
          ? "Attention required · Partial coverage"
          : "Attention required",
        tone: "warn",
      };
    if (partial)
      return { pill: "warn", label: "Partial coverage", tone: "warn" };
    return {
      pill: "idle",
      label: "No findings in collected evidence",
      tone: "ok",
    };
  }

  function restorePill() {
    if (["running", "preparing"].includes(scanPhase)) return;
    const st = sessionStatus();
    setPill(st.pill, st.label);
  }

  // Shared confirm dialog for other view modules (Agent Radar process
  // control uses it). Resolves true only on explicit confirmation.
  window.CureConfirm = function (opts) {
    return requestConfirm(opts || {});
  };

  // Theme toggle: switches between dark and light themes, persists to localStorage
  function applyTheme(theme) {
    const isLight = theme === "light";
    document.documentElement.dataset.theme = isLight ? "light" : "dark";
    try {
      localStorage.setItem("cure-theme", isLight ? "light" : "dark");
    } catch (_) {}
    const btn = document.getElementById("theme-toggle");
    if (btn) {
      btn.setAttribute("aria-label", isLight ? "Switch to dark theme" : "Switch to light theme");
      btn.title = isLight ? "Switch to dark theme" : "Switch to light theme";
      const iconDark = btn.querySelector(".theme-icon-dark");
      const iconLight = btn.querySelector(".theme-icon-light");
      if (iconDark && iconLight) {
        iconDark.classList.toggle("hidden", isLight);
        iconLight.classList.toggle("hidden", !isLight);
      }
    }
  }

  function initTheme() {
    let saved = null;
    try {
      saved = localStorage.getItem("cure-theme");
    } catch (_) {}
    if (!saved) {
      saved = "dark";
    }
    applyTheme(saved);
    const btn = document.getElementById("theme-toggle");
    if (btn) {
      btn.addEventListener("click", () => {
        const current = document.documentElement.dataset.theme === "light" ? "light" : "dark";
        applyTheme(current === "light" ? "dark" : "light");
      });
    }
  }
  initTheme();

  // Topbar clock — the PC's own local date and time, recomputed from the
  // system clock on every tick. Suspend/resume and timezone changes can
  // never leave a stale value on screen; the visibility listener re-anchors
  // immediately when the window comes back.
  function tickClock() {
    const el = document.getElementById("topbar-clock");
    if (!el) return;
    el.textContent = new Date().toLocaleString([], {
      weekday: "short",
      day: "numeric",
      month: "short",
      year: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  }
  tickClock();
  setInterval(tickClock, 1000);
  document.addEventListener("visibilitychange", tickClock);

  // Placed action receipt: quarantine/undo outcomes get a stable, inline
  // home in the results view (instead of only a floating toast).
  function showReceipt(title, detail, isError) {
    const box = document.getElementById("action-receipt");
    const txt = document.getElementById("action-receipt-text");
    if (!box || !txt) return;
    txt.textContent =
      title + " — " + detail + " · " + new Date().toLocaleTimeString();
    box.classList.toggle("error", !!isError);
    box.classList.remove("hidden");
  }
  function hideReceipt() {
    const box = document.getElementById("action-receipt");
    if (box) box.classList.add("hidden");
  }

  function appendLog(stage, message) {
    const li = document.createElement("li");
    const tag = document.createElement("b");
    tag.textContent = stage;
    const body = document.createElement("span");
    body.textContent = String(message);
    li.append(tag, body);
    logList.append(li);
    while (logList.children.length > 200) logList.firstChild.remove();
    logList.scrollTop = logList.scrollHeight;
  }

  function appendItemLine(p) {
    const li = document.createElement("li");
    li.className =
      "item-line fresh risk-" + String(p.risk || "Safe").toLowerCase();

    const arrow = document.createElement("span");
    arrow.className = "arrow";
    arrow.textContent = "→";

    const name = document.createElement("span");
    name.className = "iname";
    name.textContent = String(p.name || "?");

    const src = document.createElement("span");
    src.className = "isrc";
    src.textContent = "[" + String(p.source || "?") + "]";

    const risk = document.createElement("span");
    risk.className = "irisk";
    risk.textContent = String(p.risk || "?");

    const score = document.createElement("span");
    score.className = "iscore";
    score.textContent = "(" + String(p.score) + ")";

    li.append(arrow, name, src, risk, score);
    logList.appendChild(li);
    while (logList.children.length > 200)
      logList.removeChild(logList.firstChild);
    logList.scrollTop = logList.scrollHeight;
    itemFeedCount += 1;
    if (feedCountEl) feedCountEl.textContent = itemFeedCount + " ITEMS";
  }

  function appendProcessLine(p) {
    const li = document.createElement("li");
    li.className =
      "item-line fresh risk-" + String(p.risk || "Safe").toLowerCase();

    const arrow = document.createElement("span");
    arrow.className = "arrow";
    arrow.textContent = "\u26A0";

    const name = document.createElement("span");
    name.className = "iname";
    name.textContent = String(p.name || "?") + " (pid " + p.pid + ")";

    const src = document.createElement("span");
    src.className = "isrc";
    src.textContent = "[process]";

    const risk = document.createElement("span");
    risk.className = "irisk";
    risk.textContent = String(p.risk || "?");

    const score = document.createElement("span");
    score.className = "iscore";
    score.textContent = "(" + String(p.score) + ")";

    li.append(arrow, name, src, risk, score);
    logList.appendChild(li);
    while (logList.children.length > 200)
      logList.removeChild(logList.firstChild);
    logList.scrollTop = logList.scrollHeight;
    itemFeedCount += 1;
    if (feedCountEl) feedCountEl.textContent = itemFeedCount + " ITEMS";
  }

  function appendRansomLine(p) {
    const li = document.createElement("li");
    li.className = "item-line fresh risk-highrisk";
    const arrow = document.createElement("span");
    arrow.className = "arrow";
    arrow.textContent = "\u26A0";
    const name = document.createElement("span");
    name.className = "iname";
    name.textContent =
      p.finding_type === "ransom-note" ? "Ransom note" : "Bulk encryption";
    const detail = document.createElement("span");
    detail.className = "isrc";
    detail.textContent = p.detail || "";
    li.append(arrow, name, detail);
    logList.appendChild(li);
    while (logList.children.length > 200)
      logList.removeChild(logList.firstChild);
    logList.scrollTop = logList.scrollHeight;
    itemFeedCount += 1;
    if (feedCountEl) feedCountEl.textContent = itemFeedCount + " ITEMS";
  }

  // ---- shared finding presentation ----
  // Presentation-only topology is isolated from scan/action state.
  function countUp(el, target) {
    if (el) el.textContent = String(target);
  }

  const SOURCE_LABELS = {
    StartupFolder: "Startup folder",
    ScheduledTask: "Scheduled task",
    RegistryRun: "Registry run",
    WindowsService: "Windows service",
    WmiSubscription: "WMI subscription",
    IfeoDebugger: "IFEO debugger",
    AppInitDlls: "AppInit DLLs",
    ComHijack: "COM hijack",
  };

  // Fallback only: the backend attaches {id, name} per finding (single
  // source of truth) and the UI prefers it — see attackFor().
  const ATTACK_MAP = {
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

  function attackFor(scored) {
    if (scored && scored.attack && scored.attack.id) {
      return { id: scored.attack.id, name: scored.attack.name || "" };
    }
    const raw = scored && scored.entry ? String(scored.entry.source) : "";
    return ATTACK_MAP[raw] || null;
  }

  // Evidence-based scope caption: what was actually checked, what was
  // skipped, and when. Never states more than the backend established.
  function scopeLine(summary) {
    const states = (summary && summary.source_states) || [];
    let skipped = 0;
    for (const row of states) {
      const st = row.state;
      if (st && typeof st === "object" && "Partial" in st && st.Partial) {
        skipped += st.Partial.skipped || 0;
      }
    }
    let s = summary.total + " entries checked";
    s += " · " + skipped + " skipped";
    if (lastScanAt) s += " · " + lastScanAt.toLocaleTimeString();
    return s;
  }

  // Evidence items arrive as {level, detail} objects (core Evidence model).
  // String fallbacks keep odd payloads readable instead of printing [object].
  function evDetail(ev) {
    if (typeof ev === "string") return ev;
    return (ev && ev.detail) || "";
  }
  function evLabel(ev) {
    return (ev && typeof ev === "object" && ev.level) || "";
  }

  function reasonChipLabel(reason) {
    const lower = reason.toLowerCase();
    if (lower.includes("drop zone")) return ["Suspicious path", "red"];
    if (lower.includes("randomly generated")) return ["Random name", "red"];
    if (lower.includes("powershell")) return ["Hidden PowerShell", "red"];
    if (lower.includes("trusted install")) return ["Trusted location", "teal"];
    if (lower.includes("user profile folder")) return ["Profile exe", "amber"];
    // Detection Engine v2 — note: "invalid signature" contains "valid
    // signature", so it must be matched first.
    if (lower.includes("invalid signature"))
      return ["Invalid Signature", "red"];
    if (lower.includes("valid signature")) return ["Valid Signature", "teal"];
    if (lower.includes("known malware hash"))
      return ["Known Malware Hash", "red"];
    if (lower.includes("unsigned binary")) return ["Unsigned Binary", "amber"];
    if (lower.includes("missing from disk")) return ["Missing image", "red"];
    if (lower.includes("class redirection")) return ["COM redirect", "amber"];
    if (lower.includes("service starts")) return ["Auto-start", ""];
    if (lower.includes("runs as")) return ["Account", ""];
    if (lower.includes("state at scan")) return ["State", ""];
    const stripped = reason.replace(/^[+-]\d+\s*/, "");
    return [stripped.split(/\s+/).slice(0, 3).join(" "), ""];
  }

  function scoreChipClass(score) {
    if (score >= 55) return "crit";
    if (score >= 40) return "high";
    if (score >= 25) return "med";
    return "low";
  }

  const quarantinedIds = new Set();
  const evidenceCache = new Map();
  const fileBacked = (scored) =>
    ["StartupFolder", "ScheduledTask"].includes(String(scored.entry.source));
  const riskLabel = (risk) =>
    risk === "HighRisk"
      ? "High risk"
      : risk === "Suspicious"
        ? "Review required"
        : "Safe score";
  function coverageIncomplete(summary) {
    const rows = (summary && summary.source_states) || [];
    return (
      !rows.length ||
      rows.some((row) => !["Available", "Checked"].includes(row.state))
    );
  }
  function makeText(tag, text, cls) {
    const node = document.createElement(tag);
    node.textContent = text;
    if (cls) node.className = cls;
    return node;
  }
  // One badge for the card's evidence items: the shared level when they
  // all agree, "mixed levels" when they do not. Empty shows nothing here —
  // the legacy note below covers that case honestly.
  function evidenceBadge(evidence) {
    const levels = [];
    for (const ev of evidence) {
      const l = evLabel(ev);
      if (l && !levels.includes(l)) levels.push(l);
    }
    if (levels.length === 1) return ["Evidence: " + levels[0], "teal"];
    if (levels.length > 1) return ["Evidence: mixed levels", "amber"];
    return null;
  }

  function buildCard(scored, cleaned) {
    const card = document.createElement("li");
    card.className = "review-card risk-" + scoreChipClass(scored.score);
    card.dataset.id = scored.entry.id;
    /* The real score drives the row's risk meter — no invented severity. */
    card.style.setProperty("--score", String(Math.max(0, Math.min(100, scored.score))));
    const main = makeText("div", "", "rc-main");
    const top = makeText("div", "", "rc-top");
    const select = makeText("button", scored.entry.name, "finding-select");
    select.type = "button";
    select.setAttribute("aria-label", "Inspect evidence: " + scored.entry.name);
    select.addEventListener("click", () => openEvidence(scored, select));
    const severity = makeText(
      "span",
      riskLabel(scored.risk) + " · " + scored.score,
      "score-chip " + scoreChipClass(scored.score),
    );
    const meter = makeText("div", "", "rc-meter");
    meter.setAttribute("aria-hidden", "true");
    meter.appendChild(makeText("i", "", "rc-meter-fill"));
    top.append(select, severity, meter);
    const meta = makeText("div", "", "chips");
    meta.append(
      makeText(
        "span",
        SOURCE_LABELS[scored.entry.source] || scored.entry.source,
        "chip src",
      ),
    );
    const attack = attackFor(scored);
    if (attack) meta.append(makeText("span", attack.id, "chip"));
    const signature = makeText(
      "span",
      "Signature: not collected",
      "chip finding-signature",
    );
    meta.append(signature);
    const loc = makeText(
      "div",
      scored.entry.command || scored.entry.location,
      "finding-loc selectable",
    );
    loc.title = loc.textContent;
    const evidence = scored.evidence || [];
    const badge = evidenceBadge(evidence);
    if (badge) meta.append(makeText("span", badge[0], "chip " + badge[1]));
    const evidenceRow = makeText(
      "div",
      evidence
        .map((ev) => evDetail(ev).replace(/^[+-]\d+\s*/, ""))
        .filter(Boolean)
        .join(" · "),
      "finding-reasons",
    );
    if (!evidence.length) {
      // Pre-evidence data: never render as a normal finding with nothing
      // wrong. The level is unknown, so none is shown.
      evidenceRow.textContent = "Legacy report: evidence not recorded";
      evidenceRow.classList.add("dim");
      evidenceRow.title =
        "This finding comes from data recorded before evidence levels existed.";
    }
    main.append(top, meta, loc, evidenceRow);
    card.append(main);
    card.append(actionButton(scored, cleaned));
    return card;
  }
  function actionButton(scored, cleaned) {
    const done = cleaned || quarantinedIds.has(scored.entry.id);
    if (done)
      return makeText("span", "Quarantined · Undo available", "manual-note");
    if (!fileBacked(scored))
      return makeText("span", "Guidance only", "manual-note");
    const btn = makeText("button", "Quarantine", "quarantine-btn");
    btn.type = "button";
    btn.addEventListener("click", () => confirmQuarantine(scored, btn));
    return btn;
  }
  async function confirmQuarantine(scored, btn) {
    const ok = await requestConfirm({
      kicker: "Operator action / reversible file move",
      title: "Quarantine this file?",
      facts: [
        ["Item", scored.entry.name],
        ["Original location", scored.entry.location],
      ],
      sections: [
        [
          "What will change",
          "The persistence file moves to C.U.R.E quarantine. The engine records its original location, size, SHA-256 and available security metadata.",
        ],
        [
          "What will not change",
          "Quarantine does not delete the file, terminate its running process, or change registry, service or WMI settings. A referenced executable may remain elsewhere.",
        ],
        [
          "How to undo",
          "Open Quarantine and choose Undo. Restore is scoped to scanned roots and checks archived bytes before moving them back. ACL and timestamp fidelity is best effort.",
        ],
      ],
      okLabel: "Quarantine file",
    });
    if (!ok) return;
    btn.disabled = true;
    btn.setAttribute("aria-busy", "true");
    try {
      await invoke("quarantine_entry", {
        id: scored.entry.id,
        name: scored.entry.name,
        command: scored.entry.command,
      });
      quarantinedIds.add(scored.entry.id);
      sessionQuarantined++;
      refreshQuarantinedTile();
      btn.textContent = "Quarantined";
      btn.removeAttribute("aria-busy");
      document.querySelectorAll("[data-id]").forEach((row) => {
        if (row.dataset.id === scored.entry.id) {
          const action = row.querySelector(".quarantine-btn");
          if (action) {
            action.textContent = "Quarantined";
            action.disabled = true;
          }
        }
      });
      showReceipt(
        "Quarantined: " + scored.entry.name,
        "recorded move · Undo available in Quarantine",
        false,
      );
      footFeedback(
        "Quarantined: " + scored.entry.name + " · Undo available",
        false,
      );
      logEvent("action", "Quarantined: " + scored.entry.name);
      await refreshQuarantineCount();
      if (evidenceSelected === scored)
        renderEvidence(scored, evidenceCache.get(scored.entry.id));
    } catch (err) {
      btn.disabled = false;
      btn.removeAttribute("aria-busy");
      showReceipt(
        "Quarantine failed",
        cleanErrText(err, "Quarantine failed"),
        true,
      );
      footFeedback(cleanErrText(err, "Quarantine failed"), true);
    }
  }
  let evidenceSelected = null;
  let evidenceInvoker = null;
  let evidenceGeneration = 0;
  function section(body, title, content, technical = false) {
    const box = makeText("section", "", "inspector-section");
    box.append(makeText("h3", title));
    box.append(
      makeText(
        "p",
        content || "Not collected",
        technical ? "technical selectable" : "",
      ),
    );
    body.append(box);
    return box;
  }
  function renderEvidence(scored, details) {
    const body = document.getElementById("evidence-body");
    body.replaceChildren();
    const done = quarantinedIds.has(scored.entry.id);
    section(
      body,
      "Summary",
      riskLabel(scored.risk) +
        " · score " +
        scored.score +
        " · " +
        (done ? "Quarantined" : "Awaiting operator decision"),
    );
    const evidenceSection = section(body, "Why C.U.R.E flagged this", "");
    evidenceSection.querySelector("p").remove();
    const evidence = scored.evidence || [];
    if (evidence.length) {
      const ul = document.createElement("ul");
      evidence.forEach((ev) =>
        ul.append(
          makeText(
            "li",
            (evLabel(ev) ? "[" + evLabel(ev) + "] " : "") + evDetail(ev),
          ),
        ),
      );
      evidenceSection.append(ul);
    } else {
      // Pre-evidence data: state it, never show an empty list as normal.
      evidenceSection.append(
        makeText(
          "p",
          "Legacy report: evidence not recorded — this finding comes from data recorded before evidence levels existed.",
          "dim",
        ),
      );
    }
    const attack = attackFor(scored);
    section(
      body,
      "Persistence source",
      (SOURCE_LABELS[scored.entry.source] || scored.entry.source) +
        (attack ? " · " + attack.id : "") +
        "\n" +
        scored.entry.location,
      true,
    );
    section(
      body,
      "Target",
      (details && details.target_path) || scored.entry.command,
      true,
    );
    section(
      body,
      "Signature / publisher",
      details
        ? details.signature +
            " · " +
            (details.publisher || "Publisher unavailable")
        : "Collecting signature and publisher…",
    );
    section(
      body,
      "SHA-256 · resolved target",
      (details && details.sha256_hex) ||
        (details ? "Not available" : "Collecting…"),
      true,
    );
    section(body, "Arguments / command line", scored.entry.command, true);
    let times =
      details && details.modified_unix_secs
        ? "Target modified: " +
          new Date(details.modified_unix_secs * 1000).toLocaleString()
        : "Target timestamps not available";
    times +=
      "\nCollected in scan: " +
      (lastScanAt ? lastScanAt.toLocaleString() : "Not recorded");
    section(body, "Timestamps", times);
    if (details && details.shortcut)
      section(
        body,
        "Shortcut evidence",
        JSON.stringify(details.shortcut, null, 2),
        true,
      );
    if (details && details.task)
      section(
        body,
        "Scheduled task evidence",
        JSON.stringify(details.task, null, 2),
        true,
      );
    section(
      body,
      "Coverage / collection limitations",
      (coverageIncomplete(lastSummary) ? "Coverage incomplete. " : "") +
        "Some collectors do not expose access coverage. Signature revocation is cache-only by default. A risk score is an investigation lead. " +
        ((details && details.collection_error) || ""),
    );
    const actions = section(
      body,
      "Available actions",
      fileBacked(scored)
        ? "Quarantine moves the persistence file after confirmation."
        : "Guidance only: investigate and back up this persistence source before manual changes.",
    );
    const row = makeText("div", "", "evidence-actions");
    const copy = makeText("button", "Copy evidence", "copy-btn");
    copy.addEventListener("click", () =>
      copyEvidence(
        evidenceText(scored) +
          (details ? "\n" + JSON.stringify(details, null, 2) : ""),
        copy,
      ),
    );
    row.append(copy);
    const reveal = makeText("button", "Open location", "copy-btn");
    reveal.addEventListener("click", async () => {
      try {
        await invoke("reveal_location", { id: scored.entry.id });
      } catch (err) {
        footFeedback(cleanErrText(err, "Location unavailable"), true);
      }
    });
    row.append(reveal);
    row.append(actionButton(scored, done));
    if (done) {
      const q = makeText("button", "Open Quarantine / Undo", "btn");
      q.addEventListener("click", () => {
        closeEvidence();
        navTo("view-quarantine");
      });
      row.append(q);
    }
    actions.append(row);
    // Signature state belongs to every visible row for this finding; never inferred from risk reasons.
    if (details)
      document.querySelectorAll("[data-id]").forEach((row) => {
        if (row.dataset.id === scored.entry.id) {
          const sig = row.querySelector(".finding-signature");
          if (sig) sig.textContent = "Signature: " + details.signature;
        }
      });
  }
  async function openEvidence(scored, invoker) {
    evidenceSelected = scored;
    evidenceInvoker = invoker;
    const generation = ++evidenceGeneration;
    document.getElementById("evidence-title").textContent = scored.entry.name;
    document.getElementById("evidence-subtitle").textContent =
      SOURCE_LABELS[scored.entry.source] || scored.entry.source;
    document.getElementById("evidence-overlay").classList.remove("hidden");
    document.getElementById("app").inert = true;
    renderEvidence(scored, evidenceCache.get(scored.entry.id));
    document.getElementById("evidence-close").focus();
    if (evidenceCache.has(scored.entry.id)) return;
    let details;
    try {
      details = await invoke("entry_details", { id: scored.entry.id });
      evidenceCache.set(scored.entry.id, details);
    } catch (err) {
      details = {
        signature: "Unavailable",
        collection_error: cleanErrText(
          err,
          "Evidence enrichment unavailable. Rescan to refresh.",
        ),
      };
    }
    if (generation === evidenceGeneration && evidenceSelected === scored)
      renderEvidence(scored, details);
  }
  function closeEvidence() {
    evidenceGeneration++;
    evidenceSelected = null;
    document.getElementById("evidence-overlay").classList.add("hidden");
    document.getElementById("app").inert = false;
    if (evidenceInvoker && evidenceInvoker.isConnected) evidenceInvoker.focus();
    if (canaryQueued) showCanaryAlert();
  }
  document
    .getElementById("evidence-close")
    .addEventListener("click", closeEvidence);
  document
    .getElementById("evidence-overlay")
    .addEventListener("mousedown", (ev) => {
      if (ev.target.id === "evidence-overlay") closeEvidence();
    });
  document.addEventListener("keydown", (ev) => {
    if (!evidenceSelected || confirmOpen) return;
    if (ev.key === "Escape") {
      ev.preventDefault();
      closeEvidence();
    }
    if (ev.key === "Tab") {
      const buttons = [
        ...document.querySelectorAll(
          "#evidence-inspector button:not(:disabled)",
        ),
      ];
      const first = buttons[0],
        last = buttons[buttons.length - 1];
      if (ev.shiftKey && document.activeElement === first) {
        ev.preventDefault();
        last.focus();
      } else if (!ev.shiftKey && document.activeElement === last) {
        ev.preventDefault();
        first.focus();
      }
    }
  });

  function fillCards(container, entries, cleaned, baseDelayMs) {
    container.innerHTML = "";
    entries.forEach((entry, index) => {
      const card = buildCard(entry, cleaned);
      card.style.setProperty("--d", baseDelayMs + index * 80 + "ms");
      container.appendChild(card);
    });
  }

  // Results coverage strip: same backend source_states as Overview, but
  // compact. PARTIAL is amber, CHECK FAILED / ACCESS DENIED are red,
  // UNAVAILABLE is grey — a partial scan never resembles a fully checked one.
  function renderResultsCoverage(summary) {
    const cov = document.getElementById("res-coverage");
    if (!cov) return;
    cov.innerHTML = "";
    const states = (summary && summary.source_states) || [];
    if (!states.length) {
      cov.append(
        covRow(
          "Coverage",
          "per-source states not reported for this scan",
          "idle",
        ),
      );
      return;
    }
    const levelOf = (st) => {
      // Real backend CoverageState serializes as "Checked" / "NotChecked" /
      // "Unavailable" / "CheckFailed" / "AccessDenied" strings or
      // {"Partial": {"skipped": N}}; the dev mock uses "Available".
      if (st === "Available" || st === "Checked") return "ok";
      if (st === "Unavailable" || st === "NotChecked") return "idle";
      if (st === "CheckFailed") return "bad";
      if (st === "AccessDenied") return "warn";
      if (st && typeof st === "object") {
        if ("Partial" in st) return "warn";
        if ("CheckFailed" in st) return "bad";
        if ("AccessDenied" in st) return "warn";
        if ("Unavailable" in st || "NotChecked" in st) return "idle";
        if ("Available" in st || "Checked" in st) return "ok";
      }
      return "idle";
    };
    for (const row of states) {
      const st = row.state;
      const info = sourceStatusInfo(st);
      // Stage UI-03 known-cosmetic fix: the reason text rode in BOTH the
      // status-derived detail and the coverage row's own detail, so a denied
      // source read "ACCESS DENIED — access denied (elevation may help) ·
      // access denied (elevation may help)". Append the row detail only when
      // it adds information the status detail does not already contain.
      const statusDetail =
        info.detail && info.detail !== "fully enumerated" ? info.detail : "";
      const rowDetail =
        row.detail &&
        !(
          statusDetail &&
          statusDetail
            .toLowerCase()
            .includes(String(row.detail).toLowerCase())
        )
          ? row.detail
          : "";
      cov.append(
        covRow(
          row.area || "source",
          info.label +
            (statusDetail ? " — " + statusDetail : "") +
            (rowDetail ? " · " + rowDetail : ""),
          levelOf(st),
        ),
      );
    }
  }

  function renderResults(summary) {
    const headline = document.getElementById("headline");
    const reviewBlock = document.getElementById("review-block");
    const cleanedBlock = document.getElementById("cleaned-block");
    const reviewClear = document.getElementById("review-clear");

    const cleanedCount = summary.high_risk_cleaned.length;
    const reviewCount = summary.suspicious_for_review.length;
    const procCount = (summary.process_findings || []).length;
    const ransomCount = (summary.ransom_findings || []).length;
    const trouble = cleanedCount + reviewCount + procCount + ransomCount;

    const subline = document.getElementById("subline");
    const scope = scopeLine(summary);
    if (trouble > 0) {
      headline.textContent = "Review required";
      subline.textContent = scope;
    } else {
      headline.textContent = coverageIncomplete(summary)
        ? "Coverage incomplete"
        : "No findings in collected evidence";
      subline.textContent = scope;
    }
    renderResultsCoverage(summary);

    countUp(document.getElementById("stat-cleaned"), sessionQuarantined);
    countUp(document.getElementById("stat-review"), reviewCount);
    countUp(document.getElementById("stat-safe"), summary.safe);
    countUp(document.getElementById("stat-procs"), procCount);

    const bFindings = document.getElementById("badge-findings");
    if (bFindings) bFindings.textContent = String(reviewCount);
    const bProcs = document.getElementById("badge-processes");
    if (bProcs) bProcs.textContent = String(procCount);
    const bRansom = document.getElementById("badge-ransom");
    if (bRansom) {
      bRansom.textContent = String(ransomCount);
      bRansom.classList.toggle("hidden", ransomCount === 0);
    }
    const bCov = document.getElementById("badge-coverage");
    if (bCov && summary.source_states) {
      const checked = summary.source_states.filter((s) => s.state === "Checked").length;
      bCov.textContent = checked + "/" + summary.source_states.length;
      bCov.className = "inv-lens-badge " + (checked === summary.source_states.length ? "inv-lens-badge-ok" : "inv-lens-badge-neutral");
    }

    const procEmpty = document.getElementById("proc-empty");
    if (procEmpty) procEmpty.classList.toggle("hidden", procCount > 0);
    const ransomEmpty = document.getElementById("ransom-empty");
    if (ransomEmpty) ransomEmpty.classList.toggle("hidden", ransomCount > 0);

    fillCards(
      document.getElementById("review-cards"),
      summary.suspicious_for_review,
      false,
      290,
    );
    fillCards(
      document.getElementById("cleaned-cards"),
      summary.high_risk_cleaned,
      true,
      250,
    );

    reviewClear.classList.toggle("hidden", reviewCount > 0);
    reviewClear.querySelector("span").textContent = coverageIncomplete(summary)
      ? "No persistence findings in collected evidence — coverage is incomplete."
      : "No persistence findings in collected evidence.";
    reviewBlock.classList.toggle("hidden", false);

    const showCleanedPanel = cleanedCount > 0;
    cleanedBlock.classList.toggle("hidden", !showCleanedPanel);

    // Process findings panel
    const processBlock = document.getElementById("process-block");
    const ransomBlock = document.getElementById("ransom-block");
    if (processBlock) {
      const procFindings = summary.process_findings || [];
      sweepState.findings = procFindings;
      sweepState.checked.clear();
      updateKillButton();
      const procCards = document.getElementById("process-cards");
      if (procCards) {
        procCards.innerHTML = "";
        procFindings.forEach(function (p) {
          const card = document.createElement("li");
          card.className =
            "review-card proc-entry risk-" + scoreChipClass(p.score);
          card.dataset.name = p.name;
          card.dataset.pid = String(p.pid);
          card.dataset.exe = p.exe_path || "";
          const box = document.createElement("input");
          box.type = "checkbox";
          box.setAttribute(
            "aria-label",
            "Select " + p.name + " (pid " + p.pid + ") for termination",
          );
          box.addEventListener("change", function () {
            if (box.checked) sweepState.checked.add(p.pid);
            else sweepState.checked.delete(p.pid);
            updateKillButton();
            const killStatusEl = document.getElementById("kill-procs-status");
            if (killStatusEl) {
              killStatusEl.textContent = "";
              killStatusEl.classList.add("hidden");
            }
            card.classList.toggle("proc-selected", box.checked);
          });
          const main = document.createElement("div");
          main.className = "rc-main";
          const topRow = document.createElement("div");
          topRow.className = "rc-top";
          const nameEl = document.createElement("span");
          nameEl.className = "rc-name";
          nameEl.textContent = p.name;
          nameEl.title = p.exe_path || "";
          const scoreEl = document.createElement("span");
          scoreEl.className = "score-chip " + scoreChipClass(p.score);
          scoreEl.textContent = String(p.score);
          scoreEl.title = p.risk + " · risk score " + p.score;
          topRow.append(nameEl, scoreEl);
          main.appendChild(topRow);
          const chips = document.createElement("div");
          chips.className = "chips";
          var pidChip = document.createElement("span");
          pidChip.className = "chip";
          pidChip.textContent = "pid " + p.pid;
          chips.appendChild(pidChip);
          if (p.exe_path) {
            var exeChip = document.createElement("span");
            exeChip.className = "chip";
            exeChip.textContent = p.exe_path.split(/[/\\]/).pop();
            exeChip.title = p.exe_path;
            chips.appendChild(exeChip);
          }
          var evidence = Array.isArray(p.evidence) ? p.evidence : [];
          for (var ri = 0; ri < Math.min(evidence.length, 4); ri++) {
            var lr = reasonChipLabel(evDetail(evidence[ri]));
            var rc = document.createElement("span");
            rc.className = "chip" + (lr[1] ? " " + lr[1] : "");
            rc.textContent = lr[0];
            rc.title = evDetail(evidence[ri]);
            chips.appendChild(rc);
          }
          if (chips.children.length > 0) main.appendChild(chips);
          card.append(box, main);
          procCards.appendChild(card);
        });
      }
      updateKillButton();
      var killStatus = document.getElementById("kill-procs-status");
      if (killStatus) {
        killStatus.textContent = "";
        killStatus.classList.add("hidden");
      }
      processBlock.classList.toggle("hidden", procFindings.length === 0);
    }

    // Ransom findings panel
    if (ransomBlock) {
      const ransomFindings = summary.ransom_findings || [];
      const ransomCards = document.getElementById("ransom-cards");
      const ransomLink = document.getElementById("ransom-link");
      if (ransomCards) {
        ransomCards.innerHTML = "";
        let hasFamily = false;
        ransomFindings.forEach(function (r) {
          const card = document.createElement("div");
          card.className = "entry-card";
          let html =
            '<div class="card-header"><span class="card-name">' +
            escHtml(
              r.finding_type === "ransom-note"
                ? "Ransom note"
                : "Bulk encryption detected",
            ) +
            '</span></div><div class="card-detail">' +
            escHtml(r.detail) +
            "</div>";
          if (r.suspected_family) {
            html +=
              '<div class="card-detail" style="color:#c9a0ff">Suspected family: ' +
              escHtml(r.suspected_family) +
              "</div>";
            hasFamily = true;
          }
          card.innerHTML = html;
          ransomCards.appendChild(card);
        });
        if (ransomLink) {
          ransomLink.classList.toggle("hidden", !hasFamily);
        }
      }
      ransomBlock.classList.toggle("hidden", ransomFindings.length === 0);
    }

    // The pill renders from the one status model, so it can never disagree
    // with the Overview or the hero.
    restorePill();

    syncCanaryStatus();
    enablePostScanNav();
    renderOverview(summary);
    renderProcesses(summary);
    if (!resultsView.classList.contains("hidden"))
      focusViewHeading(resultsView);
  }

  // A newer navigation always wins: a queued view change from an earlier
  // click must never reveal a view the operator has already left.
  let viewToken = 0;

  /* ---------- Rescue inline results console -------------------------------
     UI-03: a Rescue-origin scan reports where it was started. The console
     below the workstation renders the same authoritative summary the
     Investigate view receives, in compact form. It never turns a coverage
     gap into an all-clear, and it never labels the checked-entry count as
     a score. */
  function clearRescueResult() {
    const panel = document.getElementById("rescue-results");
    if (!panel) return;
    panel.classList.add("hidden");
    const stats = document.getElementById("xr-result-stats");
    const notes = document.getElementById("xr-result-notes");
    const findings = document.getElementById("xr-result-findings");
    const empty = document.getElementById("xr-result-empty");
    if (stats) stats.replaceChildren();
    if (notes) notes.replaceChildren();
    if (findings) findings.replaceChildren();
    if (empty) {
      empty.classList.add("hidden");
      empty.textContent = "";
    }
  }

  function xrStat(label, value, tone) {
    const li = document.createElement("li");
    const l = makeText("span", label, "xr-stat-label");
    const v = makeText("span", value, "xr-stat-value");
    if (tone) v.classList.add(tone);
    li.append(l, v);
    return li;
  }

  function renderRescueResult(summary, errorText) {
    const panel = document.getElementById("rescue-results");
    if (!panel) return;
    const title = document.getElementById("xr-results-title");
    const sub = document.getElementById("xr-result-sub");
    const stats = document.getElementById("xr-result-stats");
    const notesEl = document.getElementById("xr-result-notes");
    const findingsEl = document.getElementById("xr-result-findings");
    const empty = document.getElementById("xr-result-empty");
    if (!title || !stats || !notesEl || !findingsEl || !empty) return;

    stats.replaceChildren();
    notesEl.replaceChildren();
    findingsEl.replaceChildren();
    empty.classList.add("hidden");
    empty.textContent = "";
    panel.classList.remove("hidden");

    if (!summary) {
      title.textContent = "Collection failed";
      sub.textContent =
        "This run produced no result. Nothing from an earlier scan is " +
        "presented here as this session's evidence.";
      stats.append(
        xrStat("Assessment", "No result", "is-bad"),
        xrStat("Coverage", "not measured"),
        xrStat("Entries checked", "0"),
        xrStat("Duration", fmtDuration(scanDurationMs)),
      );
      const li = document.createElement("li");
      li.className = "is-bad";
      li.append(
        makeText("span", "Error", "xr-note-area"),
        makeText("span", errorText || "The scan did not finish.", ""),
      );
      notesEl.append(li);
      return;
    }

    const counts = troubleCounts(summary);
    const states = summary.source_states || [];
    const verdict = window.CureSweep ? window.CureSweep.getVerdict() : null;
    const unreported = verdict ? verdict.unreported : 0;
    const denied = verdict ? verdict.denied : 0;
    const failed = verdict ? verdict.failed : 0;
    const partialLayers = verdict ? verdict.partial : 0;
    /* A gap the backend actually reported (unreadable, failed or partially
       enumerated). "Unreported" layers are a reporting limitation, not a
       failure, and are worded separately below. */
    const hardGap = denied + failed + partialLayers > 0;
    const incomplete =
      hardGap || unreported > 0 || coverageIncomplete(summary);
    const trouble = states.filter((row) => {
      const info = sourceStatusInfo(row.state);
      return info.level !== "ok";
    });

    if (verdict) {
      title.textContent =
        counts.findings > 0 && hardGap
          ? verdict.headline + " · coverage incomplete"
          : verdict.headline;
    } else {
      title.textContent =
        counts.findings > 0
          ? "Findings for review"
          : incomplete
            ? "No findings observed · coverage incomplete"
            : "No findings observed";
    }

    sub.textContent =
      counts.total +
      " entries checked in " +
      fmtDuration(scanDurationMs) +
      " · " +
      (verdict ? verdict.covered : 0) +
      " of " +
      (verdict ? verdict.total : 9) +
      " layers report coverage";

    const coverageTone = incomplete ? "is-warn" : "is-ok";
    stats.append(
      xrStat(
        "Layers checked",
        (verdict ? verdict.covered : 0) + " / " + (verdict ? verdict.total : 9),
        coverageTone,
      ),
      xrStat("Entries checked", String(counts.total)),
      xrStat(
        "Findings for review",
        String(counts.findings),
        counts.findings > 0
          ? counts.critical > 0
            ? "is-bad"
            : "is-warn"
          : null,
      ),
      xrStat("Duration", fmtDuration(scanDurationMs)),
    );

    // Every reason the coverage is not complete, with the backend's own text.
    for (const row of trouble) {
      const info = sourceStatusInfo(row.state);
      const li = document.createElement("li");
      li.className = info.level === "bad" ? "is-bad" : "";
      li.append(
        makeText("span", info.label, "xr-note-area"),
        makeText(
          "span",
          row.area + " — " + (row.detail || info.detail),
          "",
        ),
      );
      notesEl.append(li);
    }
    if (unreported > 0) {
      const li = document.createElement("li");
      li.append(
        makeText("span", "Not reported", "xr-note-area"),
        makeText(
          "span",
          unreported +
            " collectors ran but the backend sends no per-area coverage row " +
            "for them, so they are not counted as checked.",
          "",
        ),
      );
      notesEl.append(li);
    }

    // Findings: the same entries Investigate will show, ranked by risk.
    const review = (summary.suspicious_for_review || []).slice();
    review.sort((a, b) => {
      const rank = (e) => (e.risk === "HighRisk" ? 0 : 1);
      return rank(a) - rank(b) || (b.score || 0) - (a.score || 0);
    });
    const procs = summary.process_findings || [];
    const ransom = summary.ransom_findings || [];
    const shown = review.slice(0, 4);
    for (const entry of shown) {
      const li = document.createElement("li");
      li.className = "xr-finding";
      li.append(
        makeText("span", entry.name || "Unnamed entry", "xr-finding-name"),
        makeText(
          "span",
          entry.location || entry.command || "",
          "xr-finding-where",
        ),
        makeText(
          "span",
          entry.risk === "HighRisk" ? "High risk" : "Suspicious",
          "xr-finding-risk" + (entry.risk === "HighRisk" ? " is-high" : ""),
        ),
      );
      findingsEl.append(li);
    }
    for (const proc of procs.slice(0, 2)) {
      const li = document.createElement("li");
      li.className = "xr-finding";
      li.append(
        makeText("span", "Process · " + proc.name, "xr-finding-name"),
        makeText(
          "span",
          "pid " + proc.pid + " · " + (proc.exe_path || ""),
          "xr-finding-where",
        ),
        makeText(
          "span",
          proc.risk === "HighRisk" ? "High risk" : "Suspicious",
          "xr-finding-risk" + (proc.risk === "HighRisk" ? " is-high" : ""),
        ),
      );
      findingsEl.append(li);
    }
    for (const r of ransom.slice(0, 2)) {
      const li = document.createElement("li");
      li.className = "xr-finding";
      li.append(
        makeText(
          "span",
          r.finding_type === "ransom-note" ? "Ransom note" : "Bulk encryption",
          "xr-finding-name",
        ),
        makeText("span", r.detail || r.path || "", "xr-finding-where"),
        makeText("span", "Review", "xr-finding-risk is-high"),
      );
      findingsEl.append(li);
    }
    const hiddenCount =
      counts.findings - (shown.length + Math.min(procs.length, 2) + Math.min(ransom.length, 2));
    if (hiddenCount > 0) {
      const li = document.createElement("li");
      li.className = "xr-finding";
      li.append(
        makeText(
          "span",
          hiddenCount + " more finding(s) in Investigate",
          "xr-finding-name",
        ),
        makeText("span", "", "xr-finding-where"),
        makeText("span", "Open", "xr-finding-risk"),
      );
      findingsEl.append(li);
    }

    if (counts.findings === 0) {
      empty.classList.remove("hidden");
      if (hardGap) {
        empty.textContent =
          "No findings were observed within the inspected scope. " +
          "At least one area could not be read, so this is not evidence " +
          "that the machine is clean.";
      } else if (unreported > 0) {
        empty.textContent =
          "No findings were observed in the areas that reported. " +
          unreported +
          " collector(s) ran without a per-area coverage report, so this " +
          "is not evidence that the machine is clean.";
      } else if (incomplete) {
        empty.textContent =
          "No findings were observed within the inspected scope. Coverage " +
          "was incomplete, so this is not evidence that the machine is clean.";
      } else {
        empty.textContent = "No findings were observed in any inspected area.";
      }
    }
  }

  function switchView(fromEl, toEl) {
    const token = ++viewToken;
    return new Promise((resolve) => {
      if (REDUCED || fromEl === toEl) {
        fromEl.classList.add("hidden");
        toEl.classList.remove("hidden", "exiting", "pre-enter");
        resolve();
        return;
      }
      fromEl.classList.add("exiting");
      setTimeout(() => {
        if (token !== viewToken) {
          fromEl.classList.remove("exiting");
          resolve();
          return;
        }
        fromEl.classList.add("hidden");
        fromEl.classList.remove("exiting");
        toEl.classList.remove("hidden");
        toEl.classList.add("pre-enter");
        void toEl.offsetWidth;
        toEl.classList.remove("pre-enter");
        setTimeout(resolve, 180);
      }, 180);
    });
  }

  function currentView() {
    const views = document.querySelectorAll(".stage > .view");
    for (const v of views) {
      if (!v.classList.contains("hidden")) return v;
    }
    return landingView;
  }

  const VIEW_TITLES = {
    "view-overview": "Overview",
    "view-agents": "AI Agent Radar",
    "scan-center": "Rescue",
    "scan-view": "Rescue",
    "landing-view": "Rescue",
    "results-view": "Investigate",
    "view-quarantine": "Quarantine",
    "view-detective": "Detective Mode",
    "cleanup-view": "Storage Workstation",
    "view-eventlog": "Monitor",
    "view-canary": "Monitor / Canary",
    "view-incident": "Monitor / Login Observation",
  };

  function setNav(viewId) {
    const items = document.querySelectorAll(".nav-item");
    items.forEach((item) => {
      const target = item.getAttribute("data-view");
      const active =
        target === viewId ||
        (target === "view-eventlog" &&
          ["view-canary", "view-incident"].includes(viewId)) ||
        (target === "scan-center" &&
          (viewId === "scan-view" || viewId === "landing-view"));
      item.setAttribute("aria-current", active ? "true" : "false");
    });
    document
      .querySelectorAll(".tool-nav [data-route]")
      .forEach((btn) =>
        btn.setAttribute("aria-current", String(btn.dataset.route === viewId)),
      );
    const title = document.getElementById("view-title");
    if (title) title.textContent = VIEW_TITLES[viewId] || "C.U.R.E";
  }

  function showViewInstant(el) {
    viewToken++;
    const views = document.querySelectorAll(".stage > .view");
    views.forEach((v) => {
      if (v === el) v.classList.remove("hidden", "exiting", "pre-enter");
      else v.classList.add("hidden");
    });
    if (el && el.id) setNav(el.id);
    focusViewHeading(el);
  }

  // SPA focus management: view changes move focus to the view heading so
  // screen-reader users land on the new content. Visual focus ring stays
  // on :focus-visible only; mouse users see no change.
  function focusViewHeading(viewEl) {
    if (!viewEl || !viewEl.querySelector) return;
    const h = viewEl.querySelector(".headline");
    if (!h) return;
    if (!h.hasAttribute("tabindex")) h.setAttribute("tabindex", "-1");
    try {
      h.focus({ preventScroll: true });
    } catch (_) {
      h.focus();
    }
  }

  // Sidebar navigation. The Storage Workstation is an ordinary view now: no
  // open/close ceremony, no pill save/restore, nothing to unwind.
  // Stage UI-04: Forensic Investigation Lens State Machine
  function switchInvestigateLens(lensName) {
    document.querySelectorAll(".inv-lens-btn").forEach((btn) => {
      const active = btn.dataset.lens === lensName;
      btn.classList.toggle("is-active", active);
      btn.setAttribute("aria-selected", String(active));
    });
    document.querySelectorAll(".inv-lens-panel").forEach((panel) => {
      panel.classList.toggle("hidden", panel.id !== `lens-panel-${lensName}`);
    });
  }

  async function navTo(viewId) {
    if (viewId === "scan-center") {
      showViewInstant(scanView);
      if (window.CureSweep) window.CureSweep.layout();
      return;
    }
    const el = document.getElementById(viewId);
    if (!el) return;
    showViewInstant(el);
    if (viewId === "view-overview") {
      // Render from the live session state, never from a stale snapshot.
      if (["running", "preparing"].includes(scanPhase)) renderOverviewLive();
      else if (scanPhase === "failed") renderOverview(null);
      else if (lastSummary) renderOverview(lastSummary);
      else renderOverviewIdle();
      loadOperatorPosture();
    }
    if (viewId === "results-view") {
      switchInvestigateLens("findings");
      if (lastSummary) renderResults(lastSummary);
    }
    if (viewId === "view-agents" && window.CureAgentRadar) {
      window.CureAgentRadar.refresh();
    }
    if (viewId === "view-detective" && window.CureDetective) {
      /* Re-render from the investigation already in memory; opening the
         board never starts a measurement. */
      window.CureDetective.render();
    }
    if (viewId === "view-quarantine") refreshQuarantine();
    if (viewId === "view-eventlog") renderEventLog();
    if (viewId === "cleanup-view" && window.CureStorageWorkstation) {
      /* A measurement adopted from Detective Mode must survive the trip
         here. Only reset when there is no measurement to keep. */
      if (
        window.CureStorageWorkstation.state &&
        window.CureStorageWorkstation.state.scan
      ) {
        window.CureStorageWorkstation.settle();
      } else {
        window.CureStorageWorkstation.idle();
      }
      window.CureStorageWorkstation.redraw();
    }
    document
      .querySelectorAll("[data-route]")
      .forEach((btn) =>
        btn.setAttribute("aria-current", String(btn.dataset.route === viewId)),
      );
    if (viewId === "view-canary") {
      renderCanaryView();
      syncCanaryStatus();
    }
  }

  function enablePostScanNav() {
    // The Investigate entry unlocks once a scan has something to show. (The
    // old nav-audit/nav-processes sidebar items died with the Stage UI-06
    // dead-section removal — those destinations are Investigate lenses now.)
    const btn = document.getElementById("nav-results");
    if (btn) btn.disabled = false;
  }

  /* Scan lifecycle ownership. One state machine (scanPhase + scanToken),
     one origin: "rescue" keeps the historical Rescue workflow (scan view,
     then results); "overview" keeps the operator on the Overview and lets
     the hero, dial and timeline carry the live state. Manual navigation
     always wins: completion only moves the shell when the user has not
     navigated since the scan flow last owned the view. */
  let scanOrigin = "rescue";
  // Latest collector activity, from real scan-progress events only.
  let scanProgressLine = "";

  // Public entry point: refuses to start while any scan operation is
  // preparing or running, so repeated clicks can never stack scans.
  function runScan(preLines = [], origin = "rescue") {
    if (scanPhase === "running" || scanPhase === "preparing") return;
    return _runScan(preLines, origin, viewToken);
  }

  async function _runScan(preLines = [], origin = "rescue", navBaseline = viewToken) {
    scanOrigin = origin === "overview" ? "overview" : "rescue";
    const overviewScanBtn = document.getElementById("overview-rescue-btn");
    if (overviewScanBtn) {
      overviewScanBtn.disabled = true;
    }
    // A new run must never inherit the previous run's result surface.
    clearRescueResult();
    setScanControls("running");
    document.getElementById("scan-retry").classList.add("hidden");
    evidenceCache.clear();
    const token = ++scanToken;
    scanPhase = "running";
    scanStartedAt = performance.now();
    scanReportedAreas.clear();
    scanProgressLine = "";
    if (scanOrigin === "rescue" && viewToken === navBaseline) {
      // The scan flow may only take the operator to the scan view when
      // they have not navigated somewhere else while the overlay check
      // was preparing. A navigation during "preparing" wins.
      showViewInstant(scanView);
      setNav("scan-view");
    }
    logList.innerHTML = "";
    itemFeedCount = 0;
    hideReceipt();
    if (feedCountEl) feedCountEl.textContent = "0 ITEMS";
    const elapsedEl = document.getElementById("scan-elapsed");
    if (elapsedEl) elapsedEl.textContent = "elapsed 0.0s";
    const elapsedTimer = setInterval(() => {
      if (token !== scanToken) {
        clearInterval(elapsedTimer);
        return;
      }
      const secs = ((performance.now() - scanStartedAt) / 1000).toFixed(1);
      if (elapsedEl) elapsedEl.textContent = "elapsed " + secs + "s";
      updateOverviewScanLine();
    }, 500);
    for (const line of Array.isArray(preLines) ? preLines : []) {
      appendLog("overlay", line);
    }
    setPill("scanning", "Collecting evidence");
    if (scanOrigin === "overview") renderOverviewLive();
    window.CureSweep.start();
    try {
      const summary = await invoke("run_auto_scan");
      if (token !== scanToken) return;
      scanDurationMs = Math.round(performance.now() - scanStartedAt);
      clearInterval(elapsedTimer);
      // The readout cell is narrow: duration only. The entry count lives in
      // the adjacent ENTRIES INSPECTED cell and in the results console.
      if (elapsedEl) elapsedEl.textContent = "finished in " + fmtDuration(scanDurationMs);
      lastSummary = summary;
      lastScanAt = new Date();
      scanPhase = "done";
      logEvent(
        "info",
        "scan finished: " +
          summary.total +
          " checks in " +
          fmtDuration(scanDurationMs),
      );
      appendLog("done", summary.total + " entries processed — scan finished");
      window.CureSweep.finish(summary);
      renderResults(summary);
      setScanControls("done", summary);
      renderRescueResult(summary);
      if (overviewScanBtn) overviewScanBtn.disabled = false;
      // UI-03: a Rescue-origin scan no longer drags the operator to the
      // Investigate view. The Rescue workflow keeps the workstation and
      // renders its assessment inline; "Review findings" is the explicit
      // way out. (Stage UI-01A navigation ownership is preserved: an
      // Overview-origin scan still never moves the operator, and manual
      // navigation is never overridden.)
    } catch (err) {
      clearInterval(elapsedTimer);
      window.CureSweep.fail(String(err));
      document.getElementById("scan-retry").classList.remove("hidden");
      scanPhase = "failed";
      setPill("error", "Operation failed");
      appendLog("error", String(err));
      const xrLog = document.getElementById("xr-log");
      if (xrLog) xrLog.open = true;
      logEvent("info", "scan failed: " + String(err));
      if (overviewScanBtn) overviewScanBtn.disabled = false;
      // The failed attempt owns this duration; the previous run's number
      // must never be presented as this run's elapsed time.
      scanDurationMs = Math.round(performance.now() - scanStartedAt);
      setScanControls("failed", null, String(err));
      renderRescueResult(null, String(err));
      // The failure is the current session state: never dress the previous
      // summary up as this operation's result.
      if (scanOrigin === "overview") renderOverview(null);
    }
  }

  /* Primary scan controls live inside the scanner workspace. The label is
     always derived from the real lifecycle phase, never from a timer. */
  function setScanControls(phase, summary, error) {
    const start = document.getElementById("xray-start-btn");
    const retry = document.getElementById("scan-retry");
    const review = document.getElementById("xray-investigate-btn");
    const note = document.getElementById("xr-actions-note");
    if (start) {
      start.disabled = phase === "running";
      start.textContent =
        phase === "running"
          ? "Scanning…"
          : phase === "done" || phase === "failed"
            ? "Scan Again"
            : "Start System Scan";
    }
    if (retry) retry.classList.toggle("hidden", phase !== "failed");
    if (review) {
      const found = summary ? troubleCounts(summary).findings : 0;
      review.classList.toggle("hidden", !(phase === "done" && found > 0));
      review.textContent =
        found > 0 ? "Review findings (" + found + ")" : "Review findings";
    }
    if (note) {
      if (phase === "running") {
        note.textContent =
          "Collecting. The radar and the layer matrix only move when the backend reports.";
      } else if (phase === "done") {
        note.textContent =
          "Scan finished. Nothing was changed — review each finding before acting.";
      } else if (phase === "failed") {
        note.textContent =
          "The last scan did not finish: " +
          (error ? error : "no result is claimed for this run.");
      } else {
        note.textContent = "Idle — no scan has run in this session.";
      }
    }
  }

  listen("scan-progress", (event) => {
    const payload = event.payload;
    window.CureSweep.progress(payload);
    if (payload.stage === "source-state") {
      // Real progress only: a segment lights when its layer has actually
      // reported, never on a timer.
      scanReportedAreas.add(payload.area);
      scanProgressLine = payload.area;
      paintScanningDial();
      updateOverviewScanLine();
      const info = sourceStatusInfo(payload.state);
      appendLog(
        "coverage",
        payload.area + " · " + info.label + " · " + payload.detail,
      );
      return;
    }
    if (payload.stage === "item-scanned") {
      appendItemLine(payload);
      return;
    }
    if (payload.stage === "process-flagged") {
      appendProcessLine(payload);
      logEvent(
        "info",
        "process flagged: " +
          payload.name +
          " (pid " +
          payload.pid +
          ", " +
          payload.risk +
          " " +
          payload.score +
          ")",
      );
      return;
    }
    if (payload.stage === "ransom-found") {
      appendRansomLine(payload);
      logEvent(
        "canary",
        "ransom indicator: " +
          (payload.finding_type || "unknown") +
          " — " +
          (payload.detail || payload.path || ""),
      );
      return;
    }
    if (payload.message) {
      // The backend's own stage message names the collector at work.
      scanProgressLine = payload.message;
      updateOverviewScanLine();
    }
    setPill("scanning", payload.message);
    if (payload.stage === "done") {
      logEvent("info", payload.message);
      appendLog(payload.stage, payload.message);
      return;
    }
    logEvent("info", "[" + payload.stage + "] " + payload.message);
    appendLog(payload.stage, payload.message);
  });

  // Live scanning line on the Overview hero: elapsed time from the real
  // scan start, and the collector the backend last reported work in.
  // Called from the runScan ticker and progress events; renders nothing
  // unless an Overview-origin scan is actually running.
  function updateOverviewScanLine() {
    if (!["running", "preparing"].includes(scanPhase)) return false;
    const scope = document.getElementById("ov-scope");
    if (!scope) return false;
    const ov = document.getElementById("view-overview");
    if (!ov || ov.classList.contains("hidden")) return false;
    const secs = ((performance.now() - scanStartedAt) / 1000).toFixed(1);
    scope.textContent =
      (scanProgressLine ? "Collecting — " + scanProgressLine : "Preparing collection") +
      " · elapsed " +
      secs +
      "s";
    return true;
  }

  document.getElementById("rescan-btn").addEventListener("click", runScan);
  document
    .getElementById("action-receipt-x")
    .addEventListener("click", hideReceipt);

  const footMsg = document.getElementById("footbar-msg");
  let footTimer = null;
  function footFeedback(text, isError) {
    footMsg.textContent = text;
    footMsg.classList.toggle("error", !!isError);
    footMsg.classList.add("show");
    clearTimeout(footTimer);
    // Durable enough to read; the same outcome is also kept in the Event
    // Log and the relevant view (receipt/status line), never toast-only.
    footTimer = setTimeout(() => footMsg.classList.remove("show"), 6000);
  }

  function cleanErrText(err, fallback) {
    const s = String(err == null ? "" : err)
      .replace(/^Error:\s*/i, "")
      .trim();
    return s || fallback;
  }

  // ---- shared destructive-action confirm dialog ----
  // requestConfirm({kicker,title,facts:[[label,value]...],note,okLabel})
  // resolves true only on explicit Confirm. Escape, backdrop click, and
  // Cancel resolve false. Focus starts on Cancel, stays inside while open,
  // and returns to the invoking control afterwards.
  let confirmOpen = false;
  function requestConfirm(opts) {
    if (confirmOpen) return Promise.resolve(false);
    const overlay = document.getElementById("confirm-overlay");
    const kicker = document.getElementById("confirm-kicker");
    const title = document.getElementById("confirm-title");
    const facts = document.getElementById("confirm-facts");
    const note = document.getElementById("confirm-note");
    const cancelBtn = document.getElementById("confirm-cancel");
    const okBtn = document.getElementById("confirm-ok");
    if (!overlay || !cancelBtn || !okBtn) return Promise.resolve(false);
    const invoker = document.activeElement;
    kicker.textContent = opts.kicker || "Confirm action";
    title.textContent = opts.title || "Are you sure?";
    facts.innerHTML = "";
    for (const [label, value] of opts.facts || []) {
      const dt = document.createElement("dt");
      dt.textContent = label;
      const dd = document.createElement("dd");
      dd.textContent = value;
      facts.append(dt, dd);
    }
    document
      .querySelectorAll("#confirm-overlay .confirm-section")
      .forEach((n) => n.remove());
    for (const [label, text] of opts.sections || []) {
      const box = makeText("section", "", "confirm-section");
      box.append(makeText("h3", label), makeText("p", text));
      note.before(box);
    }
    note.textContent = opts.note || "";
    note.classList.toggle("hidden", !opts.note);
    okBtn.textContent = opts.okLabel || "Confirm";
    confirmOpen = true;
    overlay.classList.remove("hidden");
    const inertTargets = [
      document.getElementById("app"),
      document.getElementById("evidence-inspector"),
    ];
    const priorInert = inertTargets.map((n) => n.inert);
    inertTargets.forEach((n) => (n.inert = true));
    return new Promise((resolve) => {
      const done = (value) => {
        confirmOpen = false;
        overlay.classList.add("hidden");
        inertTargets.forEach((n, i) => (n.inert = priorInert[i]));
        overlay.removeEventListener("mousedown", onBackdrop);
        document.removeEventListener("keydown", onKey, true);
        if (invoker && invoker.focus) invoker.focus();
        resolve(value);
        if (canaryQueued) showCanaryAlert();
      };
      const onBackdrop = (ev) => {
        if (ev.target === overlay) done(false);
      };
      const onKey = (ev) => {
        if (ev.key === "Escape") {
          ev.stopPropagation();
          done(false);
        } else if (ev.key === "Tab") {
          // Minimal two-button trap: keep Tab cycling inside the dialog.
          ev.preventDefault();
          (document.activeElement === cancelBtn ? okBtn : cancelBtn).focus();
        }
      };
      overlay.addEventListener("mousedown", onBackdrop);
      document.addEventListener("keydown", onKey, true);
      cancelBtn.onclick = () => done(false);
      okBtn.onclick = () => done(true);
      cancelBtn.focus();
    });
  }

  document
    .getElementById("btn-quarantine-folder")
    .addEventListener("click", async () => {
      try {
        await invoke("open_quarantine_folder");
        footFeedback("Quarantine folder opened", false);
      } catch (err) {
        footFeedback(
          cleanErrText(err, "Could not open quarantine folder"),
          true,
        );
      }
    });
  document
    .getElementById("btn-view-log")
    .addEventListener("click", async () => {
      try {
        await invoke("view_log");
        footFeedback("Scan log opened", false);
      } catch (err) {
        footFeedback(cleanErrText(err, "Could not open scan log"), true);
      }
    });
  document.getElementById("btn-exit").addEventListener("click", async () => {
    try {
      await invoke("exit_app");
    } catch (err) {
      footFeedback(cleanErrText(err, "Could not exit app"), true);
    }
  });

  // ---- canary guard (ransomware decoy monitor) ----------------------------

  let canaryActive = false;

  async function syncCanaryStatus() {
    try {
      const st = await invoke("canary_status");
      canaryActive = !!st.active;
      updateCanaryUI();
    } catch (err) {
      const badge = document.getElementById("can-state");
      if (badge) {
        badge.textContent = "Unavailable";
        badge.className = "severity-badge sev-warn";
      }
      footFeedback(
        cleanErrText(
          err,
          "Canary status unavailable. Reopen Monitor to retry.",
        ),
        true,
      );
    }
  }

  function updateCanaryUI() {
    const note = document.getElementById("m-guard-note");
    if (note) {
      note.textContent = canaryActive
        ? "Decoys planted — watching until you disable it."
        : "Enabling plants decoys and watches them for this session.";
    }
    const toggle = document.getElementById("can-toggle");
    if (toggle) {
      toggle.setAttribute("aria-pressed", String(canaryActive));
      toggle.classList.toggle("on", canaryActive);
      toggle.textContent = canaryActive ? "Disable guard" : "Enable guard";
    }
    const badge = document.getElementById("can-state");
    if (badge) {
      if (canaryTriggered) {
        badge.textContent = canaryActive
          ? "ACTIVE · ALERT RECORDED"
          : "OFF · ALERT RECORDED";
        badge.className = "severity-badge sev-bad";
      } else if (canaryActive) {
        badge.textContent = "ACTIVE";
        badge.className = "severity-badge sev-info";
      } else {
        badge.textContent = "OFF";
        badge.className = "severity-badge";
      }
    }
  }

  async function toggleCanary() {
    try {
      if (canaryActive) {
        await invoke("stop_canary_guard");
        canaryActive = false;
        footFeedback("Canary guard deactivated", false);
        logEvent("action", "canary guard deactivated");
      } else {
        await invoke("start_canary_guard");
        canaryActive = true;
        footFeedback("Canary guard active — decoys planted", false);
        logEvent("action", "canary guard activated — decoys planted");
      }
    } catch (err) {
      footFeedback(cleanErrText(err, "Could not toggle canary guard"), true);
    }
    updateCanaryUI();
  }

  // Listen for canary-alert events from backend
  const canaryOverlay = document.getElementById("canary-alert-overlay");
  const canaryDetail = document.getElementById("canary-alert-detail");
  const canaryDismissBtn = document.getElementById("canary-dismiss-btn");

  // Canary alert dialog: modal while visible. Escape dismisses, focus
  // moves to Dismiss on open and returns to the invoker on close, and
  // Tab is kept on the dialog's single action while open.
  let canaryInvoker = null;
  let canaryQueued = false;
  function canaryKeyTrap(ev) {
    if (ev.key === "Escape") {
      ev.stopPropagation();
      hideCanaryAlert();
    } else if (ev.key === "Tab") {
      ev.preventDefault();
      if (canaryDismissBtn) canaryDismissBtn.focus();
    }
  }
  function showCanaryAlert() {
    if (!canaryOverlay || !canaryOverlay.classList.contains("hidden")) return;
    if (
      confirmOpen ||
      evidenceSelected ||
      !document
        .getElementById("inc-drawer-overlay")
        .classList.contains("hidden")
    ) {
      canaryQueued = true;
      return;
    }
    canaryQueued = false;
    canaryInvoker = document.activeElement;
    canaryOverlay.classList.remove("hidden");
    document.getElementById("app").inert = true;
    document.addEventListener("keydown", canaryKeyTrap, true);
    if (canaryDismissBtn) canaryDismissBtn.focus();
  }
  function hideCanaryAlert() {
    if (!canaryOverlay) return;
    canaryOverlay.classList.add("hidden");
    document.getElementById("app").inert = false;
    document.removeEventListener("keydown", canaryKeyTrap, true);
    if (canaryInvoker && canaryInvoker.focus) canaryInvoker.focus();
    canaryInvoker = null;
  }

  if (canaryDismissBtn) {
    canaryDismissBtn.addEventListener("click", hideCanaryAlert);
  }

  TAU.event.listen("canary-alert", (ev) => {
    const payload =
      typeof ev.payload === "string" ? JSON.parse(ev.payload) : ev.payload;
    const kind = payload.kind || "unknown";
    const folder = payload.folder || "";
    const file = payload.file || "";
    const action = payload.action || "";
    const text =
      kind.replace(/-/g, " ").toUpperCase() +
      " — " +
      (folder ? folder + " " : "") +
      file +
      (action ? " (" + action + ")" : "");
    canaryTriggered = true;
    canarySessionAlerts.push({ at: new Date(), text });
    if (canarySessionAlerts.length > 100) canarySessionAlerts.shift();
    logEvent("canary", "canary alert: " + text);
    appendCanaryAlertRow({ at: new Date(), text });
    updateCanaryUI();
    if (canaryDetail) {
      canaryDetail.textContent = text;
    }
    showCanaryAlert();
  });

  // ---- byte formatting shared by the overview and the storage rail -------

  function fmtBytes(n) {
    if (n >= 1073741824) return (n / 1073741824).toFixed(1) + " GB";
    if (n >= 1048576) return (n / 1048576).toFixed(1) + " MB";
    if (n >= 1024) return Math.round(n / 1024) + " KB";
    return n + " B";
  }

  const sweepState = {
    findings: [],
    checked: new Set(),
    running: false,
  };

  function updateKillButton() {
    const btn = document.getElementById("kill-procs-btn");
    if (!btn) return;
    const enabled = sweepState.checked.size > 0 && !sweepState.running;
    btn.disabled = !enabled;
    btn.textContent = "Terminate selected (" + sweepState.checked.size + ")";
  }

  var killBtn = document.getElementById("kill-procs-btn");
  var killStatus = document.getElementById("kill-procs-status");
  if (killBtn) {
    killBtn.addEventListener("click", async function () {
      if (sweepState.running || killBtn.disabled) return;
      var targets = [];
      sweepState.findings.forEach(function (f) {
        if (sweepState.checked.has(f.pid)) targets.push([f.name, f.pid]);
      });
      if (targets.length === 0) return;
      const ok = await requestConfirm({
        kicker: "Confirm process termination",
        title: "Terminate " + targets.length + " selected process(es)?",
        facts: [
          [
            "Targets",
            targets
              .map(function (t) {
                return t[0] + " (pid " + t[1] + ")";
              })
              .join(", "),
          ],
          ["Action", "Terminate the selected processes immediately."],
          [
            "Reversible",
            "No — termination cannot be undone, but nothing is deleted.",
          ],
        ],
        note: "PIDs are re-validated before termination. Re-run a scan afterwards to verify.",
        okLabel: "Terminate " + targets.length + " process(es)",
      });
      if (!ok) return;
      sweepState.running = true;
      killBtn.disabled = true;
      killBtn.classList.add("btn-active");
      killBtn.textContent = "Killing…";
      if (killStatus) {
        killStatus.textContent = "";
        killStatus.classList.add("hidden");
      }
      try {
        var report = await invoke("kill_high_risk_processes", {
          processes: targets,
        });
        var killed = report.killed || [];
        var failed = report.failed || [];
        if (killed.length > 0) {
          var pidSet = new Set(
            killed.map(function (k) {
              return k.pid;
            }),
          );
          var cards = document.querySelectorAll(
            "#process-cards .review-card.proc-entry",
          );
          cards.forEach(function (card) {
            if (pidSet.has(Number(card.dataset.pid))) {
              card.classList.add("proc-killed");
              var cb = card.querySelector('input[type="checkbox"]');
              if (cb) cb.disabled = true;
            }
          });
        }
        sweepState.checked.clear();
        var parts = [];
        if (killed.length > 0)
          parts.push("Killed " + killed.length + " process(es)");
        if (failed.length > 0) parts.push(failed.length + " failed");
        var msg = parts.join(", ") || "No processes were killed";
        setPill(
          killed.length > 0 && failed.length === 0 ? "clean" : "warn",
          msg,
        );
        logEvent(
          "action",
          "process kill: " +
            msg +
            (failed.length ? " — " + failed.join("; ") : ""),
        );
        if (killStatus) {
          killStatus.textContent = msg;
          killStatus.classList.remove("hidden");
        }
      } catch (err) {
        setPill("error", "Kill failed: " + String(err));
        if (killStatus) {
          killStatus.textContent = "Error: " + String(err);
          killStatus.classList.remove("hidden");
        }
      } finally {
        sweepState.running = false;
        killBtn.classList.remove("btn-active");
        updateKillButton();
      }
    });
  }

  // ══════════════ UI V2 views (all data from real backend responses) ══════

  function fmtDuration(ms) {
    if (ms == null) return "—";
    const s = ms / 1000;
    if (s < 60) return s.toFixed(1) + "s";
    const m = Math.floor(s / 60);
    return m + "m " + Math.round(s % 60) + "s";
  }

  // ---- event log view ----

  function appendEventLogRow(e) {
    updateMonitorStats();
    if (!eventMatchesFilter(e.kind)) return;
    const list = document.getElementById("event-log-list");
    if (!list) return;
    list.appendChild(buildEventRow(e));
    while (list.children.length > 300) list.removeChild(list.firstChild);
    list.scrollTop = list.scrollHeight;
    const empty = document.getElementById("event-log-empty");
    if (empty) empty.classList.add("hidden");
  }

  function renderEventLog() {
    const list = document.getElementById("event-log-list");
    if (!list) return;
    list.innerHTML = "";
    for (const e of eventLog) {
      if (eventMatchesFilter(e.kind)) list.appendChild(buildEventRow(e));
    }
    const empty = document.getElementById("event-log-empty");
    if (empty)
      empty.classList.toggle(
        "hidden",
        eventLog.some((e) => eventMatchesFilter(e.kind)),
      );
    updateMonitorStats();
  }

  // Monitor Workstation feed: one filter state, live telemetry counters.
  let eventFeedFilter = "all";

  function eventMatchesFilter(kind) {
    return eventFeedFilter === "all" || kind === eventFeedFilter;
  }

  function eventKindTag(kind) {
    if (kind === "canary") return "Canary";
    if (kind === "action") return "Action";
    if (kind === "info") return "Scan";
    return kind || "Info";
  }

  function buildEventRow(e) {
    const li = document.createElement("li");
    li.className = "m-row m-row-" + (e.kind || "info");
    const time = document.createElement("span");
    time.className = "m-time";
    time.textContent = e.at.toLocaleTimeString();
    const tag = document.createElement("span");
    tag.className = "m-tag m-tag-" + (e.kind || "info");
    tag.textContent = eventKindTag(e.kind);
    const body = document.createElement("span");
    body.className = "m-text selectable";
    body.textContent = e.text;
    li.append(time, tag, body);
    return li;
  }

  function updateMonitorStats() {
    const total = document.getElementById("m-stat-events");
    const canary = document.getElementById("m-stat-canary");
    const actions = document.getElementById("m-stat-actions");
    const last = document.getElementById("m-stat-last");
    if (total) total.textContent = String(eventLog.length);
    if (canary)
      canary.textContent = String(
        eventLog.filter((e) => e.kind === "canary").length,
      );
    if (actions)
      actions.textContent = String(
        eventLog.filter((e) => e.kind === "action").length,
      );
    if (last) {
      const e = eventLog[eventLog.length - 1];
      last.textContent = e ? e.text : "—";
      last.title = e
        ? e.at.toLocaleTimeString() + " — " + e.text
        : "Nothing recorded yet this session";
    }
  }

  // ---- canary view ----

  function appendCanaryAlertRow(a) {
    const list = document.getElementById("can-alerts");
    if (!list) return;
    const li = document.createElement("li");
    li.className = "m-row m-row-canary";
    const time = document.createElement("span");
    time.className = "m-time";
    time.textContent = a.at.toLocaleTimeString();
    const tag = document.createElement("span");
    tag.className = "m-tag m-tag-canary";
    tag.textContent = "Alert";
    const body = document.createElement("span");
    body.className = "m-text selectable";
    body.textContent = a.text;
    li.append(time, tag, body);
    list.appendChild(li);
    const empty = document.getElementById("can-empty");
    if (empty) empty.classList.add("hidden");
  }

  function renderCanaryView() {
    const list = document.getElementById("can-alerts");
    if (!list) return;
    list.innerHTML = "";
    for (const a of canarySessionAlerts) appendCanaryAlertRow(a);
    const empty = document.getElementById("can-empty");
    if (empty) empty.classList.toggle("hidden", canarySessionAlerts.length > 0);
    updateCanaryUI();
  }

  // ---- overview ----

  function troubleCounts(s) {
    const cleaned = s.high_risk_cleaned.length;
    const review = s.suspicious_for_review || [];
    const procs = s.process_findings || [];
    const ransom = s.ransom_findings || [];
    const reviewHigh = review.filter((e) => e.risk === "HighRisk").length;
    const reviewSusp = review.filter((e) => e.risk === "Suspicious").length;
    const procHigh = procs.filter((p) => p.risk === "HighRisk").length;
    const procSusp = procs.filter((p) => p.risk === "Suspicious").length;
    return {
      cleaned,
      review: review.length,
      proc: procs.length,
      ransom: ransom.length,
      safe: s.safe,
      total: s.total,
      critical: reviewHigh + procHigh + ransom.length,
      suspicious: reviewSusp + procSusp,
      findings: cleaned + review.length + procs.length + ransom.length,
    };
  }

  function setMetric(id, text, tone) {
    const el = document.getElementById(id);
    if (!el) return;
    el.textContent = text;
    el.classList.remove("is-ok", "is-warn", "is-bad");
    if (tone) el.classList.add(tone);
  }

  /* ---- Overview support ----
     Every number on the Overview comes from one of three real sources:
     the scan summary (this session's collection), the operator_status
     command (live drive-pressure and agent-process enumeration), or the
     durable quarantine records. Nothing is simulated and nothing is
     carried over from a previous process as "current". */

  const OV_LAYER_TOTAL = 9;

  function setOvDial(checked, label, tone) {
    const t = tone && tone.indexOf("is-") === 0 ? tone.slice(3) : tone;
    const value = document.getElementById("ov-dial-value");
    const group = ensureDialSegments();
    if (value)
      value.textContent = checked == null ? "\u2014" : checked + "/" + OV_LAYER_TOTAL;
    if (group) {
      const segments = group.children;
      for (let i = 0; i < segments.length; i++) {
        const lit = checked != null && i < checked;
        segments[i].setAttribute(
          "class",
          "ov-dial-segment" +
            (lit ? " is-lit" : "") +
            (lit && t ? " is-" + t : ""),
        );
      }
    }
    const labelEl = document.getElementById("ov-dial-label");
    if (labelEl) labelEl.textContent = label;
  }

  // While a scan runs, the dial advances only as layers actually report.
  function paintScanningDial() {
    if (!["running", "preparing"].includes(scanPhase)) return;
    setOvDial(scanReportedAreas.size, "Layers reporting", null);
  }

  // The Overview timeline: verified session events only.
  function renderOverviewTimeline() {
    const ovActions = document.getElementById("ov-actions");
    const ovActionsEmpty = document.getElementById("ov-actions-empty");
    if (!ovActions) return;
    ovActions.replaceChildren();
    const recent = eventLog.slice(-6).reverse();
    for (const ev of recent) {
      const li = document.createElement("li");
      const time = document.createElement("span");
      time.className = "ev-time";
      try {
        time.textContent = ev.at.toLocaleTimeString();
      } catch (_) {
        time.textContent = "";
      }
      const tag = document.createElement("b");
      tag.textContent = "[" + (ev.kind || "info") + "]";
      const body = document.createElement("span");
      body.className = "selectable";
      body.textContent = ev.text || "";
      li.append(time, tag, body);
      ovActions.appendChild(li);
    }
    if (ovActionsEmpty)
      ovActionsEmpty.classList.toggle("hidden", recent.length > 0);
  }

  // Fresh process launch: a clean, neutral session. Nothing from a previous
  // process is presented as this session's result, and "not assessed" is
  // never silently promoted to "healthy".
  function renderOverviewIdle() {
    const posture = document.getElementById("ov-posture");
    if (posture) {
      posture.textContent = "Not assessed";
      posture.classList.remove("is-ok", "is-warn", "is-bad", "is-scan");
    }
    const beacon = document.getElementById("ov-beacon");
    if (beacon) beacon.className = "ov-beacon";
    const subline = document.getElementById("ov-subline");
    if (subline)
      subline.textContent =
        "Run a System Scan to establish the evidence available on this system.";
    const scope = document.getElementById("ov-scope");
    if (scope) scope.textContent = "No scan has run this session.";
    setOvDial(null, "Layers checked", null);
    setMetric("ov-assessment", "Not assessed", null);
    setMetric("ov-coverage-state", "—", null);
    setMetric("ov-findings", "—", null);
    setMetric("ov-storage", "Not measured", null);
    setMetric("ov-agents", "—", null);
    refreshQuarantineCount();
    const review = document.getElementById("ov-findings-panel");
    if (review) review.classList.add("hidden");
    const reviewBtn = document.getElementById("ov-review-btn");
    if (reviewBtn) {
      reviewBtn.disabled = true;
      reviewBtn.title = "Run a scan first — findings appear after collection";
    }
    renderIdleCoverage();
    renderOverviewTimeline();
    updateOvAside();
    renderOvRecommendation();
  }

  // A scan in flight: the Overview reports the live operation and never
  // dresses the previous scan up as this session's result.
  function renderOverviewLive() {
    const posture = document.getElementById("ov-posture");
    if (posture) {
      posture.textContent = "Scanning";
      posture.classList.remove("is-ok", "is-bad", "is-warn");
      posture.classList.add("is-scan");
    }
    const beacon = document.getElementById("ov-beacon");
    if (beacon) beacon.className = "ov-beacon scan";
    const subline = document.getElementById("ov-subline");
    if (subline)
      subline.textContent =
        "Collection is running — the dial advances as each layer reports.";
    setOvDial(scanReportedAreas.size, "Layers reporting", null);
    setMetric("ov-assessment", "Scanning", null);
    setMetric("ov-coverage-state", "In progress", null);
    setMetric("ov-findings", "In progress", null);
    // Previous findings are not this operation's result; the panel waits.
    const livePanel = document.getElementById("ov-findings-panel");
    if (livePanel) livePanel.classList.add("hidden");
    const liveBtn = document.getElementById("ov-review-btn");
    if (liveBtn)
      liveBtn.title = "Findings appear here when the scan completes";
    updateOverviewScanLine();
    renderIdleCoverage();
    renderOverviewTimeline();
  }

  // Idle and in-flight coverage: every layer is named and none is claimed
  // to have reported before it has.
  function renderIdleCoverage() {
    const cov = document.getElementById("ov-coverage");
    if (!cov) return;
    cov.replaceChildren();
    const layers = [
      "Registry autoruns",
      "Startup folder",
      "Scheduled tasks",
      "Services (auto-start)",
      "WMI subscriptions",
      "IFEO debuggers",
      "AppInit DLLs",
      "COM hijacks (HKCU)",
      "Running processes",
      "Ransom indicators",
      "Canary guard",
    ];
    for (const name of layers) {
      const detail =
        name === "Canary guard"
          ? canaryTriggered
            ? "TRIGGERED — see Canary Guard"
            : canaryActive
              ? "active — watching decoys"
              : "off"
          : "not assessed yet";
      cov.append(covRow(name, detail, name === "Canary guard" && canaryActive ? "ok" : "idle"));
    }
  }

  /* Live operator telemetry: real drive-pressure measurement and a real
     process enumeration, refreshed when the Overview is shown. These are
     availability facts about the machine right now — never a health
     verdict about the session's security assessment. */
  let operatorPosture = null;
  let quarantineVaultCount = null;
  let operatorPostureInFlight = false;
  let operatorPostureEverLoaded = false;

  async function loadOperatorPosture() {
    if (operatorPostureInFlight) return;
    operatorPostureInFlight = true;
    try {
      operatorPosture = await invoke("operator_status");
      operatorPostureEverLoaded = true;
    } catch (_) {
      operatorPosture = null;
    }
    operatorPostureInFlight = false;
    renderOperatorPosture();
  }

  /* The real backend serialises ResponseLevel as "level2_action_recommended"
     (serde snake_case of Level2ActionRecommended); the labelled mock uses
     "level_2_action_recommended". Accept both spellings. */
  const OV_LEVEL_LABELS = {
    level_0_observe: "Informational",
    level0_observe: "Informational",
    level_1_review: "Review",
    level1_review: "Review",
    level_2_action_recommended: "Action recommended",
    level2_action_recommended: "Action recommended",
    level_3_emergency_containment: "Containment",
    level3_emergency_containment: "Containment",
  };

  /* The Rescue header shows the operator engine's own recommendation level.
     It is read from the live posture, so it can never be a decorative
     "LEVEL 0" badge painted next to a machine that needs attention. */
  function renderResponseBadge() {
    const badge = document.getElementById("xray-response-badge");
    if (!badge) return;
    const labels = {
      0: "observe",
      1: "review",
      2: "action recommended",
      3: "containment",
    };
    const classes = {
      0: "level-0-observe",
      1: "level-1-review",
      2: "level-2-action",
      3: "level-3-containment",
    };
    if (operatorPosture) {
      const rank = {
        level_0_observe: 0,
        level0_observe: 0,
        level_1_review: 1,
        level1_review: 1,
        level_2_action_recommended: 2,
        level2_action_recommended: 2,
        level_3_emergency_containment: 3,
        level3_emergency_containment: 3,
      };
      let level = 0;
      for (const rec of operatorPosture.recommendations || []) {
        const r = rank[rec.level] != null ? rank[rec.level] : 0;
        if (r > level) level = r;
      }
      badge.textContent = "Level " + level + " · " + labels[level];
      badge.className = "operator-level-badge " + classes[level];
      badge.title =
        level === 0
          ? "The operator engine reports nothing above observe"
          : "Highest recommendation level reported by the operator engine";
    } else if (!operatorPostureEverLoaded) {
      badge.textContent = "Reading…";
      badge.className = "operator-level-badge";
      badge.title = "Reading the operator posture";
    } else {
      badge.textContent = "Unavailable";
      badge.className = "operator-level-badge";
      badge.title = "The operator posture could not be read";
    }
  }

  function renderOperatorPosture() {
    const p = operatorPosture;
    if (p) {
      const storage = document.getElementById("ov-storage");
      if (storage) {
        storage.textContent = p.c_drive_pressure_pct.toFixed(1) + "% used";
        storage.classList.remove("is-ok", "is-warn", "is-bad");
        storage.classList.add(p.c_drive_congested ? "is-warn" : "is-ok");
        storage.title =
          "Measured now: " + fmtBytes(p.c_drive_free_bytes) + " free on C:";
      }
      const agents = document.getElementById("ov-agents");
      if (agents) {
        agents.textContent =
          p.active_agent_count +
          " active" +
          (p.unverified_agent_count > 0
            ? " · " + p.unverified_agent_count + " unverified"
            : "");
        agents.classList.remove("is-ok", "is-warn", "is-bad");
        if (p.unverified_agent_count > 0) agents.classList.add("is-warn");
      }
    }
    updateOvAside();
    renderOvRecommendation();
    renderResponseBadge();
  }

  // Hero aside: verified context chips. Rendered only when a real
  // measurement exists; a missing measurement is stated, not faked.
  function updateOvAside() {
    const aside = document.getElementById("ov-hero-aside");
    if (!aside) return;
    aside.replaceChildren();
    const chips = [];
    if (operatorPosture) {
      chips.push({
        tone: operatorPosture.c_drive_congested ? "warn" : "ok",
        label: "C: drive",
        value: operatorPosture.c_drive_pressure_pct.toFixed(1) + "% used",
        detail: fmtBytes(operatorPosture.c_drive_free_bytes) + " free · measured now",
      });
      chips.push({
        tone: operatorPosture.unverified_agent_count > 0 ? "warn" : "ok",
        label: "Agent processes",
        value: String(operatorPosture.active_agent_count),
        detail:
          operatorPosture.unverified_agent_count > 0
            ? operatorPosture.unverified_agent_count + " unverified"
            : "enumerated live",
      });
    } else if (!operatorPostureEverLoaded) {
      chips.push({
        tone: "idle",
        label: "Live telemetry",
        value: "Reading…",
        detail: "measuring drives and agent processes",
      });
    } else {
      chips.push({
        tone: "idle",
        label: "Live telemetry",
        value: "Unavailable",
        detail: "operator posture could not be read",
      });
    }
    if (quarantineVaultCount != null) {
      chips.push({
        tone: "idle",
        label: "Quarantine vault",
        value: String(quarantineVaultCount),
        detail: "durable records · not this session",
      });
    }
    for (const c of chips) {
      const chip = makeText("div", "", "ov-chip tone-" + c.tone);
      chip.append(
        makeText("span", c.label, "ov-chip-label"),
        makeText("span", c.value, "ov-chip-value"),
        makeText("span", c.detail, "ov-chip-detail"),
      );
      aside.append(chip);
    }
  }

  /* Compact contextual recommendation. Appears only when the backend
     reports a real, measured condition, and never promises a specific
     amount of recoverable space — it states what was measured. */
  function renderOvRecommendation() {
    const panel = document.getElementById("ov-rec-panel");
    if (!panel) return;
    const recs = (operatorPosture && operatorPosture.recommendations) || [];
    if (!recs.length) {
      panel.classList.add("hidden");
      panel.replaceChildren();
      return;
    }
    const rank = {
      level_3_emergency_containment: 0,
      level_2_action_recommended: 1,
      level_1_review: 2,
      level_0_observe: 3,
    };
    const rec = recs
      .slice()
      .sort((a, b) => (rank[a.level] ?? 9) - (rank[b.level] ?? 9))[0];
    panel.replaceChildren();
    const head = makeText("div", "", "ov-rec-head");
    head.append(
      makeText("span", "Recommended action", "ov-rec-kicker"),
      makeText(
        "span",
        OV_LEVEL_LABELS[rec.level] || "Informational",
        "ov-rec-level",
      ),
    );
    panel.append(head);
    panel.append(makeText("div", rec.title, "ov-rec-title"));
    panel.append(makeText("div", rec.description, "ov-rec-desc"));
    if (rec.id === "rec_drive_pressure" && operatorPosture) {
      panel.append(
        makeText(
          "div",
          "Measured: C: " +
            operatorPosture.c_drive_pressure_pct.toFixed(1) +
            "% used · " +
            fmtBytes(operatorPosture.c_drive_free_bytes) +
            " free · " +
            fmtBytes(operatorPosture.recommended_storage_relief_bytes) +
            " in safe-to-relocate libraries",
          "ov-rec-meta",
        ),
      );
    }
    const btn = makeText("button", rec.action_label || "View action", "btn ov-rec-btn");
    btn.addEventListener("click", () => runOperatorAction(rec));
    panel.append(btn);
    panel.classList.remove("hidden");
  }

  function runOperatorAction(rec) {
    if (rec.action_type === "storage_rescue") {
      navTo("cleanup-view");
      setTimeout(() => {
        const tab = document.getElementById("sw-tab-rescue");
        if (tab) tab.click();
      }, 150);
    } else if (rec.action_type === "agent_session") {
      navTo("view-agents");
    } else {
      navTo("scan-center");
    }
  }

  /* Overview dial: the inspection layers, one segment each.

     The earlier version drew a single ring and a coverage arc, which before
     the first scan was a large empty circle. Segments say what the
     ring could not: how many of the machine's layers have actually
     reported. Ten collection surfaces since Detection Engine v2 added
     Winlogon & Active Setup. */
  const DIAL_R = 48;
  const DIAL_CX = 60;
  const DIAL_CY = 60;
  const DIAL_LAYERS = 10;
  function dialSegmentPath(index) {
    const slot = 360 / DIAL_LAYERS;
    const gap = 13;
    const a0 = ((index * slot + gap / 2 - 90) * Math.PI) / 180;
    const a1 = ((index * slot + slot - gap / 2 - 90) * Math.PI) / 180;
    const x0 = DIAL_CX + Math.cos(a0) * DIAL_R;
    const y0 = DIAL_CY + Math.sin(a0) * DIAL_R;
    const x1 = DIAL_CX + Math.cos(a1) * DIAL_R;
    const y1 = DIAL_CY + Math.sin(a1) * DIAL_R;
    return "M" + x0.toFixed(2) + " " + y0.toFixed(2) +
           " A" + DIAL_R + " " + DIAL_R + " 0 0 1 " + x1.toFixed(2) + " " + y1.toFixed(2);
  }
  function ensureDialSegments() {
    let group = document.getElementById("ov-dial-segments");
    if (!group) return null;
    if (group.childElementCount !== DIAL_LAYERS) {
      group.replaceChildren();
      for (let i = 0; i < DIAL_LAYERS; i++) {
        const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
        path.setAttribute("d", dialSegmentPath(i));
        path.setAttribute("class", "ov-dial-segment");
        group.appendChild(path);
      }
    }
    return group;
  }
  function paintOverviewDial(ovStates, tone) {
    const rows = ovStates || [];
    const checked = rows.filter(function (row) {
      const st = row.state;
      const key = typeof st === "string" ? st : Object.keys(st || {})[0];
      return key === "Checked" || key === "Available";
    }).length;
    setOvDial(rows.length ? checked : null, "Layers checked", tone);
  }

  function covRow(name, detail, level) {
    const li = document.createElement("li");
    const dot = document.createElement("span");
    dot.className = "cov-dot " + (level || "idle");
    const nm = document.createElement("span");
    nm.className = "cov-name";
    nm.textContent = name;
    const det = document.createElement("span");
    det.className = "cov-detail";
    det.textContent = detail;
    li.append(dot, nm, det);
    return li;
  }

  /* The Overview renderer. One function, fed by the one status model, so
     the hero, the metrics and the pill can never contradict one another.
     While a scan is in flight it renders the live operation instead of the
     previous scan's results. */
  function renderOverview(summary) {
    if (["running", "preparing"].includes(scanPhase)) {
      renderOverviewLive();
      return;
    }
    const posture = document.getElementById("ov-posture");
    const subline = document.getElementById("ov-subline");
    const beacon = document.getElementById("ov-beacon");
    if (scanPhase === "failed") {
      // The latest operation failed. Its result is "nothing was collected":
      // the previous summary stays available in Investigate but must not be
      // presented here as this session's current finding set.
      if (posture) {
        posture.textContent = "Operation failed";
        posture.classList.remove("is-ok", "is-warn", "is-scan");
        posture.classList.add("is-bad");
      }
      if (beacon) beacon.className = "ov-beacon bad";
      if (subline)
        subline.textContent =
          "The last scan did not finish — the error is recorded in the activity timeline. Retry from here or from Rescue.";
      setOvDial(null, "Layers checked", null);
      setMetric("ov-assessment", "Operation failed", "is-bad");
      setMetric("ov-coverage-state", "—", null);
      setMetric("ov-findings", "—", null);
      refreshQuarantineCount();
      loadOperatorPosture();
      renderOperatorPosture();
      const failedPanel = document.getElementById("ov-findings-panel");
      if (failedPanel) failedPanel.classList.add("hidden");
      const failedBtn = document.getElementById("ov-review-btn");
      if (failedBtn) {
        if (lastSummary) {
          failedBtn.disabled = false;
          failedBtn.title = "Open the findings from the last completed scan";
        } else {
          failedBtn.disabled = true;
          failedBtn.title = "No completed scan this session";
        }
      }
      const failedScope = document.getElementById("ov-scope");
      if (failedScope)
        failedScope.textContent = "No complete collection in the latest operation.";
      renderIdleCoverage();
      renderOverviewTimeline();
      return;
    }
    const t = troubleCounts(summary);
    const status = sessionStatus();
    if (posture) {
      posture.textContent = status.label;
      posture.classList.remove("is-ok", "is-warn", "is-bad", "is-scan");
      if (status.tone === "bad") posture.classList.add("is-bad");
      else if (status.tone === "warn") posture.classList.add("is-warn");
      else if (status.tone === "ok") posture.classList.add("is-ok");
    }
    if (beacon) {
      beacon.className =
        "ov-beacon" +
        (t.critical > 0
          ? " bad"
          : t.findings > 0 || coverageIncomplete(summary)
            ? " warn"
            : " ok");
    }
    if (t.critical > 0) {
      if (subline)
        subline.textContent =
          t.critical + " high-risk finding(s) need review in Investigate.";
    } else if (t.findings > 0) {
      if (subline)
        subline.textContent =
          t.findings +
          " finding(s) recorded" +
          (t.cleaned ? ", " + t.cleaned + " quarantined" : "") +
          " — nothing was deleted.";
    } else if (coverageIncomplete(summary)) {
      if (subline)
        subline.textContent =
          "Collection stopped short — review the coverage summary before drawing conclusions.";
    } else {
      if (subline)
        subline.textContent =
          summary.total +
          " checks completed — no persistence, process, or ransom findings" +
          (lastScanAt ? " · last scan " + lastScanAt.toLocaleString() : "");
    }

    const ovStates = (summary && summary.source_states) || [];
    paintOverviewDial(
      ovStates,
      t.critical > 0
        ? "is-bad"
        : t.findings > 0 || coverageIncomplete(summary)
          ? "is-warn"
          : "is-ok",
    );
    let skipped = 0;
    /* Coverage describes how much of the machine reported — never the
       operation's outcome. A collector that errored or was denied access
       means the coverage is partial (the per-layer list below spells out
       CHECK FAILED / ACCESS DENIED); "failed" is reserved for the scan
       operation itself, which is the assessment metric's job to state. */
    let covComplete = true;
    for (const row of ovStates) {
      const st = row.state;
      if (st === "Checked" || st === "Available") continue;
      covComplete = false;
      if (st && typeof st === "object" && "Partial" in st) {
        skipped += (st.Partial && st.Partial.skipped) || 0;
      }
    }
    setMetric("ov-assessment", status.label, status.tone === "ok" ? "is-ok" : status.tone === "warn" ? "is-warn" : status.tone === "bad" ? "is-bad" : null);
    setMetric(
      "ov-coverage-state",
      ovStates.length ? (covComplete ? "COMPLETE" : "PARTIAL") : "—",
      ovStates.length ? (covComplete ? "is-ok" : "is-warn") : null,
    );
    setMetric("ov-findings", String(t.findings), t.findings ? "is-warn" : null);
    refreshQuarantineCount();
    loadOperatorPosture();
    renderOperatorPosture();

    // Recent findings: only rendered when the collection actually found
    // something; an empty panel would read as a result.
    const recentPanel = document.getElementById("ov-findings-panel");
    const recent = document.getElementById("ov-recent");
    if (recentPanel && recent) {
      recent.replaceChildren();
      (summary.suspicious_for_review || []).slice(0, 4).forEach((scored) => {
        const button = makeText(
          "button",
          scored.entry.name,
          "finding-select recent-finding",
        );
        button.append(
          makeText(
            "small",
            riskLabel(scored.risk) +
              " · " +
              (SOURCE_LABELS[scored.entry.source] || scored.entry.source),
          ),
        );
        button.addEventListener("click", () => openEvidence(scored, button));
        recent.append(button);
      });
      recentPanel.classList.toggle("hidden", recent.children.length === 0);
    }
    const reviewBtn = document.getElementById("ov-review-btn");
    if (reviewBtn) {
      reviewBtn.disabled = false;
      reviewBtn.title = "Open the findings collected this session";
    }
    const ovScope = document.getElementById("ov-scope");
    if (ovScope) {
      const areas = ovStates.length
        ? ovStates.map((r) => r.area).join(" · ")
        : "Startup entries · Services · Tasks · WMI";
      ovScope.textContent =
        "Scope: " +
        areas +
        " · " +
        summary.total +
        " checked · " +
        skipped +
        " skipped" +
        (typeof summary.elevated === "boolean"
          ? summary.elevated
            ? " · elevated scan"
            : " · standard-user scan"
          : "") +
        (lastScanAt ? " · " + lastScanAt.toLocaleString() : "");
    }
    renderOverviewTimeline();

    const bySource = {};
    summary.high_risk_cleaned
      .concat(summary.suspicious_for_review)
      .forEach((e) => {
        const src = e.entry.source;
        bySource[src] = bySource[src] || { n: 0, high: 0 };
        bySource[src].n += 1;
        if (e.risk === "HighRisk") bySource[src].high += 1;
      });
    const srcRow = (key, label) => {
      const info = bySource[key] || { n: 0, high: 0 };
      const detail =
        (info.n ? info.n + " flagged · " : "") + "Coverage not reported";
      const level = info.high ? "bad" : info.n ? "warn" : "idle";
      return covRow(label, detail, level);
    };
    const cov = document.getElementById("ov-coverage");
    cov.replaceChildren();
    // Prefer backend access states when present: a failed/skipped check
    // renders its state — never a bare "checked — nothing flagged" zero.
    const states = summary.source_states || [];
    const stateByArea = {};
    for (const row of states) stateByArea[row.area] = row;
    const stateLevel = (st) => {
      // Same CoverageState shapes as renderResultsCoverage (see above).
      if (st === "Available" || st === "Checked") return "ok";
      if (st === "Unavailable" || st === "NotChecked") return "idle";
      if (st === "CheckFailed") return "bad";
      if (st === "AccessDenied") return "warn";
      if (st && typeof st === "object") {
        if ("Partial" in st) return "warn";
        if ("CheckFailed" in st) return "bad";
        if ("AccessDenied" in st) return "warn";
        if ("Unavailable" in st || "NotChecked" in st) return "idle";
        if ("Available" in st || "Checked" in st) return "ok";
      }
      return "idle";
    };
    const stateDetail = (row) => {
      const info = sourceStatusInfo(row.state);
      let detail = info.label + (row.detail ? " · " + row.detail : "");
      if (
        row.state &&
        typeof row.state === "object" &&
        "Partial" in row.state
      ) {
        detail += " — " + row.state.Partial.skipped + " skipped";
      }
      return detail;
    };
    const stateCovRow = (area, fallbackLabel) => {
      const row = stateByArea[area];
      if (!row) return covRow(area, fallbackLabel, "idle");
      return covRow(area, stateDetail(row), stateLevel(row.state));
    };
    if (states.length) {
      cov.append(
        stateCovRow("Registry autoruns", "no data"),
        srcRow("StartupFolder", "Startup folder"),
        stateCovRow("Scheduled tasks", "no data"),
        stateCovRow("Services (auto-start)", "no data"),
        stateCovRow("WMI subscriptions", "no data"),
        srcRow("IfeoDebugger", "IFEO debuggers"),
        srcRow("AppInitDlls", "AppInit DLLs"),
        srcRow("ComHijack", "COM hijacks (HKCU)"),
        covRow(
          "Running processes",
          (t.proc ? t.proc + " flagged · " : "") + "Coverage not reported",
          t.proc ? "warn" : "idle",
        ),
        covRow(
          "Ransom indicators",
          (t.ransom ? t.ransom + " found · " : "") + "Coverage not reported",
          t.ransom ? "bad" : "idle",
        ),
        covRow(
          "Canary guard",
          canaryTriggered
            ? "TRIGGERED — see Canary Guard"
            : canaryActive
              ? "active — watching decoys"
              : "off",
          canaryTriggered ? "bad" : canaryActive ? "ok" : "idle",
        ),
      );
    } else {
      renderIdleCoverage();
    }
  }

  // ---- startup audit view ----

  function evidenceText(scored) {
    const e = scored.entry;
    return (
      "[CURE evidence] " +
      e.name +
      " | " +
      e.source +
      " | score " +
      scored.score +
      " (" +
      scored.risk +
      ")\n" +
      "command: " +
      e.command +
      "\n" +
      "location: " +
      e.location +
      "\n" +
      "evidence: " +
      (scored.evidence || [])
        .map((ev) => (evLabel(ev) ? evLabel(ev) + ": " : "") + evDetail(ev))
        .join("; ")
    );
  }

  function copyEvidence(text, btn) {
    const originalLabel = btn.textContent;
    const done = () => {
      btn.textContent = "Copied";
      setTimeout(() => {
        btn.textContent = originalLabel;
      }, 1800);
    };
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard
        .writeText(text)
        .then(done, () => footFeedback("Copy failed", true));
    } else {
      const ta = document.createElement("textarea");
      ta.value = text;
      document.body.appendChild(ta);
      ta.select();
      try {
        document.execCommand("copy");
        done();
      } catch (_) {
        footFeedback("Copy failed", true);
      }
      ta.remove();
    }
  }

  // ---- process sentinel view ----

  let procFilter = "all";
  let procCache = [];

  function procRow(p) {
    const tr = document.createElement("tr");
    const nameTd = document.createElement("td");
    nameTd.className = "proc-name";
    nameTd.textContent = p.name;
    const pidTd = document.createElement("td");
    pidTd.className = "mono";
    pidTd.textContent = String(p.pid);
    const pathTd = document.createElement("td");
    pathTd.className = "mono selectable trunc-cell";
    pathTd.tabIndex = 0;
    const short = (p.exe_path || "").split(/[/\\]/).pop() || "—";
    pathTd.textContent = short;
    pathTd.title = p.exe_path || "Not collected";
    const scoreTd = document.createElement("td");
    scoreTd.className = "mono";
    scoreTd.textContent = String(p.score);
    const riskTd = document.createElement("td");
    const badge = document.createElement("span");
    badge.className =
      "severity-badge " +
      (p.risk === "HighRisk"
        ? "sev-bad"
        : p.risk === "Suspicious"
          ? "sev-warn"
          : "sev-safe");
    badge.textContent =
      p.risk === "HighRisk" ? "HIGH RISK" : (p.risk || "").toUpperCase();
    riskTd.append(badge);
    const rsnTd = document.createElement("td");
    rsnTd.className = "row-reasons selectable";
    rsnTd.tabIndex = 0;
    const evidence = Array.isArray(p.evidence) ? p.evidence : [];
    if (evidence.length) {
      rsnTd.textContent =
        evidence
          .slice(0, 3)
          .map((ev) => evDetail(ev).replace(/^[+-]\d+\s*/, ""))
          .filter(Boolean)
          .join(" · ") || "—";
      rsnTd.title = evidence.map(evDetail).join("\n") || "Not collected";
    } else {
      // Pre-evidence data: explicit, never a silent dash.
      rsnTd.textContent = "Legacy report: evidence not recorded";
      rsnTd.classList.add("dim");
    }
    const actTd = document.createElement("td");
    const kill = document.createElement("button");
    kill.className = "kill-one";
    kill.classList.add("btn");
    kill.textContent = "Terminate";
    kill.title = "Terminate this process (asks for confirmation)";
    kill.addEventListener("click", async () => {
      if (kill.disabled) return;
      const ok = await requestConfirm({
        kicker: "Confirm process termination",
        title: "Terminate this process now?",
        facts: [
          ["Process", p.name],
          ["PID", String(p.pid)],
          ["Path", p.exe_path || "Not collected"],
          ["Action", "Terminate the process immediately."],
          [
            "Reversible",
            "No — termination cannot be undone, but nothing is deleted.",
          ],
        ],
        note: "The PID is re-validated before termination. Re-run a scan afterwards to verify.",
        okLabel: "Terminate process",
      });
      if (!ok) return;
      kill.disabled = true;
      kill.textContent = "Killing…";
      try {
        const report = await invoke("kill_high_risk_processes", {
          processes: [[p.name, p.pid]],
        });
        const ok = (report.killed || []).length > 0;
        kill.textContent = ok ? "Killed" : "Failed";
        if (!ok) kill.disabled = false;
        const msg = ok
          ? "Killed " + p.name + " (pid " + p.pid + ")"
          : "Kill failed: " + (report.failed || []).join("; ");
        footFeedback(msg, !ok);
        logEvent("action", msg);
      } catch (err) {
        kill.disabled = false;
        kill.classList.add("btn");
        kill.textContent = "Terminate";
        footFeedback(cleanErrText(err, "Kill failed"), true);
      }
    });
    actTd.append(kill);
    tr.append(nameTd, pidTd, pathTd, scoreTd, riskTd, rsnTd, actTd);
    return tr;
  }

  function renderProcesses(summary) {
    procCache = summary.process_findings || [];
    const sub = document.getElementById("proc-subline");
    if (sub) sub.textContent = procCache.length + " flagged in the last scan";
    paintProcessRows();
  }

  function paintProcessRows() {
    const body = document.getElementById("proc-rows");
    const cards = document.getElementById("process-cards");
    const visible = procCache.filter(
      (p) => procFilter === "all" || p.risk === procFilter,
    );
    const visiblePids = new Set(visible.map((p) => String(p.pid)));
    // The lens shows review cards; the hidden table is kept for older
    // harnesses. Both follow the same filter.
    if (cards) {
      cards.querySelectorAll(".proc-entry").forEach((card) => {
        card.classList.toggle(
          "hidden",
          procFilter !== "all" && !visiblePids.has(card.dataset.pid),
        );
      });
    }
    if (!body) return;
    body.innerHTML = "";
    for (const p of visible) body.append(procRow(p));
    const empty = document.getElementById("proc-empty");
    if (empty) {
      empty.classList.toggle("hidden", visiblePids.size > 0);
      if (!visiblePids.size)
        empty.textContent = procCache.length
          ? "No processes match this filter."
          : "No flagged processes in the last scan.";
    }
  }

  // ---- quarantine view (real backend data) ----

  // Quarantine vault count: durable evidence across sessions, deliberately
  // labelled as such so it is never read as this session's activity.
  async function refreshQuarantineCount() {
    try {
      const records = await invoke("list_quarantine");
      quarantineVaultCount = records.length;
      setMetric(
        "ov-quarantined",
        records.length + (records.length === 1 ? " item" : " items"),
        null,
      );
    } catch (_) {
      setMetric("ov-quarantined", "Unavailable", null);
    }
    updateOvAside();
  }
  function formatQuarantineBytes(bytes) {
    if (bytes == null || isNaN(bytes) || bytes === 0) return "0 B";
    const units = ["B", "KB", "MB", "GB", "TB"];
    let i = 0;
    let v = Number(bytes);
    while (v >= 1024 && i < units.length - 1) {
      v /= 1024;
      i++;
    }
    return (i === 0 ? v : v.toFixed(1)) + " " + units[i];
  }

  async function refreshQuarantine() {
    const list = document.getElementById("q-list"),
      empty = document.getElementById("q-empty");
    const sub = document.getElementById("q-subline");
    let records;
    try {
      records = await invoke("list_quarantine");
    } catch (err) {
      sub.textContent = cleanErrText(
        err,
        "Quarantine unavailable. Choose Refresh to retry.",
      );
      footFeedback(sub.textContent, true);
      return;
    }
    list.replaceChildren();
    empty.classList.toggle("hidden", records.length > 0);
    sub.textContent =
      records.length +
      " archived file" +
      (records.length === 1 ? "" : "s") +
      " · moves recorded · scoped undo";

    const statCount = document.getElementById("q-stat-count");
    if (statCount) statCount.textContent = records.length;
    const statBytes = document.getElementById("q-stat-bytes");
    if (statBytes) {
      const total = records.reduce((sum, r) => sum + (Number(r.file_size) || 0), 0);
      statBytes.textContent = formatQuarantineBytes(total);
    }

    for (const r of records) {
      const li = makeText("li", "", "review-card q-row q-card");
      const top = makeText("div", "", "q-top");
      top.append(
        makeText("span", r.name, "rc-name mono"),
        makeText("span", r.source, "chip src"),
      );
      const undo = makeText("button", "Undo / Restore", "copy-btn q-undo");
      undo.title = "Restore this file to its original location";
      const pending = r.state === "Pending";
      if (pending) undo.textContent = "Reconcile / Undo";
      undo.addEventListener("click", async () => {
        undo.disabled = true;
        undo.setAttribute("aria-busy", "true");
        try {
          await invoke("undo_entry", { id: r.id });
          quarantinedIds.delete(r.id);
          evidenceCache.delete(r.id);
          logEvent("action", "Restored: " + r.name);
          sessionQuarantined = Math.max(0, sessionQuarantined - 1);
          refreshQuarantinedTile();
          showReceipt(
            "Restored: " + r.name,
            "returned to " + r.original_path,
            false,
          );
          const receipt = document.getElementById("q-receipt");
          receipt.textContent =
            "Restored: " +
            r.name +
            " · " +
            r.original_path +
            " · byte integrity checked; ACL/timestamp restore is best effort.";
          receipt.classList.remove("hidden");
          footFeedback("Restored: " + r.name, false);
          await refreshQuarantine();
          await refreshQuarantineCount();
          if (lastSummary) renderResults(lastSummary);
        } catch (err) {
          undo.disabled = false;
          undo.removeAttribute("aria-busy");
          footFeedback(cleanErrText(err, "Undo failed"), true);
        }
      });
      top.append(undo);
      li.append(top);
      const dl = makeText("dl", "", "q-facts");
      const facts = [
        ["Original location", r.original_path],
        ["Quarantine location", r.quarantine_path],
        ["SHA-256", r.sha256_hex || "Not recorded (legacy record)"],
        ["Size", r.file_size == null ? "Not recorded" : r.file_size + " bytes"],
        ["Quarantined", r.archived_at],
        [
          "ACL / fidelity",
          r.acl_captured
            ? "ACL snapshot captured · restore is best effort"
            : "ACL snapshot unavailable",
        ],
        ["Record state", r.state || "Committed"],
        [
          "Reason",
          "Explicit operator-confirmed quarantine · source: " + r.source,
        ],
        [
          "Undo",
          pending
            ? "Engine reconciles the pending record before scoped restore"
            : "Available · scoped restore with integrity checks",
        ],
      ];
      if ((r.security_notes || []).length)
        facts.push(["Fidelity notes", r.security_notes.join("; ")]);
      for (const [key, value] of facts)
        dl.append(makeText("dt", key), makeText("dd", value));
      li.append(dl);
      li.append(
        makeText(
          "p",
          "Quarantine does not terminate a running process. Undo never overwrites an existing original file.",
          "q-rev dim",
        ),
      );
      list.append(li);
    }
  }

  // ══════════════ incident investigation view ══════

  let incidentDuration = 30;
  let lastIncident = null;

  const VERDICT_CLASS = {
    CauseIdentified: "sev-bad",
    StrongCorrelation: "sev-bad",
    ReviewRequired: "sev-warn",
    NoDirectEvidence: "sev-safe",
    InsufficientObservation: "sev-warn",
  };

  const VERDICT_TEXT = {
    CauseIdentified: "CAUSE IDENTIFIED",
    StrongCorrelation: "STRONG CORRELATION",
    ReviewRequired: "REVIEW REQUIRED",
    NoDirectEvidence: "NO DIRECT EVIDENCE",
    InsufficientObservation: "INSUFFICIENT OBSERVATION",
  };

  const VERDICT_WHY = {
    CauseIdentified:
      "A startup entry's exact executable launched with a transient window on the same process.",
    StrongCorrelation:
      "A startup entry matches an observed launch. Consistent — not proof of intent.",
    ReviewRequired:
      "Only weak or partial relationships found — investigate manually.",
    NoDirectEvidence:
      "Observation completed; nothing links activity to startup persistence.",
    InsufficientObservation:
      "Observation failed or produced no usable data — not a clean bill of health.",
  };

  const LEVEL_CLASS = {
    Direct: "sev-bad",
    Strong: "sev-warn",
    Partial: "",
    Weak: "",
    None: "",
  };
  // Exact backend semantics (core/src/incident.rs module docs). Display only.
  const LEVEL_DEFS = {
    Direct:
      "finding's executable path matches the observed executable path (normalized). Service entries additionally require the reported PID to match.",
    Strong:
      "finding command and observed command line reference each other (executable substring either direction, or shared distinctive arguments ≥12 chars) with the launch inside the window.",
    Partial: "same executable file name, different paths (both shown).",
    Weak: "same folder but different program, or a child of a correlated process. Proximity only.",
    None: "no observed relationship.",
  };

  // Normalize a SourceStatus DTO (string "Available" from the mock, or the
  // serde-tagged object from Rust) into a display label + tone + detail.
  // Labels are the backend's own tokens: CHECKED / PARTIAL / UNAVAILABLE /
  // CHECK FAILED / ACCESS DENIED. Uncertainty never renders as success.
  // (Scan CoverageState "Checked" is also accepted defensively.)
  function sourceStatusInfo(status) {
    if (
      status === "Available" ||
      status === "Checked" ||
      (status &&
        typeof status === "object" &&
        ("Available" in status || "Checked" in status))
    ) {
      return { label: "CHECKED", level: "ok", detail: "fully enumerated" };
    }
    if (typeof status === "string") {
      if (status === "Unavailable" || status === "NotChecked")
        return {
          label: status === "NotChecked" ? "NOT CHECKED" : "UNAVAILABLE",
          level: "idle",
          detail: "cannot run here",
        };
      if (status === "CheckFailed")
        return {
          label: "CHECK FAILED",
          level: "bad",
          detail: "enumeration failed",
        };
      if (status === "AccessDenied")
        return {
          label: "ACCESS DENIED",
          level: "bad",
          detail: "access denied (elevation may help)",
        };
      return { label: String(status).toUpperCase(), level: "idle", detail: "" };
    }
    if (status && typeof status === "object") {
      if ("Partial" in status) {
        const p = status.Partial || {};
        return {
          label: "PARTIAL",
          level: "warn",
          detail:
            (p.skipped || 0) + " skipped" + (p.reason ? " — " + p.reason : ""),
        };
      }
      if ("Unavailable" in status) {
        const u = status.Unavailable || {};
        return {
          label: "UNAVAILABLE",
          level: "idle",
          detail: (typeof u === "string" ? u : u.reason) || "cannot run here",
        };
      }
      if ("CheckFailed" in status) {
        const f = status.CheckFailed || {};
        return {
          label: "CHECK FAILED",
          level: "bad",
          detail:
            (typeof f === "string" ? f : f.reason) || "enumeration failed",
        };
      }
      if ("AccessDenied" in status) {
        const a = status.AccessDenied || {};
        return {
          label: "ACCESS DENIED",
          level: "bad",
          detail:
            (typeof a === "string" ? a : a.reason) ||
            "access denied (elevation may help)",
        };
      }
    }
    return { label: "NOT COLLECTED", level: "idle", detail: "" };
  }

  function pidOf(text) {
    const m = /pid\s+(\d+)/i.exec(String(text || ""));
    return m ? Number(m[1]) : null;
  }

  // Forensic drawer (evidence inspector). Focus moves to Close on open and
  // returns to the invoker on close; Escape and backdrop clicks close.
  let incDrawerInvoker = null;
  function openIncDrawer(title, sub, groups) {
    const overlay = document.getElementById("inc-drawer-overlay");
    const body = document.getElementById("inc-drawer-body");
    const titleEl = document.getElementById("inc-drawer-title");
    const subEl = document.getElementById("inc-drawer-sub");
    const closeBtn = document.getElementById("inc-drawer-close");
    if (!overlay || !body || !titleEl) return;
    incDrawerInvoker = document.activeElement;
    titleEl.textContent = title || "Evidence";
    if (subEl) subEl.textContent = sub || "";
    body.innerHTML = "";
    for (const g of groups || []) {
      const h = document.createElement("dt");
      h.className = "drawer-section";
      h.textContent = g.heading;
      body.appendChild(h);
      const spacer = document.createElement("dd");
      spacer.className = "drawer-section-val";
      spacer.textContent = "";
      body.appendChild(spacer);
      for (const [k, v] of g.rows || []) {
        const dt = document.createElement("dt");
        dt.textContent = k;
        const dd = document.createElement("dd");
        dd.className = "selectable";
        dd.textContent =
          v === undefined || v === null || v === ""
            ? "Not collected"
            : String(v);
        if (v !== undefined && v !== null && String(v).length > 60)
          dd.title = String(v);
        body.appendChild(dt);
        body.appendChild(dd);
      }
    }
    overlay.classList.remove("hidden");
    document.getElementById("app").inert = true;
    if (closeBtn) closeBtn.focus();
  }
  function closeIncDrawer() {
    const overlay = document.getElementById("inc-drawer-overlay");
    if (overlay) overlay.classList.add("hidden");
    document.getElementById("app").inert = false;
    if (incDrawerInvoker && incDrawerInvoker.focus) incDrawerInvoker.focus();
    incDrawerInvoker = null;
    if (canaryQueued) showCanaryAlert();
  }

  function incLifetime(ms) {
    if (ms === undefined || ms === null) return "Not collected";
    return (ms / 1000).toFixed(2) + "s (±0.5 s poll precision)";
  }

  function buildCorrCard(c, procOf) {
    const li = document.createElement("li");
    li.className = "review-card";
    li.dataset.pid = String(c.process_pid);
    const main = document.createElement("div");
    main.className = "rc-main";
    const topRow = document.createElement("div");
    topRow.className = "rc-top";
    const name = document.createElement("span");
    name.className = "rc-name selectable trunc";
    name.textContent =
      (c.process_name || "Not collected") + " (pid " + c.process_pid + ")";
    name.title =
      (c.process_name || "Not collected") + " (pid " + c.process_pid + ")";
    name.tabIndex = 0;
    const badgeEl = document.createElement("span");
    badgeEl.className = "severity-badge " + (LEVEL_CLASS[c.level] || "");
    badgeEl.textContent = c.level;
    badgeEl.title = LEVEL_DEFS[c.level] || c.level;
    topRow.append(name, badgeEl);
    main.appendChild(topRow);
    const chips = document.createElement("div");
    chips.className = "chips";
    const fchip = document.createElement("span");
    fchip.className = "chip src selectable";
    fchip.textContent =
      (c.finding_name || "no finding") +
      (c.finding_source ? " · " + c.finding_source : "");
    fchip.title = "finding id: " + (c.finding_id || "Not collected");
    fchip.tabIndex = 0;
    chips.appendChild(fchip);
    const p = procOf ? procOf.get(c.process_pid) : null;
    if (p && p.ppid !== undefined) {
      const ppidChip = document.createElement("span");
      ppidChip.className = "chip";
      ppidChip.textContent = "ppid " + p.ppid;
      chips.appendChild(ppidChip);
    }
    main.appendChild(chips);
    if (c.evidence && c.evidence.length) {
      const ul = document.createElement("ul");
      ul.className = "evidence-list";
      for (const ev of c.evidence) {
        const item = document.createElement("li");
        item.className = "selectable";
        item.textContent = ev;
        ul.appendChild(item);
      }
      main.appendChild(ul);
    }
    const def = document.createElement("div");
    def.className = "corr-def dim";
    def.textContent = LEVEL_DEFS[c.level] || "";
    main.appendChild(def);
    li.appendChild(main);
    const inspect = document.createElement("button");
    inspect.className = "copy-btn";
    inspect.textContent = "Inspect evidence";
    inspect.addEventListener("click", () => {
      const rows = [
        ["Process", c.process_name || "Not collected"],
        ["PID", String(c.process_pid)],
        ["PPID", p ? String(p.ppid) : "Not collected"],
        ["Path", p ? p.exe_path || "Not collected" : "Not collected"],
        [
          "Command line",
          p && p.command_line ? p.command_line : "Not collected",
        ],
      ];
      openIncDrawer(
        "Correlation · " + (c.level || "?"),
        (c.process_name || "?") + " ↔ " + (c.finding_name || "no finding"),
        [
          {
            heading: "CORRELATION",
            rows: [
              [
                "Classification",
                (c.level || "Not collected") +
                  " — " +
                  (LEVEL_DEFS[c.level] || ""),
              ],
              [
                "Finding",
                (c.finding_name || "Not collected") +
                  (c.finding_source ? " · " + c.finding_source : ""),
              ],
              ["Finding id", c.finding_id || "Not collected"],
              [
                "Related evidence",
                (c.evidence || []).join("; ") || "Not collected",
              ],
            ],
          },
          { heading: "EXECUTION", rows: rows.slice(3) },
          { heading: "IDENTITY", rows: rows.slice(0, 3) },
        ],
      );
    });
    li.appendChild(inspect);
    return li;
  }

  function setIncidentStatus(text, isError) {
    const el = document.getElementById("inc-status");
    if (el) {
      el.textContent = text;
      el.classList.toggle("hidden", !text);
    }
    if (text) logEvent(isError ? "info" : "action", "incident: " + text);
  }

  function renderIncident(result) {
    lastIncident = result;
    const badge = document.getElementById("inc-verdict");
    if (badge) {
      badge.textContent = VERDICT_TEXT[result.verdict] || result.verdict;
      badge.className =
        "severity-badge " + (VERDICT_CLASS[result.verdict] || "");
    }
    const why = document.getElementById("inc-verdict-why");
    if (why) why.textContent = VERDICT_WHY[result.verdict] || "";
    for (const id of [
      "inc-results",
      "inc-scope-panel",
      "inc-timeline-panel",
      "inc-review-panel",
      "inc-processes-panel",
      "inc-windows-panel",
      "inc-export-panel",
    ]) {
      const el = document.getElementById(id);
      if (el) el.classList.remove("hidden");
    }
    const recPanel = document.getElementById("inc-recovery-panel");
    if (recPanel)
      recPanel.classList.toggle(
        "hidden",
        result.verdict !== "InsufficientObservation",
      );
    const meta = document.getElementById("inc-meta");
    if (meta) {
      meta.textContent =
        (result.investigation_id || "unknown id") +
        " · started " +
        (result.started_at || "Not collected") +
        " · observed " +
        (result.duration_secs !== undefined
          ? result.duration_secs + "s"
          : "Not collected") +
        " · " +
        (result.elevated ? "elevated" : "standard-user") +
        (result.truncated ? " · truncated — results capped" : " · complete");
    }
    const receipt = document.getElementById("inc-receipt");
    if (receipt) {
      receipt.classList.remove("hidden");
      receipt.textContent =
        "Investigation " +
        (result.investigation_id || "") +
        " complete: " +
        (VERDICT_TEXT[result.verdict] || result.verdict || "Not collected") +
        " — observed " +
        (result.processes || []).length +
        " processes, " +
        (result.windows || []).length +
        " windows, " +
        (result.correlations || []).length +
        " correlations.";
    }
    const scope = document.getElementById("inc-scope");
    if (scope) {
      scope.innerHTML = "";
      const winInfo = sourceStatusInfo(result.window_observation);
      const procInfo = sourceStatusInfo(result.process_observation);
      const winRow = covRow(
        "Window observation",
        winInfo.label + (winInfo.detail ? " — " + winInfo.detail : ""),
        winInfo.level,
      );
      winRow.title =
        "Titles + classes only — no screenshots, no keystrokes, no contents.";
      const procRowEl = covRow(
        "Process observation",
        procInfo.label + (procInfo.detail ? " — " + procInfo.detail : ""),
        procInfo.level,
      );
      procRowEl.title =
        "Command lines are fetched only for correlated processes (bounded).";
      const durRow = covRow(
        "Observation window",
        (result.duration_secs !== undefined
          ? result.duration_secs + "s"
          : "Not collected") +
          (result.elevated ? " · elevated" : " · standard-user"),
        "idle",
      );
      scope.append(durRow, procRowEl, winRow);
    }
    const procOf = new Map((result.processes || []).map((p) => [p.pid, p]));
    const corrByPid = new Map();
    for (const c of result.correlations || []) {
      if (!corrByPid.has(c.process_pid)) corrByPid.set(c.process_pid, []);
      corrByPid.get(c.process_pid).push(c);
    }
    const winsByPid = new Map();
    for (const w of result.windows || []) {
      if (!winsByPid.has(w.pid)) winsByPid.set(w.pid, []);
      winsByPid.get(w.pid).push(w);
    }
    const tl = document.getElementById("incident-timeline");
    if (tl) {
      tl.innerHTML = "";
      for (const e of result.timeline || []) {
        const li = document.createElement("li");
        li.className = "inc-event";
        const pid = pidOf(e.text);
        const proc = (pid !== null && procOf.get(pid)) || null;
        const top = document.createElement("div");
        top.className = "inc-event-top";
        const time = document.createElement("span");
        time.className = "ev-time";
        time.textContent = e.wall_time || "Not collected";
        const tag = document.createElement("b");
        tag.textContent = "[" + String(e.kind || "event") + "]";
        const body = document.createElement("span");
        body.className = "selectable";
        body.textContent = e.text || "";
        top.append(time, tag, body);
        li.appendChild(top);
        const det = document.createElement("div");
        det.className = "inc-event-det";
        const fields = [];
        fields.push(["process", proc ? proc.name : "Not collected"]);
        fields.push(["pid", pid !== null ? String(pid) : "Not collected"]);
        fields.push(["ppid", proc ? String(proc.ppid) : "Not collected"]);
        fields.push([
          "path",
          proc ? proc.exe_path || "Not collected" : "Not collected",
        ]);
        fields.push([
          "command line",
          proc && proc.command_line ? proc.command_line : "Not collected",
        ]);
        const corrs = pid !== null ? corrByPid.get(pid) || [] : [];
        fields.push([
          "correlation",
          corrs.length
            ? corrs
                .map((c) => c.level + ": " + (c.finding_name || "finding"))
                .join("; ")
            : "Not collected",
        ]);
        let winText = "Not collected";
        if (
          String(e.kind || "")
            .toLowerCase()
            .includes("window")
        ) {
          const match = (result.windows || []).find(
            (w) => w.title && String(e.text || "").includes(w.title),
          );
          if (match)
            winText =
              "“" +
              match.title +
              "” · class " +
              (match.class_name || "Not collected") +
              " · pid " +
              match.pid;
          else if (pid !== null && winsByPid.get(pid)) {
            winText = winsByPid
              .get(pid)
              .map(
                (w) =>
                  "“" +
                  (w.title || "(no title)") +
                  "” · " +
                  (w.class_name || "?"),
              )
              .join("; ");
          }
        } else if (pid !== null && winsByPid.get(pid)) {
          winText = winsByPid
            .get(pid)
            .map((w) => "“" + (w.title || "(no title)") + "”")
            .join("; ");
        }
        fields.push(["window", winText]);
        for (const [k, v] of fields) {
          const s = document.createElement("span");
          s.className = "inc-field selectable trunc";
          s.tabIndex = 0;
          s.textContent = k + ": " + v;
          s.title = k + ": " + v;
          det.appendChild(s);
        }
        li.appendChild(det);
        if (proc) {
          const btn = document.createElement("button");
          btn.className = "copy-btn inc-inspect";
          btn.textContent = "Inspect pid " + proc.pid;
          btn.addEventListener("click", () => {
            const pc = corrByPid.get(proc.pid) || [];
            const ws = winsByPid.get(proc.pid) || [];
            openIncDrawer(
              proc.name || "pid " + proc.pid,
              "pid " + proc.pid + " · ppid " + proc.ppid,
              [
                {
                  heading: "IDENTITY",
                  rows: [
                    ["Name", proc.name || "Not collected"],
                    ["PID", String(proc.pid)],
                    ["PPID", String(proc.ppid)],
                  ],
                },
                {
                  heading: "EXECUTION",
                  rows: [
                    ["Path", proc.exe_path || "Not collected"],
                    ["Command line", proc.command_line || "Not collected"],
                    [
                      "Creation source",
                      proc.via_events ? "WMI creation event" : "snapshot diff",
                    ],
                    ["Publisher", "Not collected"],
                    ["Signature", "Not collected"],
                  ],
                },
                {
                  heading: "OBSERVATION",
                  rows: [
                    ["First seen", "+" + proc.first_seen_ms + " ms"],
                    ["Last seen", "+" + proc.last_seen_ms + " ms"],
                    [
                      "Lifetime",
                      incLifetime(
                        (proc.last_seen_ms || 0) - (proc.first_seen_ms || 0),
                      ),
                    ],
                    ["Exited", proc.exited ? "Yes" : "No"],
                    [
                      "Baseline context",
                      proc.pre_existing
                        ? "already running when observation started"
                        : "created during the window",
                    ],
                  ],
                },
                {
                  heading: "WINDOW",
                  rows: ws.length
                    ? ws.map((w, i) => [
                        "Window " + (i + 1),
                        "“" +
                          (w.title || "(no title)") +
                          "” · " +
                          (w.class_name || "?") +
                          " · " +
                          incLifetime(
                            (w.last_seen_ms || 0) - (w.first_seen_ms || 0),
                          ),
                      ])
                    : [["Window", "No window observed for this PID"]],
                },
                {
                  heading: "CORRELATION",
                  rows: pc.length
                    ? pc.map((c, i) => [
                        "Link " + (i + 1),
                        c.level +
                          ": " +
                          (c.finding_name || "") +
                          " — " +
                          (LEVEL_DEFS[c.level] || ""),
                      ])
                    : [["Classification", "No startup correlation"]],
                },
                {
                  heading: "SOURCE STATUS",
                  rows: [
                    [
                      "Process observation",
                      sourceStatusInfo(result.process_observation).label,
                    ],
                    [
                      "Window observation",
                      sourceStatusInfo(result.window_observation).label,
                    ],
                  ],
                },
              ],
            );
          });
          li.appendChild(btn);
        }
        tl.appendChild(li);
      }
      if (!(result.timeline || []).length) {
        const li = document.createElement("li");
        li.textContent = "No timeline events recorded — see Limitations below.";
        tl.appendChild(li);
      }
    }
    const corrStrong = document.getElementById("inc-corr-strong");
    const corr = document.getElementById("inc-correlations");
    const corrEmpty = document.getElementById("inc-corr-empty");
    if (corrStrong) corrStrong.innerHTML = "";
    if (corr) corr.innerHTML = "";
    {
      const strong = (result.correlations || []).filter(
        (c) => c.level === "Direct" || c.level === "Strong",
      );
      const weak = (result.correlations || []).filter(
        (c) => c.level !== "Direct" && c.level !== "Strong",
      );
      for (const c of strong)
        if (corrStrong) corrStrong.appendChild(buildCorrCard(c, procOf));
      for (const c of weak)
        if (corr) corr.appendChild(buildCorrCard(c, procOf));
      const total = (result.correlations || []).length;
      if (corrEmpty) corrEmpty.classList.toggle("hidden", total > 0);
      if (!strong.length && corrStrong) {
        const li = document.createElement("li");
        li.className = "review-card";
        li.textContent = total
          ? "No supporting (DIRECT/STRONG) evidence in this observation."
          : "No supporting (DIRECT/STRONG) evidence in this observation.";
        corrStrong.appendChild(li);
      }
      if (!weak.length && corr) {
        const li = document.createElement("li");
        li.className = "review-card";
        li.textContent = total
          ? "No weak or partial signals in this observation."
          : "No observed process relates to a startup finding.";
        corr.appendChild(li);
      }
      if (!total) {
        if (corrStrong && !corrStrong.children.length) {
          const li = document.createElement("li");
          li.className = "review-card";
          li.textContent =
            "No supporting (DIRECT/STRONG) evidence in this observation.";
          corrStrong.appendChild(li);
        }
        if (corr && !corr.children.length) {
          const li = document.createElement("li");
          li.className = "review-card";
          li.textContent = "No observed process relates to a startup finding.";
          corr.appendChild(li);
        }
      }
    }
    const procList = document.getElementById("inc-processes");
    const procEmpty = document.getElementById("inc-processes-empty");
    if (procList) {
      procList.innerHTML = "";
      const procs = (result.processes || [])
        .slice()
        .sort((a, b) => (a.first_seen_ms || 0) - (b.first_seen_ms || 0));
      for (const p of procs) {
        const li = document.createElement("li");
        li.className = "review-card";
        li.dataset.pid = String(p.pid);
        const main = document.createElement("div");
        main.className = "rc-main";
        const topRow = document.createElement("div");
        topRow.className = "rc-top";
        const name = document.createElement("span");
        name.className = "rc-name selectable trunc";
        name.tabIndex = 0;
        name.textContent =
          (p.name || "Not collected") +
          " (pid " +
          p.pid +
          " · ppid " +
          p.ppid +
          ")";
        name.title =
          (p.exe_path || "Not collected") +
          (p.command_line
            ? "\n" + p.command_line
            : "\ncommand line: Not collected");
        const corr0 = (corrByPid.get(p.pid) || [])[0];
        const badgeEl = document.createElement("span");
        badgeEl.className =
          "severity-badge " + (corr0 ? LEVEL_CLASS[corr0.level] || "" : "");
        badgeEl.textContent = corr0 ? corr0.level : "NO LINK";
        badgeEl.title = corr0
          ? LEVEL_DEFS[corr0.level] || ""
          : "No startup correlation for this process.";
        topRow.append(name, badgeEl);
        main.appendChild(topRow);
        const chips = document.createElement("div");
        chips.className = "chips";
        const pathChip = document.createElement("span");
        pathChip.className = "chip src selectable trunc";
        pathChip.tabIndex = 0;
        pathChip.textContent = p.exe_path || "Not collected";
        pathChip.title = p.exe_path || "Not collected";
        chips.appendChild(pathChip);
        const lifeChip = document.createElement("span");
        lifeChip.className = "chip";
        const lifeMs = (p.last_seen_ms || 0) - (p.first_seen_ms || 0);
        lifeChip.textContent =
          (p.exited ? "exited · " : "running · ") +
          (lifeMs / 1000).toFixed(2) +
          "s";
        lifeChip.title =
          "Lifetime " +
          incLifetime(lifeMs) +
          (p.pre_existing
            ? " · already running when observation started"
            : "") +
          (p.via_events
            ? " · first noticed via WMI creation event"
            : " · first noticed via snapshot diff");
        chips.appendChild(lifeChip);
        main.appendChild(chips);
        const cmd = document.createElement("div");
        cmd.className = "inc-cmd selectable trunc";
        cmd.tabIndex = 0;
        cmd.textContent = "cmd: " + (p.command_line || "Not collected");
        cmd.title = p.command_line || "Not collected";
        main.appendChild(cmd);
        li.appendChild(main);
        const inspect = document.createElement("button");
        inspect.className = "copy-btn";
        inspect.textContent = "Inspect";
        inspect.addEventListener("click", () => {
          const pc = corrByPid.get(p.pid) || [];
          const ws = winsByPid.get(p.pid) || [];
          openIncDrawer(
            p.name || "pid " + p.pid,
            "pid " + p.pid + " · ppid " + p.ppid,
            [
              {
                heading: "IDENTITY",
                rows: [
                  ["Name", p.name || "Not collected"],
                  ["PID", String(p.pid)],
                  ["PPID", String(p.ppid)],
                ],
              },
              {
                heading: "EXECUTION",
                rows: [
                  ["Path", p.exe_path || "Not collected"],
                  ["Command line", p.command_line || "Not collected"],
                  [
                    "Creation source",
                    p.via_events ? "WMI creation event" : "snapshot diff",
                  ],
                ],
              },
              {
                heading: "OBSERVATION",
                rows: [
                  ["First seen", "+" + p.first_seen_ms + " ms"],
                  ["Last seen", "+" + p.last_seen_ms + " ms"],
                  ["Lifetime", incLifetime(lifeMs)],
                  ["Exited", p.exited ? "Yes" : "No"],
                  [
                    "Baseline context",
                    p.pre_existing
                      ? "already running when observation started"
                      : "created during the window",
                  ],
                ],
              },
              {
                heading: "WINDOW",
                rows: ws.length
                  ? ws.map((w, i) => [
                      "Window " + (i + 1),
                      "“" +
                        (w.title || "(no title)") +
                        "” · " +
                        (w.class_name || "?"),
                    ])
                  : [["Window", "No window observed for this PID"]],
              },
              {
                heading: "CORRELATION",
                rows: pc.length
                  ? pc.map((c, i) => [
                      "Link " + (i + 1),
                      c.level + ": " + (c.finding_name || ""),
                    ])
                  : [["Classification", "No startup correlation"]],
              },
            ],
          );
        });
        li.appendChild(inspect);
        procList.appendChild(li);
      }
      if (procEmpty) procEmpty.classList.toggle("hidden", procs.length > 0);
    }
    const wins = document.getElementById("inc-windows");
    const winsEmpty = document.getElementById("inc-windows-empty");
    if (wins) {
      wins.innerHTML = "";
      const all = (result.windows || [])
        .slice()
        .sort((a, b) => (a.first_seen_ms || 0) - (b.first_seen_ms || 0));
      for (const w of all) {
        const li = document.createElement("li");
        li.className = "review-card";
        li.dataset.pid = String(w.pid);
        const main = document.createElement("div");
        main.className = "rc-main";
        const topRow = document.createElement("div");
        topRow.className = "rc-top";
        const name = document.createElement("span");
        name.className = "rc-name selectable trunc";
        name.tabIndex = 0;
        name.textContent = "“" + (w.title || "(no title)") + "”";
        name.title =
          "class " +
          (w.class_name || "Not collected") +
          "\ntitle: " +
          (w.title || "(no title)");
        const ms = (w.last_seen_ms || 0) - (w.first_seen_ms || 0);
        const transient = w.closed && ms < 5000;
        const life = document.createElement("span");
        life.className = "score-chip low";
        life.textContent =
          (transient ? "TRANSIENT · " : w.closed ? "closed · " : "open · ") +
          (ms / 1000).toFixed(2) +
          "s";
        life.title =
          "Visible lifetime (poll granularity ±0.5 s)" +
          (w.pre_existing ? " · already open when observation started" : "");
        topRow.append(name, life);
        main.appendChild(topRow);
        const chips = document.createElement("div");
        chips.className = "chips";
        const pidChip = document.createElement("span");
        pidChip.className = "chip";
        pidChip.textContent = "pid " + w.pid;
        chips.appendChild(pidChip);
        const clsChip = document.createElement("span");
        clsChip.className = "chip selectable trunc";
        clsChip.tabIndex = 0;
        clsChip.textContent = "class " + (w.class_name || "Not collected");
        clsChip.title = w.class_name || "Not collected";
        chips.appendChild(clsChip);
        const corr0 =
          (corrByPid.get(w.pid) || []).find((c) => c.level !== "None") ||
          (corrByPid.get(w.pid) || [])[0];
        const corrChip = document.createElement("span");
        corrChip.className = "chip" + (corr0 ? " amber" : "");
        corrChip.textContent = corr0
          ? corr0.level + ": " + corr0.finding_name
          : "no startup correlation";
        corrChip.title = corr0
          ? LEVEL_DEFS[corr0.level] || ""
          : "No observed relationship for this window's process.";
        chips.appendChild(corrChip);
        const sigChip = document.createElement("span");
        sigChip.className = "chip";
        sigChip.textContent = "metadata only — no capture";
        chips.appendChild(sigChip);
        main.appendChild(chips);
        li.appendChild(main);
        const row2 = document.createElement("div");
        row2.className = "audit-actions";
        const inspect = document.createElement("button");
        inspect.className = "copy-btn";
        inspect.textContent = "Inspect";
        inspect.addEventListener("click", () => {
          const p = procOf.get(w.pid) || null;
          openIncDrawer(
            "“" + (w.title || "(no title)") + "”",
            "pid " +
              w.pid +
              " · " +
              (transient ? "transient" : w.closed ? "closed" : "open"),
            [
              {
                heading: "IDENTITY",
                rows: [
                  ["Window title", w.title || "Not collected"],
                  ["Window class", w.class_name || "Not collected"],
                  ["PID", String(w.pid)],
                  ["Process", p ? p.name || "Not collected" : "Not collected"],
                  ["PPID", p ? String(p.ppid) : "Not collected"],
                ],
              },
              {
                heading: "OBSERVATION",
                rows: [
                  ["First seen", "+" + w.first_seen_ms + " ms"],
                  ["Last seen", "+" + w.last_seen_ms + " ms"],
                  ["Lifetime", incLifetime(ms)],
                  ["Closed", w.closed ? "Yes" : "No"],
                  [
                    "Transient status",
                    transient
                      ? "transient (closed after a brief life — short life alone means nothing about intent)"
                      : "not transient",
                  ],
                  [
                    "Baseline context",
                    w.pre_existing
                      ? "already open when observation started"
                      : "opened during the window",
                  ],
                ],
              },
              {
                heading: "EXECUTION",
                rows: [
                  ["Path", p ? p.exe_path || "Not collected" : "Not collected"],
                  [
                    "Command line",
                    p && p.command_line ? p.command_line : "Not collected",
                  ],
                ],
              },
              {
                heading: "CORRELATION",
                rows: corr0
                  ? [
                      [
                        "Classification",
                        corr0.level + " — " + (LEVEL_DEFS[corr0.level] || ""),
                      ],
                      [
                        "Finding",
                        (corr0.finding_name || "") +
                          (corr0.finding_source
                            ? " · " + corr0.finding_source
                            : ""),
                      ],
                    ]
                  : [["Classification", "No startup correlation"]],
              },
            ],
          );
        });
        const copy = document.createElement("button");
        copy.className = "copy-btn";
        copy.textContent = "Copy evidence";
        copy.addEventListener("click", () => {
          copyEvidence(
            "[CURE window] " +
              (w.title || "(no title)") +
              " | pid " +
              w.pid +
              " | class " +
              (w.class_name || "Not collected") +
              " | lifetime " +
              ms +
              " ms" +
              (corr0
                ? " | " + corr0.level + ": " + corr0.finding_name
                : " | no startup correlation"),
            copy,
          );
        });
        row2.append(inspect, copy);
        li.append(row2);
        wins.appendChild(li);
      }
      if (winsEmpty) {
        const transientCount = all.filter(
          (w) =>
            w.closed && (w.last_seen_ms || 0) - (w.first_seen_ms || 0) < 5000,
        ).length;
        winsEmpty.classList.toggle("hidden", all.length > 0);
        winsEmpty.textContent = all.length
          ? transientCount +
            " transient window(s) of " +
            all.length +
            " observed — all windows listed above with titles/classes only."
          : "No windows observed in this window.";
      }
    }
    // An observation that actually ran is a verified session event.
    logEvent(
      "action",
      "login observation finished: " +
        ((lastIncident.processes || []).length || 0) +
        " processes, " +
        ((lastIncident.windows || []).length || 0) +
        " windows observed",
    );
    const incView = document.getElementById("view-incident");
    if (incView && !incView.classList.contains("hidden"))
      focusViewHeading(incView);
  }

  // ---- share result card (MVP2 TASK 8) ------------------------------------
  // Aggregate counts only: no paths, no file names, no machine identity, no
  // secret values. The preview shows exactly the bytes that get saved.

  let shareCanvas = null;

  function shareMetrics() {
    const findings = lastSummary
      ? (lastSummary.suspicious_for_review || []).length +
        (lastSummary.high_risk_cleaned || []).length
      : 0;
    const procs = lastSummary
      ? (lastSummary.process_findings || []).length
      : 0;
    return {
      findings: findings,
      procs: procs,
      canary: eventLog.filter((e) => e.kind === "canary").length,
      actions: eventLog.filter((e) => e.kind === "action").length,
      quarantined: sessionQuarantined,
      events: eventLog.length,
      scanned: !!lastSummary,
    };
  }

  function buildShareCard() {
    const W = 1200;
    const H = 630;
    const canvas = document.createElement("canvas");
    canvas.width = W;
    canvas.height = H;
    const ctx = canvas.getContext("2d");
    const m = shareMetrics();

    // Chassis
    ctx.fillStyle = "#0b0e14";
    ctx.fillRect(0, 0, W, H);
    ctx.strokeStyle = "rgba(255,255,255,0.10)";
    ctx.lineWidth = 2;
    ctx.strokeRect(10, 10, W - 20, H - 20);

    // Wordmark
    ctx.fillStyle = "#e9ecf1";
    ctx.font = '700 30px "Segoe UI", system-ui, sans-serif';
    ctx.fillText("C.U.R.E", 64, 84);
    ctx.fillStyle = "#7f8996";
    ctx.font = '600 15px "Segoe UI", system-ui, sans-serif';
    ctx.fillText("EVIDENCE CONSOLE · LOCAL · OFFLINE", 200, 82);

    // Title
    ctx.fillStyle = "#a78bfa";
    ctx.font = '650 16px "Segoe UI", system-ui, sans-serif';
    ctx.fillText("S H A R E   R E S U L T   C A R D", 64, 138);
    ctx.fillStyle = "#e9ecf1";
    ctx.font = '650 44px "Segoe UI", system-ui, sans-serif';
    ctx.fillText("Session Evidence Summary", 64, 192);

    // Scan-status line, so a card without a scan can never read as a verdict
    ctx.fillStyle = "#a9b1bd";
    ctx.font = '400 18px "Segoe UI", system-ui, sans-serif';
    ctx.fillText(
      m.scanned
        ? "Based on the last completed scan and this session's recorded actions."
        : "No scan has completed this session yet — these are session counters only.",
      64,
      228,
    );

    // Metric tiles (2 rows x 3)
    const tiles = [
      { label: "FINDINGS IN LAST SCAN", value: String(m.findings), tone: m.findings > 0 ? "#e4706a" : "#63c79a" },
      { label: "FLAGGED PROCESSES", value: String(m.procs), tone: m.procs > 0 ? "#e0a458" : "#63c79a" },
      { label: "CANARY ALERTS", value: String(m.canary), tone: m.canary > 0 ? "#e0a458" : "#63c79a" },
      { label: "ACTIONS TAKEN", value: String(m.actions), tone: "#38bdf8" },
      { label: "QUARANTINED (SESSION)", value: String(m.quarantined), tone: "#38bdf8" },
      { label: "SESSION EVENTS", value: String(m.events), tone: "#a9b1bd" },
    ];
    const tileW = (W - 128 - 48) / 3;
    const tileH = 128;
    tiles.forEach((t, i) => {
      const col = i % 3;
      const row = Math.floor(i / 3);
      const x = 64 + col * (tileW + 24);
      const y = 256 + row * (tileH + 22);
      ctx.fillStyle = "#11151c";
      ctx.beginPath();
      ctx.roundRect(x, y, tileW, tileH, 10);
      ctx.fill();
      ctx.strokeStyle = "rgba(255,255,255,0.08)";
      ctx.lineWidth = 1;
      ctx.stroke();
      ctx.fillStyle = t.tone;
      ctx.font = '700 46px Consolas, "Cascadia Mono", monospace';
      ctx.fillText(t.value, x + 24, y + 66);
      ctx.fillStyle = "#7f8996";
      ctx.font = '650 14px "Segoe UI", system-ui, sans-serif';
      ctx.fillText(t.label, x + 24, y + 98);
    });

    // Footer: honesty line + generation date. Deliberately no machine name.
    const now = new Date();
    ctx.fillStyle = "#7f8996";
    ctx.font = '400 16px "Segoe UI", system-ui, sans-serif';
    ctx.fillText(
      "Aggregate counts only — no paths, no names, no secret values. Verify nothing from this card alone; run C.U.R.E on the machine for the evidence behind it.",
      64,
      H - 62,
    );
    ctx.fillText(
      "Generated " +
        now.toISOString().slice(0, 10) +
        " " +
        now.toTimeString().slice(0, 5),
      64,
      H - 34,
    );
    return canvas;
  }

  function openSharePreview() {
    shareCanvas = buildShareCard();
    const img = document.getElementById("share-preview-img");
    if (img) img.src = shareCanvas.toDataURL("image/png");
    const status = document.getElementById("share-status");
    if (status) {
      status.textContent = "";
      status.classList.add("hidden");
    }
    const overlay = document.getElementById("share-overlay");
    if (overlay) {
      overlay.classList.remove("hidden");
      const save = document.getElementById("share-save");
      if (save) save.focus();
    }
  }

  function closeSharePreview() {
    const overlay = document.getElementById("share-overlay");
    if (overlay) overlay.classList.add("hidden");
  }

  async function saveShareCard() {
    if (!shareCanvas) return;
    const status = document.getElementById("share-status");
    const stamp = new Date().toISOString().slice(0, 10);
    try {
      const path = await invoke("share_card_png", {
        name: "cure-share-" + stamp,
        dataBase64: shareCanvas.toDataURL("image/png").split(",")[1],
      });
      if (status) {
        status.textContent = "Saved: " + path;
        status.classList.remove("hidden");
      }
      footFeedback("Share card saved: " + path, false);
      logEvent("action", "exported share result card to " + path);
    } catch (err) {
      if (status) {
        status.textContent = cleanErrText(err, "Share card export failed");
        status.classList.remove("hidden");
      }
    }
  }

  async function exportIncident(format) {
    if (!lastIncident) {
      footFeedback("Run an investigation first", true);
      return;
    }
    const redactBox = document.getElementById("exp-redact");
    const redact = redactBox ? !!redactBox.checked : true;
    footFeedback("Exporting incident " + format.toUpperCase() + "…", false);
    try {
      const path = await invoke("export_incident_report", {
        result: lastIncident,
        format,
        redact,
      });
      footFeedback("Incident report saved: " + path, false);
      logEvent(
        "action",
        "exported incident " + format.toUpperCase() + " to " + path,
      );
    } catch (err) {
      footFeedback(cleanErrText(err, "Incident export failed"), true);
    }
  }

  listen("incident-progress", (event) => {
    const p = event.payload || {};
    const el = document.getElementById("inc-progress");
    if (el) {
      el.classList.remove("hidden");
      el.textContent =
        "Observing… " +
        (p.polls || 0) +
        " polls · " +
        (p.processes || 0) +
        " processes · " +
        (p.windows || 0) +
        " windows";
    }
  });

  // Overlay review cards: one per candidate window. Every action is an
  // explicit per-window click — Close (graceful WM_CLOSE), Force close
  // (terminate, explicit only), Don't-ask-again (path+hash allowlist).
  // All attacker-controlled strings (title, path, process) go through
  // textContent, never innerHTML.
  function renderOverlayReview(candidates, lines) {
    const done = new Set();
    function maybeFinish() {
      if (done.size === candidates.length) {
        appendLog(
          "overlay",
          "review complete: " +
            lines.filter(function (l) {
              return l.indexOf("overlay:") === 0;
            }).length +
            " action(s)",
        );
        runScan(lines);
      }
    }
    candidates.forEach(function (c) {
      const li = document.createElement("li");
      li.className = "item-line fresh risk-high";
      const head = document.createElement("span");
      head.className = "iname";
      head.textContent =
        "overlay candidate: " + c.process + " (pid " + c.pid + ")";
      const detail = document.createElement("span");
      detail.className = "isrc";
      detail.textContent =
        c.path +
        " · " +
        c.signature +
        " · " +
        c.width +
        "x" +
        c.height +
        " (" +
        Math.round(c.coverage_pct) +
        "% of monitor) · " +
        c.title;
      const btnRow = document.createElement("span");
      function markDone(label) {
        done.add(c.hwnd);
        Array.prototype.forEach.call(
          btnRow.querySelectorAll("button"),
          function (b) {
            b.disabled = true;
          },
        );
        lines.push(label);
        appendLog("overlay", label);
        maybeFinish();
      }
      const closeBtn = document.createElement("button");
      closeBtn.className = "btn";
      closeBtn.textContent = "Close window";
      closeBtn.addEventListener("click", async () => {
        closeBtn.disabled = true;
        try {
          const r = await invoke("close_overlay_window", {
            hwnd: c.hwnd,
            force: false,
          });
          if (r && r.closed)
            markDone("overlay: closed " + c.process + " (graceful)");
          else {
            closeBtn.disabled = false;
            appendLog(
              "overlay",
              "still open (use Force close to terminate): " + c.process,
            );
          }
        } catch (e) {
          closeBtn.disabled = false;
          appendLog("error", String(e));
        }
      });
      const forceBtn = document.createElement("button");
      forceBtn.className = "btn btn-danger";
      forceBtn.textContent = "Force close";
      forceBtn.title =
        "Terminate the owning process. Use only for a window you have reviewed.";
      forceBtn.addEventListener("click", async () => {
        const confirmed = await requestConfirm({
          kicker: "Process termination",
          title: "Force close this window?",
          facts: [
            ["Process", c.process + " (PID " + c.pid + ")"],
            ["Executable", c.path],
            [
              "What will change",
              "The owning process will be terminated. Unsaved work may be lost.",
            ],
          ],
          note: "This action cannot be undone. Graceful Close window remains available.",
          okLabel: "Terminate process",
        });
        if (!confirmed) return;
        forceBtn.disabled = true;
        try {
          const r = await invoke("close_overlay_window", {
            hwnd: c.hwnd,
            force: true,
          });
          if (r && r.closed)
            markDone("overlay: force-closed " + c.process + " (terminated)");
          else {
            forceBtn.disabled = false;
            appendLog("overlay", "force close failed: " + c.process);
          }
        } catch (e) {
          forceBtn.disabled = false;
          appendLog("error", String(e));
        }
      });
      const allowBtn = document.createElement("button");
      allowBtn.className = "ghost-btn";
      allowBtn.textContent = "Don't ask again for this app";
      allowBtn.title =
        "Allowlist this exact binary (path + hash). Future matches skip it.";
      allowBtn.addEventListener("click", async () => {
        allowBtn.disabled = true;
        try {
          const msg = await invoke("allowlist_overlay", { path: c.path });
          markDone("overlay: " + msg);
        } catch (e) {
          allowBtn.disabled = false;
          appendLog("error", String(e));
        }
      });
      btnRow.append(
        closeBtn,
        document.createTextNode(" "),
        forceBtn,
        document.createTextNode(" "),
        allowBtn,
      );
      li.append(
        head,
        document.createElement("br"),
        detail,
        document.createElement("br"),
        btnRow,
      );
      logList.appendChild(li);
    });
    const contLi = document.createElement("li");
    const contBtn = document.createElement("button");
    contBtn.className = "btn";
    contBtn.textContent = "Continue scan without closing →";
    contBtn.addEventListener("click", () => {
      contBtn.disabled = true;
      lines.push(
        "overlay: review skipped by operator (" +
          (candidates.length - done.size) +
          " candidate(s) left open)",
      );
      _runScan(lines, scanOrigin, viewToken);
    });
    contLi.appendChild(contBtn);
    logList.appendChild(contLi);
    logList.scrollTop = logList.scrollHeight;
  }

  (async () => {
    scanView.classList.add("hidden");
    showViewInstant(document.getElementById("view-overview"));
    renderOverviewIdle();
    loadOperatorPosture();
    restorePill();
    refreshQuarantineCount();
    // One scan-start flow for both entry points. The origin decides who
    // owns navigation: "overview" keeps the operator on the Overview, while
    // "rescue" keeps the historical Rescue workflow.
    async function startScanFlow(origin) {
      if (["running", "preparing"].includes(scanPhase)) return;
      scanPhase = "preparing";
      scanOrigin = origin;
      // Navigation baseline for the whole flow: if the operator navigates
      // while the overlay check runs, the scan must not drag them back.
      const flowNavToken = viewToken;
      setPill("scanning", "Reviewing overlay candidates");
      if (origin === "rescue") {
        // Preparing is a real phase (overlay review) and the workspace says
        // so; it must not look like an active collection.
        window.CureSweep.preparing();
        setScanControls("running");
        await switchView(currentView(), scanView);
        setNav("scan-view");
      }
      const lines = [];
      let rep = null;
      try {
        rep = await invoke("list_overlay_candidates");
      } catch (e) {
        lines.push("Overlay check unavailable: " + e);
        _runScan(lines, origin, flowNavToken);
        return;
      }
      const candidates = (rep && rep.candidates) || [];
      appendLog(
        "overlay",
        "checked " +
          (rep ? rep.checked : 0) +
          " windows, " +
          candidates.length +
          " candidate(s) need review",
      );
      if (candidates.length === 0) {
        lines.push("No suspicious overlay windows found");
        _runScan(lines, origin, flowNavToken);
        return;
      }
      // Matches are SHOWN, never closed here: each card carries its own
      // Close / Force close / Don't-ask-again buttons, plus one button to
      // continue the scan. Nothing closes without a per-window click.
      // The review cards live in the scan log, so a scan that needs this
      // operator interaction brings the scan view forward — the operator
      // must act, so this is not an uninvited navigation.
      showViewInstant(scanView);
      setNav("scan-view");
      renderOverlayReview(candidates, lines);
    }
    document
      .getElementById("start-rescue-btn")
      .addEventListener("click", () => startScanFlow("rescue"));
    document
      .getElementById("overview-rescue-btn")
      .addEventListener("click", () => startScanFlow("overview"));
    document.querySelectorAll("[data-route]")
      .forEach((btn) =>
        btn.addEventListener("click", () => navTo(btn.dataset.route)),
      );

    // Stage UI-04: Forensic Investigation Lens Switching
    document.querySelectorAll(".inv-lens-btn").forEach((btn) => {
      btn.addEventListener("click", () => {
        const lens = btn.dataset.lens;
        if (!lens) return;
        switchInvestigateLens(lens);
      });
    });

    // Stage UI-04: Live search across findings
    const findingsSearch = document.getElementById("inv-findings-search");
    if (findingsSearch) {
      findingsSearch.addEventListener("input", () => {
        const query = findingsSearch.value.trim().toLowerCase();
        document.querySelectorAll("#review-cards li, #cleaned-cards li").forEach((card) => {
          const text = card.textContent.toLowerCase();
          card.classList.toggle("hidden", query.length > 0 && !text.includes(query));
        });
      });
    }

    // Stage UI-04: Findings filter buttons (All, High Risk, Suspicious, Safe)
    document.querySelectorAll("[data-ff]").forEach((btn) => {
      btn.addEventListener("click", () => {
        document.querySelectorAll("[data-ff]").forEach((b) => b.classList.remove("on"));
        btn.classList.add("on");
        const filter = btn.dataset.ff;
        document.querySelectorAll("#review-cards li").forEach((card) => {
          if (filter === "all") {
            card.classList.remove("hidden");
          } else if (filter === "HighRisk") {
            card.classList.toggle("hidden", !card.classList.contains("risk-crit") && !card.classList.contains("risk-high"));
          } else if (filter === "Suspicious") {
            card.classList.toggle("hidden", !card.classList.contains("risk-med"));
          } else if (filter === "Safe") {
            card.classList.toggle("hidden", !card.classList.contains("risk-safe"));
          }
        });
      });
    });
    document
      .getElementById("scan-retry")
      .addEventListener("click", () => runScan());
    const xStart = document.getElementById("xray-start-btn");
    if (xStart) {
      xStart.addEventListener("click", () => {
        const srBtn = document.getElementById("start-rescue-btn");
        if (srBtn) srBtn.click();
        else runScan();
      });
    }
    const xInv = document.getElementById("xray-investigate-btn");
    if (xInv) {
      xInv.addEventListener("click", () => navTo("results-view"));
    }
    const xrOpen = document.getElementById("xr-result-open");
    if (xrOpen) {
      xrOpen.addEventListener("click", () => navTo("results-view"));
    }
    const xrTools = document.getElementById("xr-tools");
    const closeTools = () => {
      if (xrTools) xrTools.open = false;
    };
    const rView = document.getElementById("radar-view-btn");
    if (rView) {
      rView.addEventListener("click", () => navTo("scan-center"));
    }
    const xDefScan = document.getElementById("xray-defender-scan-btn");
    if (xDefScan) {
      xDefScan.addEventListener("click", () => {
        closeTools();
        openDefenderModal();
      });
    }
    const xExportCase = document.getElementById("xray-export-case-btn");
    if (xExportCase) {
      xExportCase.addEventListener("click", () => {
        closeTools();
        exportCaseFile();
      });
    }

    async function exportCaseFile() {
      if (lastIncident) {
        await exportIncident("json");
      } else {
        await exportReport("json");
      }
    }

    let defenderEventsBound = false;
    function openDefenderModal() {
      bindDefenderModalEvents();
      const modal = document.getElementById("xray-defender-modal");
      if (!modal) return;

      const pathInput = document.getElementById("xray-defender-path-input");
      const outputBox = document.getElementById("xray-defender-output-box");
      const containBox = document.getElementById("xray-defender-containment-box");

      if (pathInput) {
        let suggestedPath = "";
        if (evidenceSelected && evidenceSelected.entry) {
          suggestedPath = evidenceSelected.entry.location || evidenceSelected.entry.command || "";
        }
        pathInput.value = suggestedPath;
      }

      if (outputBox) {
        outputBox.classList.add("hidden");
        outputBox.textContent = "";
      }
      if (containBox) {
        containBox.classList.add("hidden");
      }

      modal.classList.remove("hidden");
    }

    window.openDefenderModal = openDefenderModal;

    function bindDefenderModalEvents() {
      const modal = document.getElementById("xray-defender-modal");
      if (!modal || defenderEventsBound) return;
      defenderEventsBound = true;

      const closeBtn = document.getElementById("xray-defender-modal-close");
      const doneBtn = document.getElementById("xray-defender-modal-done");
      const closeModal = () => modal.classList.add("hidden");
      if (closeBtn) closeBtn.addEventListener("click", closeModal);
      if (doneBtn) doneBtn.addEventListener("click", closeModal);

      const scanBtn = document.getElementById("xray-defender-run-scan");
      const pathInput = document.getElementById("xray-defender-path-input");
      const outputBox = document.getElementById("xray-defender-output-box");
      const containBox = document.getElementById("xray-defender-containment-box");
      const targetTokenEl = document.getElementById("xray-defender-contain-target-token");
      const containInput = document.getElementById("xray-defender-contain-token-input");
      const containBtn = document.getElementById("xray-defender-run-contain");
      const verifyJournalBtn = document.getElementById("xray-defender-verify-journal");

      if (scanBtn) {
        scanBtn.addEventListener("click", async () => {
          const filePath = (pathInput && pathInput.value.trim()) || "";
          if (!filePath) {
            alert("Please specify a target file path to scan.");
            return;
          }

          try {
            scanBtn.disabled = true;
            scanBtn.textContent = "Scanning with MpCmdRun...";
            if (outputBox) {
              outputBox.classList.remove("hidden");
              outputBox.style.color = "#cbd5e1";
              outputBox.textContent = `Executing Windows Defender (read-only mode: -DisableRemediation) on: ${filePath}...`;
            }

            const res = await invoke("operator_defender_scan", { filePath });
            scanBtn.disabled = false;
            scanBtn.textContent = "Run Read-Only Scan (-DisableRemediation)";

            const isThreat = res.threat_detected || res.threats_found;
            const code = res.return_code !== undefined ? res.return_code : res.exit_code;

            if (isThreat) {
              if (outputBox) {
                outputBox.style.color = "#f43f5e";
                outputBox.textContent = `[!] DEFENDER THREAT DETECTED (Exit code: ${code}):\n${res.raw_output}`;
              }
              if (containBox) {
                containBox.classList.remove("hidden");
                const parts = filePath.split(/[/\\]/);
                const filename = parts[parts.length - 1] || "target";
                const token = `CONTAIN_DEFENDER_${filename}`;
                if (targetTokenEl) targetTokenEl.textContent = token;
                if (containInput) { containInput.value = ""; containInput.disabled = false; }
                if (containBtn) containBtn.disabled = false;
              }
            } else {
              if (outputBox) {
                outputBox.style.color = "#34d399";
                outputBox.textContent = `[OK] DEFENDER ENGINE REPORTED CLEAN (Exit code: ${code}):\n${res.raw_output || "No active threats identified."}`;
              }
              if (containBox) containBox.classList.add("hidden");
            }
          } catch (err) {
            scanBtn.disabled = false;
            scanBtn.textContent = "Run Read-Only Scan (-DisableRemediation)";
            if (outputBox) {
              outputBox.classList.remove("hidden");
              outputBox.style.color = "#f43f5e";
              outputBox.textContent = `Defender invocation error: ${cleanErrText(err, "Command execution failed")}`;
            }
          }
        });
      }

      if (containBtn) {
        containBtn.addEventListener("click", async () => {
          const filePath = (pathInput && pathInput.value.trim()) || "";
          const parts = filePath.split(/[/\\]/);
          const filename = parts[parts.length - 1] || "target";
          const expectedToken = `CONTAIN_DEFENDER_${filename}`;
          const typedToken = (containInput && containInput.value.trim()) || "";

          if (typedToken !== expectedToken) {
            alert(`Confirmation token does not match. Expected '${expectedToken}'.`);
            return;
          }

          try {
            containBtn.disabled = true;
            containBtn.textContent = "Quarantining binary...";
            const res = await invoke("operator_defender_contain", {
              filePath,
              confirmationToken: typedToken,
            });

            if (outputBox) {
              outputBox.classList.remove("hidden");
              outputBox.style.color = "#34d399";
              outputBox.textContent = `[OK] CONTAINMENT EXECUTED: Status=${res.engine_status}\n${res.raw_output}`;
            }
            if (containBox) containBox.classList.add("hidden");
          } catch (err) {
            containBtn.disabled = false;
            containBtn.textContent = "Approve Native Defender Containment";
            alert(`Containment failed: ${cleanErrText(err, "Defender remediation failed")}`);
          }
        });
      }

      if (verifyJournalBtn) {
        verifyJournalBtn.addEventListener("click", async () => {
          try {
            verifyJournalBtn.disabled = true;
            verifyJournalBtn.textContent = "Verifying hashes...";
            const count = await invoke("operator_verify_audit_journal", {});
            verifyJournalBtn.disabled = false;
            verifyJournalBtn.textContent = "Verify Audit Journal";
            if (outputBox) {
              outputBox.classList.remove("hidden");
              outputBox.style.color = "#38bdf8";
              outputBox.textContent = `[OK] AUDIT JOURNAL INTEGRITY VERIFIED:\n${count} sequential cryptographic SHA-256 blocks validated without tampering or chain breakage.`;
            }
          } catch (err) {
            verifyJournalBtn.disabled = false;
            verifyJournalBtn.textContent = "Verify Audit Journal";
            if (outputBox) {
              outputBox.classList.remove("hidden");
              outputBox.style.color = "#f43f5e";
              outputBox.textContent = `[!] AUDIT JOURNAL INTEGRITY ERROR: ${cleanErrText(err, "Journal verification failed")}`;
            }
          }
        });
      }
    }
    bindDefenderModalEvents();
    window.addEventListener("DOMContentLoaded", bindDefenderModalEvents);
    window.addEventListener("load", bindDefenderModalEvents);
    document.querySelectorAll(".nav-item").forEach((btn) => {
      btn.addEventListener("click", () => {
        if (!btn.disabled) navTo(btn.getAttribute("data-view"));
      });
    });
const canToggle2 = document.getElementById("can-toggle");
    if (canToggle2) canToggle2.addEventListener("click", toggleCanary);
    document.querySelectorAll(".filter-btn").forEach((btn) => {
      btn.setAttribute("aria-pressed", String(btn.classList.contains("on")));
      btn.addEventListener("click", () =>
        btn.parentElement
          .querySelectorAll(".filter-btn")
          .forEach((other) =>
            other.setAttribute("aria-pressed", String(other === btn)),
          ),
      );
    });
    const qRefresh = document.getElementById("q-refresh");
    if (qRefresh) qRefresh.addEventListener("click", refreshQuarantine);
    async function exportReport(format) {
      const redactBox = document.getElementById("exp-redact");
      const redact = redactBox ? !!redactBox.checked : true;
      footFeedback("Exporting " + format.toUpperCase() + " report…", false);
      try {
        const path = await invoke("export_report", { format, redact });
        footFeedback("Report saved: " + path, false);
        logEvent(
          "action",
          "exported " + format.toUpperCase() + " report to " + path,
        );
      } catch (err) {
        footFeedback(cleanErrText(err, "Report export failed"), true);
      }
    }
    const expTxt = document.getElementById("exp-txt");
    if (expTxt) expTxt.addEventListener("click", () => exportReport("txt"));
    const expJson = document.getElementById("exp-json");
    if (expJson) expJson.addEventListener("click", () => exportReport("json"));
    // Share result card (MVP2 TASK 8): preview the exact PNG, then save it.
    const shareBtn = document.getElementById("ov-share-btn");
    if (shareBtn) shareBtn.addEventListener("click", openSharePreview);
    const shareClose = document.getElementById("share-close");
    if (shareClose) shareClose.addEventListener("click", closeSharePreview);
    const shareCancel = document.getElementById("share-cancel");
    if (shareCancel) shareCancel.addEventListener("click", closeSharePreview);
    const shareSave = document.getElementById("share-save");
    if (shareSave) shareSave.addEventListener("click", saveShareCard);
    const shareOverlay = document.getElementById("share-overlay");
    if (shareOverlay)
      shareOverlay.addEventListener("mousedown", (ev) => {
        if (ev.target === shareOverlay) closeSharePreview();
      });
    document.addEventListener(
      "keydown",
      (ev) => {
        const ov = document.getElementById("share-overlay");
        if (ev.key === "Escape" && ov && !ov.classList.contains("hidden")) {
          ev.stopPropagation();
          closeSharePreview();
        }
      },
      true,
    );
    const logClear = document.getElementById("log-clear");
    if (logClear)
      logClear.addEventListener("click", () => {
        eventLog.length = 0;
        renderEventLog();
      });
    document
      .querySelectorAll("#lens-panel-processes .filter-btn[data-f]")
      .forEach((btn) => {
        btn.addEventListener("click", () => {
          document
            .querySelectorAll("#lens-panel-processes .filter-btn[data-f]")
            .forEach((b) => b.classList.remove("on"));
          btn.classList.add("on");
          procFilter = btn.getAttribute("data-f") || "all";
          paintProcessRows();
        });
      });
    document
      .querySelectorAll("#view-eventlog .filter-btn[data-ef]")
      .forEach((btn) => {
        btn.addEventListener("click", () => {
          document
            .querySelectorAll("#view-eventlog .filter-btn[data-ef]")
            .forEach((b) => b.classList.remove("on"));
          btn.classList.add("on");
          eventFeedFilter = btn.getAttribute("data-ef") || "all";
          renderEventLog();
        });
      });
    document
      .querySelectorAll("#view-incident .filter-btn[data-dur]")
      .forEach((btn) => {
        btn.addEventListener("click", () => {
          document
            .querySelectorAll("#view-incident .filter-btn[data-dur]")
            .forEach((b) => b.classList.remove("on"));
          btn.classList.add("on");
          incidentDuration = Number(btn.getAttribute("data-dur")) || 30;
        });
      });
    const incDrawerOverlay = document.getElementById("inc-drawer-overlay");
    const incDrawerClose = document.getElementById("inc-drawer-close");
    if (incDrawerClose)
      incDrawerClose.addEventListener("click", closeIncDrawer);
    if (incDrawerOverlay)
      incDrawerOverlay.addEventListener("mousedown", (ev) => {
        if (ev.target === incDrawerOverlay) closeIncDrawer();
      });
    document.addEventListener(
      "keydown",
      (ev) => {
        const openDrawer = document.getElementById("inc-drawer-overlay");
        if (
          ev.key === "Tab" &&
          openDrawer &&
          !openDrawer.classList.contains("hidden")
        ) {
          ev.preventDefault();
          document.getElementById("inc-drawer-close").focus();
        }
        if (ev.key === "Escape") {
          const ov = document.getElementById("inc-drawer-overlay");
          if (ov && !ov.classList.contains("hidden")) {
            ev.stopPropagation();
            closeIncDrawer();
          }
        }
      },
      true,
    );
    const incStart = document.getElementById("btn-incident-start");
    if (incStart)
      incStart.addEventListener("click", async () => {
        if (incStart.disabled) return;
        incStart.disabled = true;
        setIncidentStatus(
          "Observing startup activity for " +
            incidentDuration +
            " s — popups welcome, machine otherwise idle.",
          false,
        );
        const prog = document.getElementById("inc-progress");
        if (prog) {
          prog.classList.remove("hidden");
          prog.textContent = "Observing…";
        }
        try {
          const result = await invoke("start_incident_observation", {
            durationSecs: incidentDuration,
          });
          renderIncident(result);
          const counts =
            "observed " +
            (result.processes || []).length +
            " processes, " +
            (result.windows || []).length +
            " windows, " +
            (result.correlations || []).length +
            " correlations.";
          setIncidentStatus(
            "Investigation " +
              (result.investigation_id || "") +
              " complete: " +
              (result.verdict || "").replace(/([A-Z])/g, " $1").trim() +
              ". " +
              counts,
            false,
          );
        } catch (err) {
          setIncidentStatus(cleanErrText(err, "Investigation failed"), true);
        } finally {
          incStart.disabled = false;
          if (prog) prog.classList.add("hidden");
        }
      });
    const expIncTxt = document.getElementById("exp-inc-txt");
    if (expIncTxt)
      expIncTxt.addEventListener("click", () => exportIncident("txt"));
    const expIncJson = document.getElementById("exp-inc-json");
    if (expIncJson)
      expIncJson.addEventListener("click", () => exportIncident("json"));
  })();
})();
