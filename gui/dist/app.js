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
  const cleanupView = document.getElementById("cleanup-view");
  const landingView = document.getElementById("landing-view");

  // ---- UI V2 session state (all derived from real backend responses) ----
  // lastSummary/lastScanAt/scanDurationMs are set only by runScan success.
  // eventLog holds session events (scan progress, canary alerts, actions).
  let lastSummary = null;
  let lastScanAt = null;
  let scanDurationMs = null;
  let scanStartedAt = 0;
  let scanPhase = "idle"; // idle | running | done
  const eventLog = [];
  const canarySessionAlerts = [];
  let canaryTriggered = false;

  function logEvent(kind, text) {
    eventLog.push({ at: new Date(), kind: kind || "info", text: String(text) });
    if (eventLog.length > 300) eventLog.splice(0, eventLog.length - 300);
    appendEventLogRow(eventLog[eventLog.length - 1]);
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
    // The X-Ray corner tag mirrors the real session state, never a guess.
    const live = document.getElementById("xray-live");
    const fieldEl = document.getElementById("xray-field");
    if (live) {
      live.textContent =
        state === "scanning"
          ? "Collecting"
          : state === "error"
            ? "Interrupted"
            : state === "clean"
              ? "Complete"
              : state === "warn"
                ? "Review"
                : "Idle";
    }
    if (fieldEl) fieldEl.dataset.running = String(state === "scanning");
  }

  // Topbar session context — real scan metadata only, never invented.
  function renderSessionMeta(summary) {
    const el = document.getElementById("session-meta");
    if (!el) return;
    if (!summary) {
      el.textContent = "NO SCAN THIS SESSION";
      return;
    }
    const review = (summary.suspicious_for_review || []).length;
    const when = lastScanAt ? lastScanAt.toLocaleTimeString() : "—";
    el.textContent =
      "LAST SCAN " +
      when +
      " · " +
      summary.total +
      " CHECKED · " +
      review +
      " TO REVIEW";
  }

  // Topbar clock — real local system time, refreshed every 15 s.
  function tickClock() {
    const el = document.getElementById("topbar-clock");
    if (!el) return;
    el.textContent = new Date().toLocaleString([], {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  }
  tickClock();
  setInterval(tickClock, 15000);

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
    const reasons = makeText(
      "div",
      (scored.reasons || [])
        .map((r) => String(r).replace(/^[+-]\d+\s*/, ""))
        .join(" · "),
      "finding-reasons",
    );
    main.append(top, meta, loc, reasons);
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
      // Supporting state after a completed, confirmed action — never before.
      window.CureCompanion.seal(
        "Sealed: " + scored.entry.name + " · Undo available",
      );
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
    const reasons = section(body, "Why C.U.R.E flagged this", "");
    reasons.querySelector("p").remove();
    const ul = document.createElement("ul");
    (scored.reasons || []).forEach((r) => ul.append(makeText("li", r)));
    reasons.append(ul);
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
      cov.append(
        covRow(
          row.area || "source",
          info.label +
            (info.detail && info.detail !== "fully enumerated"
              ? " — " + info.detail
              : "") +
            (row.detail ? " · " + row.detail : ""),
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
    document
      .getElementById("companion-result")
      .classList.toggle("hidden", trouble > 0);
    window.CureCompanion.result(
      coverageIncomplete(summary) ? "review" : "success",
      coverageIncomplete(summary) ? "Coverage incomplete" : "Collection ended",
      coverageIncomplete(summary)
        ? "Review the collection limitations below."
        : "No findings in collected evidence.",
    );
    renderResultsCoverage(summary);

    countUp(document.getElementById("stat-cleaned"), sessionQuarantined);
    countUp(document.getElementById("stat-review"), reviewCount);
    countUp(document.getElementById("stat-safe"), summary.safe);

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
      ? "No persistence findings in collected evidence. Coverage is incomplete; review the collection limits below."
      : "No persistence findings in collected evidence. Review the reported coverage below.";
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
          var reasons = Array.isArray(p.reasons) ? p.reasons : [];
          for (var ri = 0; ri < Math.min(reasons.length, 4); ri++) {
            var lr = reasonChipLabel(String(reasons[ri]));
            var rc = document.createElement("span");
            rc.className = "chip" + (lr[1] ? " " + lr[1] : "");
            rc.textContent = lr[0];
            rc.title = reasons[ri];
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

    if (trouble === 0) {
      setPill(
        coverageIncomplete(summary) ? "warn" : "idle",
        coverageIncomplete(summary)
          ? "Coverage incomplete"
          : "No findings in collected evidence",
      );
    } else {
      const hasHighRiskInReview = summary.suspicious_for_review.some(
        (s) => s.risk === "HighRisk",
      );
      setPill(
        hasHighRiskInReview ? "danger" : "warn",
        coverageIncomplete(summary)
          ? "Review required · Coverage incomplete"
          : "Review required",
      );
    }

    syncCanaryStatus();
    enablePostScanNav();
    renderOverview(summary);
    renderAudit(summary);
    renderProcesses(summary);
    if (!resultsView.classList.contains("hidden"))
      focusViewHeading(resultsView);
  }

  // A newer navigation always wins: a queued view change from an earlier
  // click must never reveal a view the operator has already left.
  let viewToken = 0;

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
    "scan-center": "Rescue",
    "scan-view": "Rescue",
    "landing-view": "Rescue",
    "results-view": "Investigate",
    "view-audit": "Investigate / Persistence",
    "view-processes": "Investigate / Processes",
    "view-quarantine": "Quarantine",
    "cleanup-view": "Disk Cleanup",
    "view-eventlog": "Monitor",
    "view-canary": "Monitor / Canary",
    "view-incident": "Incident Investigation",
  };

  function setNav(viewId) {
    const items = document.querySelectorAll(".nav-item");
    items.forEach((item) => {
      const target = item.getAttribute("data-view");
      const active =
        target === viewId ||
        (target === "results-view" &&
          ["view-audit", "view-processes", "view-incident"].includes(viewId)) ||
        (target === "view-eventlog" && viewId === "view-canary") ||
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

  // Sidebar navigation. Cleanup entry/exit reuse the existing open/close
  // paths so pill save/restore and the armed-button state stay consistent.
  async function navTo(viewId) {
    if (viewId === "scan-center") {
      if (cleanupState.open)
        closeCleanup(
          ["running", "preparing"].includes(scanPhase) ? scanView : landingView,
        );
      else
        showViewInstant(
          ["running", "preparing"].includes(scanPhase) ? scanView : landingView,
        );
      return;
    }
    if (viewId === "cleanup-view") {
      await openCleanup();
      showViewInstant(cleanupView);
      return;
    }
    const el = document.getElementById(viewId);
    if (!el) return;
    if (cleanupState.open) closeCleanup(el);
    else showViewInstant(el);
    if (viewId === "view-overview" && lastSummary) renderOverview(lastSummary);
    if (viewId === "view-quarantine") refreshQuarantine();
    if (viewId === "view-eventlog") renderEventLog();
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
    ["nav-results", "nav-audit", "nav-processes"].forEach((id) => {
      const btn = document.getElementById(id);
      if (btn) btn.disabled = false;
    });
  }

  async function runScan(preLines = []) {
    if (scanPhase === "running") return;
    document.getElementById("scan-retry").classList.add("hidden");
    evidenceCache.clear();
    const token = ++scanToken;
    scanPhase = "running";
    scanStartedAt = performance.now();
    showViewInstant(scanView);
    setNav("scan-view");
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
      if (elapsedEl)
        elapsedEl.textContent =
          "elapsed " +
          ((performance.now() - scanStartedAt) / 1000).toFixed(1) +
          "s";
    }, 500);
    for (const line of Array.isArray(preLines) ? preLines : []) {
      appendLog("overlay", line);
    }
    setPill("scanning", "Collecting evidence");
    window.CureSweep.start();
    try {
      const summary = await invoke("run_auto_scan");
      if (token !== scanToken) return;
      scanDurationMs = Math.round(performance.now() - scanStartedAt);
      clearInterval(elapsedTimer);
      if (elapsedEl)
        elapsedEl.textContent =
          "finished in " +
          fmtDuration(scanDurationMs) +
          " · " +
          summary.total +
          " entries checked";
      lastSummary = summary;
      lastScanAt = new Date();
      scanPhase = "done";
      renderSessionMeta(summary);
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
      await switchView(currentView(), resultsView);
      setNav("results-view");
      if (token !== scanToken) return;
      focusViewHeading(resultsView);
    } catch (err) {
      clearInterval(elapsedTimer);
      window.CureSweep.fail();
      document.getElementById("scan-retry").classList.remove("hidden");
      scanPhase = "idle";
      setPill("error", String(err));
      appendLog("error", String(err));
      logEvent("info", "scan failed: " + String(err));
    }
  }

  listen("scan-progress", (event) => {
    const payload = event.payload;
    window.CureSweep.progress(payload);
    if (payload.stage === "source-state") {
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
    setPill("scanning", payload.message);
    if (payload.stage === "done") {
      logEvent("info", payload.message);
      appendLog(payload.stage, payload.message);
      return;
    }
    logEvent("info", "[" + payload.stage + "] " + payload.message);
    appendLog(payload.stage, payload.message);
  });

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

  // ---- disk cleanup (separate flow / own view) ----------------------------

  function fmtBytes(n) {
    if (n >= 1073741824) return (n / 1073741824).toFixed(1) + " GB";
    if (n >= 1048576) return (n / 1048576).toFixed(1) + " MB";
    if (n >= 1024) return Math.round(n / 1024) + " KB";
    return n + " B";
  }

  const cleanupEls = {
    backBtn: document.getElementById("cleanup-back"),
    statusLine: document.getElementById("cleanup-status-line"),
    statusText: document.getElementById("cleanup-status-text"),
    subline: document.getElementById("cleanup-subline"),
    idle: document.getElementById("cleanup-idle"),
    scanBtn: document.getElementById("cleanup-scan-btn"),
    loading: document.getElementById("cleanup-loading"),
    body: document.getElementById("cleanup-body"),
    total: document.getElementById("cleanup-total"),
    grid: document.getElementById("cleanup-grid"),
    downloads: document.getElementById("cleanup-downloads"),
    dlList: document.getElementById("cleanup-dl-list"),
    btn: document.getElementById("cleanup-btn"),
    btnLabel: document.getElementById("cleanup-btn-label"),
    status: document.getElementById("cleanup-status"),
    failures: document.getElementById("cleanup-failures"),
    stage: document.getElementById("cleanup-operation"),
    queue: document.getElementById("cleanup-queue"),
    panel: document.getElementById("cleanup-panel"),
  };
  function setCleanupBtnLabel(t) {
    if (cleanupEls.btnLabel) cleanupEls.btnLabel.textContent = t;
    else cleanupEls.btn.textContent = t;
  }
  const cleanupState = {
    summary: null,
    selectedCats: new Set(),
    checkedDownloads: new Set(),
    running: false,
    open: false,
    outcome: null,
    savedPill: null,
  };
  /* Real session facts only. Overview never triggers a disk measurement. */
  const sessionCleanup = { measured: false, last: null, freed: 0 };

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

  function setCleanupPill(state, text) {
    cleanupEls.statusLine.className = "pill " + state;
    cleanupEls.statusText.textContent = text;
    const pose =
      state === "error" || state === "warn"
        ? "review"
        : cleanupPhase === "done-ok"
          ? "success"
          : state === "scanning"
            ? "scan"
            : "idle";
    window.CureCompanion.cleanup(
      pose,
      pose === "success"
        ? "Cleanup ended"
        : pose === "review"
          ? "Review cleanup result"
          : pose === "scan"
            ? "Measuring candidates"
            : cleanupPhase === "ready"
              ? "Selection ready"
              : "Ready to measure",
      text,
    );
  }

  // Cleanup has no progress API: show an honest busy state, never estimated bytes.
  function startCleanupOperation() {
    cleanupEls.stage.classList.remove("hidden");
    cleanupEls.queue.classList.remove("hidden");
    renderCleanupQueue();
    document.getElementById("stage-main").scrollTop = 0;
    window.CureStorage.running();
    window.CureCompanion.cleanup(
      "cleanup",
      "Cleanup in progress",
      "Deleting confirmed selections. Waiting for the engine result.",
    );
  }
  function finishCleanupOperation() {
    cleanupEls.stage.classList.add("hidden");
    cleanupEls.queue.classList.add("hidden");
  }
  function resetCleanupOperation() {
    cleanupEls.stage.classList.add("hidden");
    cleanupEls.queue.classList.add("hidden");
  }

  /* What is queued is exactly what the operator confirmed — real selections,
     real byte totals from the measurement. No estimated progress. */
  function renderCleanupQueue() {
    const list = document.getElementById("cleanup-queue-list");
    if (!list || !cleanupState.summary) return;
    list.replaceChildren();
    for (const cat of cleanupState.summary.categories) {
      if (cat.item_count === 0 || !cleanupState.selectedCats.has(cat.key))
        continue;
      const row = document.createElement("div");
      row.className = "cq-row";
      const name = document.createElement("span");
      name.textContent = cat.label;
      const meta = document.createElement("b");
      meta.textContent = "queued · " + fmtBytes(cat.total_bytes);
      row.append(name, meta);
      list.append(row);
    }
    for (const dl of cleanupState.summary.downloads) {
      if (!cleanupState.checkedDownloads.has(dl.path)) continue;
      const row = document.createElement("div");
      row.className = "cq-row";
      const name = document.createElement("span");
      name.textContent = dl.name;
      const meta = document.createElement("b");
      meta.textContent = "queued · " + fmtBytes(dl.size_bytes);
      row.append(name, meta);
      list.append(row);
    }
  }

  // ---- cleanup state / rendering -------------------------------------------

  // Clear a stale cleanup result when the selection changes or the
  // view closes. Destructive confirmation now goes through requestConfirm.
  function resetCleanupResult() {
    cleanupEls.status.textContent = "";
    cleanupEls.status.classList.add("hidden");
    cleanupEls.status.classList.remove("cleanup-ok", "cleanup-fail");
    if (cleanupPhase === "done-ok" || cleanupPhase === "done-fail") {
      cleanupPhase = "ready";
      setCleanupStep(1);
    }
    updateCleanupButton();
  }

  function cleanupSelectionBytes() {
    let bytes = 0;
    for (const cat of cleanupState.summary.categories) {
      if (cat.item_count > 0 && cleanupState.selectedCats.has(cat.key)) {
        bytes += cat.total_bytes;
      }
    }
    for (const dl of cleanupState.summary.downloads) {
      if (cleanupState.checkedDownloads.has(dl.path)) bytes += dl.size_bytes;
    }
    return bytes;
  }

  function updateCleanupButton() {
    const s = cleanupState.summary;
    const anyCat =
      s &&
      s.categories.some(
        (c) => c.item_count > 0 && cleanupState.selectedCats.has(c.key),
      );
    const anyDl = cleanupState.checkedDownloads.size > 0;
    const enabled = (anyCat || anyDl) && !cleanupState.running;
    cleanupEls.btn.disabled = !enabled;
    if (!cleanupState.running) {
      cleanupEls.btn.classList.remove("btn-active");
    }
    if (!cleanupState.running && s) {
      const selB = cleanupSelectionBytes();
      setCleanupBtnLabel(
        selB > 0 ? "Clean up • " + fmtBytes(selB) : "Clean up",
      );
    } else if (!cleanupState.running) {
      setCleanupBtnLabel("Clean up");
    }
    window.CureStorage.selectionChanged(
      cleanupState.selectedCats,
      cleanupState.checkedDownloads,
    );
    renderCleanupSummary();
  }

  let cleanupPhase = "idle"; // idle | ready | running | done-ok | done-fail

  // 3-step pipeline strip (Candidates → Confirmation → Cleanup).
  function setCleanupStep(n, allDone) {
    document.querySelectorAll("#cleanup-steps .step").forEach((li) => {
      const k = Number(li.dataset.step);
      li.classList.toggle("done", allDone ? true : k < n);
      li.classList.toggle("current", !allDone && k === n);
    });
  }

  // Live summary panel — counts come straight from the scan summary and
  // the current selection. Never invented.
  function renderCleanupSummary() {
    const probe = document.getElementById("cs-total-items");
    if (!probe) return;
    const set = (id, v) => {
      const el = document.getElementById(id);
      if (el) el.textContent = v;
    };
    const s = cleanupState.summary;
    if (!s) {
      set("cs-total-items", "—");
      set("cs-total-size", "—");
      set("cs-selected", "—");
      const st0 = document.getElementById("cs-status");
      if (st0) {
        st0.textContent = "Idle";
        st0.classList.remove("on", "bad");
      }
      return;
    }
    const totalN =
      s.categories.reduce((n, c) => n + c.item_count, 0) + s.downloads.length;
    let selN = 0,
      selB = 0;
    for (const c of s.categories) {
      if (c.item_count > 0 && cleanupState.selectedCats.has(c.key)) {
        selN += c.item_count;
        selB += c.total_bytes;
      }
    }
    selN += cleanupState.checkedDownloads.size;
    for (const dl of s.downloads) {
      if (cleanupState.checkedDownloads.has(dl.path)) selB += dl.size_bytes;
    }
    set("cs-total-items", String(totalN));
    set("cs-total-size", fmtBytes(s.total_bytes));
    set("cs-selected", selN + " items · " + fmtBytes(selB));
    if (cleanupPhase === "ready")
      window.CureCompanion.cleanup(
        "idle",
        "Selection ready",
        "Selected: " +
          selN +
          " items · " +
          fmtBytes(selB) +
          ". Confirmation required.",
      );
    const labels = {
      idle: "Idle",
      ready: "Ready",
      running: "Cleaning…",
      "done-ok": "Complete",
      "done-fail": "Attention",
    };
    const st = document.getElementById("cs-status");
    if (st) {
      st.textContent = labels[cleanupPhase] || "—";
      st.classList.toggle("on", cleanupPhase !== "idle");
      st.classList.toggle("bad", cleanupPhase === "done-fail");
    }
  }

  function renderCleanup(summary, keepResult = false) {
    cleanupState.summary = summary;
    cleanupState.selectedCats = new Set();
    cleanupState.checkedDownloads = new Set();
    cleanupState.running = false;
    if (!keepResult) {
      cleanupPhase = "ready";
      setCleanupStep(1);
    }

    cleanupEls.loading.classList.add("hidden");
    cleanupEls.body.classList.remove("hidden");
    if (!keepResult) {
      cleanupEls.status.textContent = "";
      cleanupEls.status.classList.add("hidden");
      cleanupEls.failures.classList.add("hidden");
      cleanupEls.failures.innerHTML = "";
    }

    const itemCount = summary.categories.reduce((n, c) => n + c.item_count, 0);
    cleanupEls.total.innerHTML =
      "<span>≈ <b>" +
      fmtBytes(summary.total_bytes) +
      "</b> reclaimable across " +
      (itemCount + summary.downloads.length) +
      " items</span>";
    cleanupEls.subline.textContent =
      (itemCount + summary.downloads.length) +
      ((itemCount + summary.downloads.length) === 1
        ? " cleanable item found on this machine"
        : " cleanable items found on this machine");
    if (!keepResult) {
      sessionCleanup.measured = true;
      sessionCleanup.last = summary;
      updateOverviewReclaimable();
    }

    cleanupEls.grid.innerHTML = "";
    window.CureStorage.setDownloads(cleanupState.checkedDownloads);
    const maxCatBytes = Math.max(
      1,
      ...summary.categories.map((c) => c.total_bytes),
    );
    for (const cat of summary.categories) {
      const card = document.createElement("button");
      card.type = "button";
      card.className = "cleanup-cat";
      card.dataset.key = cat.key;
      card.disabled = cat.item_count === 0;
      const on = cat.item_count > 0;
      if (on) cleanupState.selectedCats.add(cat.key);
      card.classList.toggle("on", on);
      card.classList.toggle("off", !on);
      card.setAttribute("aria-pressed", String(on));
      card.setAttribute(
        "aria-label",
        cat.label +
          " — " +
          fmtBytes(cat.total_bytes) +
          ", " +
          cat.item_count +
          " items",
      );
      card.title = on
        ? "Click to skip this category"
        : cat.item_count === 0
          ? "Nothing found in this category"
          : "Currently skipped — click to include";
      const top = document.createElement("span");
      top.className = "cc-top";
      const name = document.createElement("span");
      name.className = "cc-name";
      name.textContent = cat.label;
      const included = document.createElement("span");
      included.className = "cc-state";
      included.textContent = on ? "Included" : "Skipped";
      top.append(name, included);
      const size = document.createElement("span");
      size.className = "cc-size";
      size.textContent = fmtBytes(cat.total_bytes);
      const count = document.createElement("span");
      count.className = "cc-count";
      count.textContent =
        cat.item_count + (cat.item_count === 1 ? " item" : " items");
      const bar = document.createElement("span");
      bar.className = "cc-bar";
      bar.setAttribute("aria-hidden", "true");
      const fill = document.createElement("i");
      fill.style.width =
        Math.max(
          cat.item_count === 0 ? 0 : 6,
          Math.round((cat.total_bytes / maxCatBytes) * 100),
        ) + "%";
      bar.appendChild(fill);
      card.append(top, size, count, bar);
      card.addEventListener("click", () => {
        if (card.disabled) return;
        const nowOn = !cleanupState.selectedCats.has(cat.key);
        if (nowOn) cleanupState.selectedCats.add(cat.key);
        else cleanupState.selectedCats.delete(cat.key);
        included.textContent = nowOn ? "Included" : "Skipped";
        card.classList.toggle("on", nowOn);
        card.classList.toggle("off", !nowOn);
        card.setAttribute("aria-pressed", String(nowOn));
        card.title = nowOn
          ? "Click to skip this category"
          : "Currently skipped — click to include";
        resetCleanupResult();
      });
      cleanupEls.grid.appendChild(card);
    }

    cleanupEls.downloads.classList.toggle(
      "hidden",
      summary.downloads.length === 0,
    );
    cleanupEls.dlList.innerHTML = "";
    for (const dl of summary.downloads) {
      const li = document.createElement("li");
      li.className = "dl-item";
      const box = document.createElement("input");
      box.type = "checkbox";
      box.dataset.path = dl.path;
      box.setAttribute(
        "aria-label",
        "Delete " + dl.name + " (" + fmtBytes(dl.size_bytes) + ")",
      );
      box.addEventListener("change", () => {
        if (box.checked) cleanupState.checkedDownloads.add(dl.path);
        else cleanupState.checkedDownloads.delete(dl.path);
        resetCleanupResult();
      });
      const name = document.createElement("span");
      name.className = "dl-name";
      name.textContent = dl.name;
      name.title = dl.path;
      const meta = document.createElement("span");
      meta.className = "dl-meta";
      meta.textContent =
        fmtBytes(dl.size_bytes) + " · " + dl.age_days + "d old";
      li.append(box, name, meta);
      cleanupEls.dlList.appendChild(li);
    }

    updateCleanupButton();
    window.CureStorage.measured(
      summary,
      cleanupState.selectedCats,
      cleanupState.checkedDownloads,
    );
  }

  async function startCleanupScan(keepResult = false) {
    cleanupEls.idle.classList.add("hidden");
    cleanupEls.body.classList.add("hidden");
    cleanupEls.loading.classList.remove("hidden");
    cleanupEls.loading.textContent = "measuring reclaimable space…";
    if (!keepResult) {
      window.CureStorage.measuring();
      setCleanupPill("scanning", "measuring reclaimable space…");
    }
    try {
      const summary = await invoke("scan_cleanup");
      renderCleanup(summary, keepResult);
      if (keepResult) {
        // The engine's own rescan is the only source of the post-cleanup ring.
        window.CureStorage.result(
          cleanupState.outcome,
          summary,
          cleanupState.checkedDownloads,
        );
        sessionCleanup.measured = true;
        sessionCleanup.last = summary;
        updateOverviewReclaimable();
        return;
      }
      setCleanupPill(
        "clean",
        "Disk scan complete — " +
          fmtBytes(summary.total_bytes) +
          " reclaimable",
      );
    } catch (err) {
      cleanupEls.loading.textContent =
        "disk cleanup unavailable: " + cleanErrText(err, "Collection failed");
      if (!keepResult) {
        window.CureStorage.idle();
        setCleanupPill("error", "Disk cleanup unavailable");
      }
    }
  }

  function showCleanupIdle() {
    resetCleanupOperation();
    cleanupPhase = "idle";
    setCleanupStep(1);
    cleanupEls.stage.classList.add("hidden");
    cleanupEls.body.classList.add("hidden");
    cleanupEls.loading.classList.add("hidden");
    cleanupEls.idle.classList.remove("hidden");
    window.CureStorage.idle();
    // A new cleanup session rotates Luma's cleanup variant.
    window.CureCompanion.beginSession("cleanup");
    setCleanupPill("idle", "Disk cleanup — ready when you are");
  }

  async function openCleanup() {
    if (cleanupState.open) return;
    cleanupState.open = true;
    cleanupState.savedPill = {
      cls: statusPill.className,
      text: statusText.textContent,
    };
    showCleanupIdle();
    await switchView(currentView(), cleanupView);
    setNav("cleanup-view");
  }

  function closeCleanup(backTo) {
    if (!cleanupState.open) return;
    cleanupState.open = false;
    resetCleanupResult();
    resetCleanupOperation();
    cleanupEls.stage.classList.add("hidden");
    if (cleanupState.savedPill) {
      statusPill.className = cleanupState.savedPill.cls;
      statusText.textContent = cleanupState.savedPill.text;
    }
    const target = backTo && backTo.classList ? backTo : resultsView;
    switchView(cleanupView, target);
    if (target.id) setNav(target.id);
  }

  cleanupEls.openBtn?.addEventListener("click", openCleanup);
  cleanupEls.backBtn.addEventListener("click", () => closeCleanup(resultsView));
  cleanupEls.scanBtn.addEventListener("click", () => startCleanupScan(false));

  cleanupEls.btn.addEventListener("click", async () => {
    if (cleanupState.running || cleanupEls.btn.disabled) return;
    const selBytes = cleanupSelectionBytes();
    const selCats = (cleanupState.summary.categories || [])
      .filter((c) => c.item_count > 0 && cleanupState.selectedCats.has(c.key))
      .map((c) => c.label || c.key);
    if (cleanupState.checkedDownloads.size > 0) {
      selCats.push(
        cleanupState.checkedDownloads.size + " selected download(s)",
      );
    }
    setCleanupStep(2);
    const ok = await requestConfirm({
      kicker: "Confirm disk cleanup",
      title: "Permanently delete selected files?",
      facts: [
        ["Selection", selCats.join(", ") || "Nothing selected"],
        ["Amount", fmtBytes(selBytes)],
        ["Action", "Permanently delete the selected files to free disk space."],
        ["Reversible", "No — deleted files are not recoverable."],
      ],
      note: "Only the selected categories and checked downloads are deleted. Quarantined items are never touched.",
      okLabel: "Delete " + fmtBytes(selBytes),
    });
    if (!ok) {
      setCleanupStep(1);
      return;
    }
    cleanupPhase = "running";
    setCleanupStep(3);
    renderCleanupSummary();
    cleanupState.running = true;
    cleanupEls.btn.disabled = true;
    cleanupEls.btn.classList.add("btn-active");
    setCleanupBtnLabel("Cleaning…");
    const expected = cleanupSelectionBytes();
    startCleanupOperation(expected);
    let outcome = null;
    try {
      const result = await invoke("run_cleanup", {
        categories: Array.from(cleanupState.selectedCats),
        downloadPaths: Array.from(cleanupState.checkedDownloads),
      });
      outcome = result;
      finishCleanupOperation(result.bytes_freed);
      cleanupPhase = result.failed ? "done-fail" : "done-ok";
      sessionCleanup.freed += result.bytes_freed;
      setCleanupStep(3, true);
      setCleanupPill(
        result.failed ? "warn" : "clean",
        "Freed " +
          fmtBytes(result.bytes_freed) +
          (result.failed
            ? " — " + result.failed + " item(s) locked or failed"
            : ""),
      );
      cleanupEls.status.textContent =
        "Freed " +
        fmtBytes(result.bytes_freed) +
        " — deleted " +
        result.deleted +
        " of " +
        result.attempted +
        (result.failed ? ", " + result.failed + " locked or failed" : "");
      window.CureCompanion.cleanup(
        result.failed ? "review" : "success",
        result.failed ? "Cleanup needs review" : "Cleanup ended",
        cleanupEls.status.textContent,
      );
      cleanupEls.status.classList.remove("hidden");
      cleanupEls.status.classList.toggle("cleanup-ok", !result.failed);
      cleanupEls.status.classList.toggle("cleanup-fail", !!result.failed);
      if (!result.failed && cleanupEls.panel) {
        cleanupEls.panel.classList.add("cleanup-success");

        setTimeout(() => {
          if (cleanupEls.panel)
            cleanupEls.panel.classList.remove("cleanup-success");
        }, 1400);
      }
      logEvent(
        "action",
        "cleanup: freed " +
          fmtBytes(result.bytes_freed) +
          ", deleted " +
          result.deleted +
          " of " +
          result.attempted +
          (result.failed ? ", " + result.failed + " failed" : ""),
      );
      if (result.failures.length > 0) {
        cleanupEls.failures.innerHTML = "";
        for (const failure of result.failures) {
          const li = document.createElement("li");
          li.className = "cf-card";
          const badge = document.createElement("span");
          badge.className = "cf-badge";
          badge.textContent = "locked · skipped";
          const p = document.createElement("span");
          p.className = "cf-path selectable";
          p.textContent = failure.path;
          p.title = failure.path;
          const r = document.createElement("span");
          r.className = "cf-reason selectable";
          r.textContent = failure.reason;
          li.append(badge, p, r);
          cleanupEls.failures.appendChild(li);
        }
        cleanupEls.failures.classList.remove("hidden");
      }
    } catch (err) {
      outcome = { failed: 1, bytes_freed: 0, deleted: 0, attempted: 0, failures: [] };
      finishCleanupOperation(0);
      cleanupPhase = "done-fail";
      setCleanupStep(3, true);
      setCleanupPill("error", "Disk cleanup failed");
      cleanupEls.status.textContent =
        "cleanup failed: " + cleanErrText(err, "Collection failed");
      cleanupEls.status.classList.remove("hidden");
      cleanupEls.status.classList.remove("cleanup-ok");
      cleanupEls.status.classList.add("cleanup-fail");
      updateOverviewReclaimable();
    } finally {
      cleanupState.running = false;
      cleanupEls.btn.classList.remove("btn-active");
      cleanupState.outcome = outcome;
      // One engine rescan feeds both the category table and the storage ring.
      await startCleanupScan(true);
    }
  });

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
    const list = document.getElementById("event-log-list");
    if (!list) return;
    const li = document.createElement("li");
    if (e.kind === "canary") li.className = "ev-canary";
    else if (e.kind === "action") li.className = "ev-action";
    const time = document.createElement("span");
    time.className = "ev-time";
    time.textContent = e.at.toLocaleTimeString();
    const tag = document.createElement("b");
    tag.textContent = "[" + e.kind + "]";
    const body = document.createElement("span");
    body.className = "selectable";
    body.textContent = e.text;
    li.append(time, tag, body);
    list.appendChild(li);
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
      const li = document.createElement("li");
      if (e.kind === "canary") li.className = "ev-canary";
      else if (e.kind === "action") li.className = "ev-action";
      const time = document.createElement("span");
      time.className = "ev-time";
      time.textContent = e.at.toLocaleTimeString();
      const tag = document.createElement("b");
      tag.textContent = "[" + e.kind + "]";
      const body = document.createElement("span");
      body.className = "selectable";
      body.textContent = e.text;
      li.append(time, tag, body);
      list.appendChild(li);
    }
    const empty = document.getElementById("event-log-empty");
    if (empty) empty.classList.toggle("hidden", eventLog.length > 0);
  }

  // ---- canary view ----

  function appendCanaryAlertRow(a) {
    const list = document.getElementById("can-alerts");
    if (!list) return;
    const li = document.createElement("li");
    li.className = "ev-canary";
    const time = document.createElement("span");
    time.className = "ev-time";
    time.textContent = a.at.toLocaleTimeString();
    const tag = document.createElement("b");
    tag.textContent = "[alert]";
    const body = document.createElement("span");
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

  /* Disk reclaimable on Overview reflects only what this session really
   measured. The overview never triggers a disk walk of its own. */
  function updateOverviewReclaimable() {
    const el = document.getElementById("ov-reclaimable");
    if (!el) return;
    if (!sessionCleanup.measured || !sessionCleanup.last) {
      el.textContent = "Not measured";
      el.classList.remove("is-ok");
      return;
    }
    el.textContent =
      fmtBytes(sessionCleanup.last.total_bytes) +
      (sessionCleanup.freed > 0 ? " · " + fmtBytes(sessionCleanup.freed) + " freed" : "");
    el.classList.toggle("is-ok", sessionCleanup.freed > 0);
  }

  /* Overview dial: the share of the nine inspection layers that actually
     reported coverage. Never an estimate — no collectors, no arc. */
  const DIAL_R = 52;
  function paintOverviewDial(ovStates, tone) {
    const arc = document.getElementById("ov-dial-arc");
    const value = document.getElementById("ov-dial-value");
    if (!arc || !value) return;
    const total = 9;
    const checked = (ovStates || []).filter(function (row) {
      const st = row.state;
      const key = typeof st === "string" ? st : Object.keys(st || {})[0];
      return key === "Checked" || key === "Available";
    }).length;
    const circumference = 2 * Math.PI * DIAL_R;
    arc.style.strokeDasharray = String(circumference);
    arc.style.strokeDashoffset = String(
      circumference * (1 - (ovStates && ovStates.length ? checked / total : 0)),
    );
    arc.classList.remove("is-warn", "is-bad");
    if (tone === "is-warn") arc.classList.add("is-warn");
    if (tone === "is-bad") arc.classList.add("is-bad");
    value.textContent = ovStates && ovStates.length ? checked + "/" + total : "—";
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

  function updateOvIncident() {
    const el = document.getElementById("ov-incident");
    if (!el) return;
    try {
      if (typeof lastIncident !== "undefined" && lastIncident) {
        el.textContent =
          "Last observation " +
          (lastIncident.investigation_id || "") +
          ": " +
          ((typeof VERDICT_TEXT !== "undefined" &&
            VERDICT_TEXT[lastIncident.verdict]) ||
            lastIncident.verdict ||
            "") +
          " — " +
          (lastIncident.processes || []).length +
          " processes, " +
          (lastIncident.windows || []).length +
          " windows, " +
          (lastIncident.correlations || []).length +
          " correlations. See the Incident view for the full timeline.";
        return;
      }
    } catch (_) {
      /* TDZ-safe: fall through to default */
    }
    el.textContent = "No login observation recorded this session.";
  }

  function renderOverview(summary) {
    const t = troubleCounts(summary);
    window.CureCompanion.overview(
      t.findings > 0 || coverageIncomplete(summary) ? "review" : "success",
      t.critical > 0
        ? "High risk · Review evidence"
        : t.findings > 0
          ? "Review required"
          : coverageIncomplete(summary)
            ? "Coverage incomplete"
            : "Collection ended",
    );
    const posture = document.getElementById("ov-posture");
    const subline = document.getElementById("ov-subline");
    const beacon = document.getElementById("ov-beacon");
    if (beacon) {
      beacon.className =
        "ov-beacon" +
        (t.critical > 0 ? " bad" : t.findings > 0 || coverageIncomplete(summary) ? " warn" : summary ? " ok" : "");
    }
    if (t.critical > 0) {
      posture.textContent = "High risk";
      posture.classList.add("is-bad");
      posture.classList.remove("is-warn", "is-ok");
      subline.textContent =
        t.critical + " high-risk finding(s) need review in Investigate.";
    } else if (t.findings > 0) {
      posture.textContent = "Review required";
      posture.classList.add("is-warn");
      posture.classList.remove("is-bad", "is-ok");
      subline.textContent =
        t.findings +
        " finding(s) recorded" +
        (t.cleaned ? ", " + t.cleaned + " quarantined" : "") +
        " — nothing was deleted.";
    } else {
      posture.textContent = coverageIncomplete(summary)
        ? "Coverage incomplete"
        : "No findings in collected evidence";
      posture.classList.remove("is-ok", "is-bad", "is-warn");
      if (coverageIncomplete(summary)) posture.classList.add("is-warn");
      subline.textContent =
        summary.total +
        " checks completed — no persistence, process, or ransom findings" +
        (lastScanAt ? " · last scan " + lastScanAt.toLocaleString() : "");
    }
    setMetric(
      "ov-last-scan",
      lastScanAt ? lastScanAt.toLocaleString() : "—",
      null,
    );
    setMetric("ov-checks", String(summary.total), null);
    const ovStates = (summary && summary.source_states) || [];
    paintOverviewDial(
      ovStates,
      t.critical > 0 ? "is-bad" : t.findings > 0 || coverageIncomplete(summary) ? "is-warn" : "is-ok",
    );
    let skipped = 0;
    let covState = "REPORTED CHECKS OK";
    let covTone = "is-ok";
    for (const row of ovStates) {
      const st = row.state;
      if (st === "CheckFailed" || st === "AccessDenied") {
        covState = "FAILED";
        covTone = "is-bad";
      } else if (st && typeof st === "object") {
        if ("Partial" in st) {
          skipped += (st.Partial && st.Partial.skipped) || 0;
          if (covState !== "FAILED") {
            covState = "PARTIAL";
            covTone = "is-warn";
          }
        } else if ("CheckFailed" in st || "AccessDenied" in st) {
          covState = "FAILED";
          covTone = "is-bad";
        } else if (
          ("Unavailable" in st || "NotChecked" in st) &&
          covState === "REPORTED CHECKS OK"
        ) {
          covState = "LIMITED";
          covTone = null;
        }
      } else if (
        (st === "Unavailable" || st === "NotChecked") &&
        covState === "REPORTED CHECKS OK"
      ) {
        covState = "LIMITED";
        covTone = null;
      }
    }
    setMetric("ov-skipped", String(skipped), skipped ? "is-warn" : null);
    setMetric(
      "ov-coverage-state",
      ovStates.length ? covState : "—",
      ovStates.length ? covTone : null,
    );
    setMetric("ov-findings", String(t.findings), t.findings ? "is-warn" : null);
    refreshQuarantineCount();
    updateOverviewReclaimable();
    const recent = document.getElementById("ov-recent");
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
    if (!recent.children.length)
      recent.append(
        makeText(
          "div",
          coverageIncomplete(summary)
            ? "No findings recorded. Review collection limits before drawing conclusions."
            : "No findings in the collected evidence. A scan is not a guarantee of detection.",
          "empty-state",
        ),
      );
    setMetric(
      "ov-critical",
      String(t.critical),
      t.critical ? "is-bad" : "is-ok",
    );
    setMetric(
      "ov-suspicious",
      String(t.suspicious),
      t.suspicious ? "is-warn" : "is-ok",
    );
    setMetric("ov-duration", fmtDuration(scanDurationMs), null);
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
        (lastScanAt ? " · " + lastScanAt.toLocaleString() : "");
    }
    const ovActions = document.getElementById("ov-actions");
    const ovActionsEmpty = document.getElementById("ov-actions-empty");
    if (ovActions) {
      ovActions.innerHTML = "";
      const recent = eventLog.slice(-5).reverse();
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
    updateOvIncident();

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
    cov.innerHTML = "";
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
      cov.append(
        srcRow("RegistryRun", "Registry autoruns (Run / RunOnce)"),
        srcRow("StartupFolder", "Startup folder"),
        srcRow("ScheduledTask", "Scheduled tasks"),
        srcRow("WindowsService", "Auto-start services"),
        srcRow("WmiSubscription", "WMI event subscriptions"),
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
    }
    const ovSub = document.getElementById("ov-subline");
    if (ovSub && typeof summary.elevated === "boolean") {
      ovSub.textContent += summary.elevated
        ? " · elevated scan"
        : " · standard-user scan";
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
      "reasons: " +
      (scored.reasons || []).join("; ")
    );
  }

  function copyEvidence(text, btn) {
    const originalLabel = btn.textContent;
    const done = () => {
      btn.textContent = "Copied ✓";
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

  function buildAuditCard(scored, cleaned) {
    return buildCard(scored, cleaned);
  }

  function renderAudit(summary) {
    lastAuditSummary = summary;
    paintAuditList();
  }

  function paintAuditList() {
    const summary = lastAuditSummary;
    const list = document.getElementById("audit-list");
    if (!list || !summary) return;
    list.innerHTML = "";
    const entries = summary.high_risk_cleaned
      .map((e) => ({ e, cleaned: true }))
      .concat(
        summary.suspicious_for_review.map((e) => ({ e, cleaned: false })),
      );
    entries.sort((a, b) => b.e.score - a.e.score);
    const shown = entries.filter(
      ({ e }) => auditFilter === "all" || e.risk === auditFilter,
    );
    for (const { e, cleaned } of shown) list.append(buildAuditCard(e, cleaned));
    const sub = document.getElementById("audit-subline");
    if (sub)
      sub.textContent =
        entries.length +
        " persistence finding(s) in the last scan — showing " +
        shown.length +
        " — nothing is disabled automatically.";
    const note = document.getElementById("audit-note");
    if (note)
      note.textContent =
        summary.safe +
        " safe-scored entries are not listed individually. Open evidence to collect signature and publisher details.";
  }

  // ---- process sentinel view ----

  let procFilter = "all";
  let procCache = [];
  let auditFilter = "all";
  let lastAuditSummary = null;

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
    const reasons = Array.isArray(p.reasons) ? p.reasons : [];
    rsnTd.textContent = reasons.slice(0, 3).join(" · ") || "—";
    rsnTd.title = reasons.join("\n") || "Not collected";
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
    if (sub)
      sub.textContent =
        procCache.length +
        " flagged process(es) in the last scan. Killing requires confirmation per process — PIDs are re-validated before termination.";
    paintProcessRows();
  }

  function paintProcessRows() {
    const body = document.getElementById("proc-rows");
    if (!body) return;
    body.innerHTML = "";
    const rows = procCache.filter(
      (p) => procFilter === "all" || p.risk === procFilter,
    );
    for (const p of rows) body.append(procRow(p));
    const empty = document.getElementById("proc-empty");
    if (empty) {
      empty.classList.toggle("hidden", rows.length > 0);
      if (!rows.length)
        empty.textContent = procCache.length
          ? "No processes match this filter."
          : "No flagged processes in the last scan.";
    }
  }

  // ---- quarantine view (real backend data) ----

  async function refreshQuarantineCount() {
    try {
      const records = await invoke("list_quarantine");
      setMetric("ov-quarantined", String(records.length), null);
    } catch (_) {
      setMetric("ov-quarantined", "Unavailable", null);
    }
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
    for (const r of records) {
      const li = makeText("li", "", "review-card q-row");
      const top = makeText("div", "", "q-top");
      top.append(
        makeText("span", r.name, "rc-name"),
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
          if (lastSummary) {
            renderResults(lastSummary);
            paintAuditList();
          }
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
    try {
      updateOvIncident();
    } catch (_) {
      /* overview not ready */
    }
    const incView = document.getElementById("view-incident");
    if (incView && !incView.classList.contains("hidden"))
      focusViewHeading(incView);
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
      runScan(lines);
    });
    contLi.appendChild(contBtn);
    logList.appendChild(contLi);
    logList.scrollTop = logList.scrollHeight;
  }

  (async () => {
    scanView.classList.add("hidden");
    showViewInstant(document.getElementById("view-overview"));
    paintOverviewDial(null, null);
    updateOverviewReclaimable();
    setPill("idle", "Ready");
    refreshQuarantineCount();
    document
      .getElementById("start-rescue-btn")
      .addEventListener("click", async () => {
        if (["running", "preparing"].includes(scanPhase)) return;
        scanPhase = "preparing";
        window.CureSweep.start();
        setPill("scanning", "Reviewing overlay candidates");
        await switchView(currentView(), scanView);
        setNav("scan-view");
        const lines = [];
        let rep = null;
        try {
          rep = await invoke("list_overlay_candidates");
        } catch (e) {
          lines.push("Overlay check unavailable: " + e);
          runScan(lines);
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
          runScan(lines);
          return;
        }
        // Matches are SHOWN, never closed here: each card carries its own
        // Close / Force close / Don't-ask-again buttons, plus one button to
        // continue the scan. Nothing closes without a per-window click.
        renderOverlayReview(candidates, lines);
      });
    document.querySelectorAll("[data-route]")
      .forEach((btn) =>
        btn.addEventListener("click", () => navTo(btn.dataset.route)),
      );
    document
      .getElementById("scan-retry")
      .addEventListener("click", () => runScan());
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
    document
      .getElementById("overview-rescue-btn")
      .addEventListener("click", async () => {
        await navTo("scan-center");
        document.getElementById("start-rescue-btn").click();
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
    const logClear = document.getElementById("log-clear");
    if (logClear)
      logClear.addEventListener("click", () => {
        eventLog.length = 0;
        renderEventLog();
      });
    document
      .querySelectorAll("#view-processes .filter-btn[data-f]")
      .forEach((btn) => {
        btn.addEventListener("click", () => {
          document
            .querySelectorAll("#view-processes .filter-btn[data-f]")
            .forEach((b) => b.classList.remove("on"));
          btn.classList.add("on");
          procFilter = btn.getAttribute("data-f") || "all";
          paintProcessRows();
        });
      });
    document
      .querySelectorAll("#view-audit .filter-btn[data-af]")
      .forEach((btn) => {
        btn.addEventListener("click", () => {
          document
            .querySelectorAll("#view-audit .filter-btn[data-af]")
            .forEach((b) => b.classList.remove("on"));
          btn.classList.add("on");
          auditFilter = btn.getAttribute("data-af") || "all";
          paintAuditList();
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
