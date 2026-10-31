/* ==========================================================================
   C.U.R.E — AI Agent Radar, Flight Recorder, Windows Change Radar
   --------------------------------------------------------------------------
   A process sighting is not an activity claim. A filesystem change observed
   while an agent was running is correlated, never attributed. This module
   only renders what the backend returns; it does not invent sessions,
   changes, timestamps or verdicts.

   Session lifecycle: a fresh launch starts idle ("not scanned this
   session"). The process table is read only when the operator clicks
   Scan Agents / Refresh — never on a timer and never on page open.
   ========================================================================== */
(function () {
  "use strict";

  const TAU = window.__TAURI__;
  const view = document.getElementById("view-agents");
  if (!view) return;

  const invoke =
    TAU && TAU.core && typeof TAU.core.invoke === "function"
      ? TAU.core.invoke
      : null;
  const listen =
    TAU && TAU.event && typeof TAU.event.listen === "function"
      ? TAU.event.listen
      : null;

  const $ = function (id) {
    return document.getElementById(id);
  };

  function sanitizeDisplayPath(p) {
    if (!p) return "path unavailable";
    return String(p)
      .replace(/([A-Za-z]:\\Users\\)[^\\]+/gi, "$1<user>")
      .replace(/(\/home\/)[^\/]+/gi, "$1<user>")
      .replace(/(\/Users\/)[^\/]+/gi, "$1<user>");
  }

  const els = {
    demo: $("ar-demo-banner"),
    radar: $("ar-radar"),
    detected: $("ar-detected-count"),
    radarLabel: $("ar-radar-label"),
    blips: $("ar-blips"),
    radarNote: $("ar-radar-note"),
    scanAt: $("ar-scan-at"),
    scanNow: $("ar-scan-now"),
    refresh: $("ar-refresh"),
    actionStatus: $("ar-action-status"),
    agentList: $("ar-agent-list"),
    agentsEmpty: $("ar-agents-empty"),
    emptyTitle: $("ar-empty-title"),
    emptyLead: $("ar-empty-lead"),
    sessionSub: $("ar-session-sub"),
    sessionBadge: $("ar-session-badge"),
    sessionDuration: $("ar-session-duration"),
    workspace: $("ar-workspace"),
    start: $("ar-start"),
    stop: $("ar-stop"),
    error: $("ar-workspace-error"),
    history: $("ar-history"),
    historyMeta: $("ar-history-meta"),
    historyOpen: $("ar-history-open"),
    flightEmpty: $("ar-flight-empty"),
    flight: $("ar-flight-live"),
    statTotal: $("ar-stat-total"),
    statCreated: $("ar-stat-created"),
    statModified: $("ar-stat-modified"),
    statRemoved: $("ar-stat-removed"),
    statBytes: $("ar-stat-bytes"),
    statConfidence: $("ar-stat-confidence"),
    coverage: $("ar-coverage"),
    reviewToggle: $("ar-review-toggle"),
    review: $("ar-review"),
    exportToggle: $("ar-export-toggle"),
    exportMenu: $("ar-export"),
    scrub: $("ar-scrub"),
    scrubLabel: $("ar-scrub-label"),
    timeline: $("ar-timeline"),
    filters: $("ar-filters"),
    eventList: $("ar-event-list"),
    eventEmpty: $("ar-event-empty"),
    evidence: $("ar-evidence"),
    evidenceEmpty: $("ar-evidence-empty"),
    evidenceCard: $("ar-evidence-card"),
    winCapture: $("ar-win-capture"),
    winCompare: $("ar-win-compare"),
    winState: $("ar-winsnap-state"),
    winSummary: $("ar-win-summary"),
    winLists: $("ar-win-lists"),
    caseSummary: $("ar-case-summary"),
    caseInclude: $("ar-case-include"),
    casePreview: $("ar-case-preview"),
    caseExport: $("ar-case-export"),
    caseStatus: $("ar-case-status"),
    caseStage: $("ar-case-stage"),
  };

  const state = {
    radar: null,
    session: null,
    windows: null,
    filter: "all",
    scrub: null,
    selectedSeq: null,
    caseModel: null,
    caseDataUrl: null,
    caseSignature: "",
    busy: false,
    scanning: false,
    scanQueued: false,
    stopQueued: false,
    // Unix seconds of the current process snapshot, or null when this
    // session has not scanned yet. Drives the honest idle state.
    scannedAt: null,
    selectedAgent: null,
    // pid -> AgentSessionDetail (real session-artifact intelligence).
    sessionsByPid: {},
    // A session restored from disk at launch is durable history: collapsed
    // and clearly stopped until the operator opens it.
    recovered: false,
    everActive: false,
    historyOpen: false,
    reviewMode: false,
    exportOpen: false,
  };

  // Monotonic token: a radar snapshot that arrives after a newer scan or
  // lifecycle change must never be rendered.
  let scanSeq = 0;

  const AGENT_LABELS = {
    opencode: "OpenCode",
    codex: "Codex CLI",
    codex_cli: "Codex CLI",
    claude: "Claude Code",
    aider: "Aider",
    ollama: "Ollama (Model Server)",
    cursor: "Cursor",
    cline: "Cline",
    continue: "Continue",
    gemini_cli: "Gemini CLI",
    copilot: "GitHub Copilot",
    antigravity: "Antigravity",
    windsurf: "Windsurf",
  };

  const SOURCE_LABELS = {
    directory_watcher: "Directory watcher",
    snapshot_reconciliation: "Snapshot reconciliation",
    baseline_snapshot: "Baseline snapshot",
  };

  function node(tag, className, text) {
    const el = document.createElement(tag);
    if (className) el.className = className;
    if (text !== undefined && text !== null) el.textContent = String(text);
    return el;
  }

  function clear(el) {
    if (el) el.replaceChildren();
  }

  function humanBytes(n) {
    const value = Math.abs(Number(n) || 0);
    if (!value) return null;
    if (value < 1024) return value + " B";
    const units = ["KB", "MB", "GB", "TB"];
    let v = value;
    let i = -1;
    do {
      v /= 1024;
      i += 1;
    } while (v >= 1024 && i < units.length - 1);
    return (n < 0 ? "-" : "") + (v >= 10 ? v.toFixed(0) : v.toFixed(1)) + " " + units[i];
  }

  function signedBytes(n) {
    if (!n) return "No net change";
    return (n > 0 ? "+" : "-") + humanBytes(n);
  }

  function clock(unix) {
    if (!unix) return "--:--:--";
    const d = new Date(Number(unix) * 1000);
    if (isNaN(d.getTime())) return "--:--:--";
    return d.toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  }

  function relativeAge(unix) {
    const secs = Math.floor(Date.now() / 1000) - Number(unix || 0);
    if (!Number.isFinite(secs) || secs < 0) return null;
    if (secs < 60) return "just now";
    if (secs < 3600) return Math.floor(secs / 60) + "m ago";
    if (secs < 86400) return Math.floor(secs / 3600) + "h ago";
    return Math.floor(secs / 86400) + "d ago";
  }

  function durationLabel(seconds) {
    const s = Math.max(0, Math.floor(Number(seconds) || 0));
    if (s >= 3600) return Math.floor(s / 3600) + "h " + String(Math.floor((s % 3600) / 60)).padStart(2, "0") + "m";
    if (s >= 60) return Math.floor(s / 60) + "m " + String(s % 60).padStart(2, "0") + "s";
    return s + "s";
  }

  function agentLabel(id) {
    return AGENT_LABELS[id] || String(id || "Agent");
  }

  function confidenceLabel(value) {
    const v = String(value || "").toLowerCase();
    if (v === "correlated") return "Correlated";
    if (v === "unknown") return "Unknown";
    return "Observed";
  }

  /* Short, honest classification of one process identity string. */
  function identityTone(identity) {
    const v = String(identity || "").toLowerCase();
    if (v.indexOf("suspect") !== -1) return "bad";
    if (v.indexOf("accessdenied") !== -1 || v.indexOf("denied") !== -1) return "bad";
    if (v.indexOf("verified") !== -1 && v.indexOf("unverified") === -1) return "ok";
    return "warn";
  }

  function identityShort(identity, fallbackLabel) {
    const v = String(identity || "").toLowerCase();
    if (v.indexOf("suspect") !== -1) return "Suspect identity";
    if (v.indexOf("accessdenied") !== -1 || v.indexOf("denied") !== -1) return "Access denied";
    if (v.indexOf("pathmissing") !== -1) return "Path missing";
    if (v.indexOf("imagenameonly") !== -1) return "Image name only";
    if (v.indexOf("verified") !== -1 && v.indexOf("unverified") === -1) return "Verified";
    return fallbackLabel || "Unverified";
  }

  function showError(message) {
    if (!els.error) return;
    els.error.textContent = message || "";
    els.error.classList.toggle("hidden", !message);
  }

  function setBusy(button, busy, label) {
    if (!button) return;
    if (busy) {
      button.dataset.idleLabel = button.textContent;
      button.textContent = label || "Working…";
      button.disabled = true;
    } else {
      if (button.dataset.idleLabel) button.textContent = button.dataset.idleLabel;
      delete button.dataset.idleLabel;
      button.disabled = false;
    }
  }

  function cleanError(err, fallback) {
    const text = err && (err.message || err.toString ? err.toString() : String(err));
    return String(text || fallback).replace(/^Error:\s*/i, "");
  }

  let actionStatusTimer = 0;
  /* Outcome line for explicit operator actions (process control). Every
     state change is reported: success, refusal, staleness, cancellation. */
  function setActionStatus(text, tone) {
    if (!els.actionStatus) return;
    els.actionStatus.textContent = text || "";
    els.actionStatus.dataset.tone = tone || "idle";
    els.actionStatus.classList.toggle("visible", !!text);
    clearTimeout(actionStatusTimer);
    if (text && (tone === "ok" || tone === "idle")) {
      actionStatusTimer = setTimeout(function () {
        els.actionStatus.classList.remove("visible");
        els.actionStatus.textContent = "";
      }, 8000);
    }
  }

  /* Scoped process control: confirm, then let the backend re-verify the
     live process identity before anything is signalled. The renderer can
     never bypass that check — the backend re-enumerates itself. */
  async function endProcessFlow(agent, proc) {
    if (!invoke || !proc || !proc.pid) return;
    if (state.scanning) {
      setActionStatus("A scan is running — try again when it settles.", "warn");
      return;
    }
    const hasWin = !!proc.has_windows;
    const confirmed = await (window.CureConfirm
      ? window.CureConfirm({
          kicker: "Process control",
          title: "End " + agent.label + " process?",
          facts: [
            ["PID", String(proc.pid)],
            ["Executable", sanitizeDisplayPath(proc.exe_path || proc.name || "unknown")],
            [
              "Started",
              proc.started_unix
                ? new Date(proc.started_unix * 1000).toLocaleString()
                : "unknown",
            ],
            ["Identity", proc.identity_label || identityShort(proc.identity)],
            [
              "Potential impact",
              "Active coding tasks and unsaved session work may be interrupted.",
            ],
          ],
          note: hasWin
            ? "Windows will first be asked to close gracefully (WM_CLOSE); if the process does not exit, it will be force-stopped."
            : "This process owns no windows, so it cannot close gracefully — it will be force-stopped (TerminateProcess).",
          okLabel: hasWin ? "End Selected Process" : "Force Stop",
        })
      : Promise.resolve(false));
    if (!confirmed) {
      setActionStatus(
        "Cancelled — no change was made to PID " + proc.pid + ".",
        "idle",
      );
      return;
    }
    setActionStatus("Ending PID " + proc.pid + "…", "busy");
    try {
      const receipt = await invoke("operator_agent_end_process", {
        pid: proc.pid,
        expectedName: proc.name || "",
        expectedStartedUnix: proc.started_unix || null,
        confirmationToken: "END_" + proc.pid,
      });
      if (receipt && receipt.verified_stopped) {
        setActionStatus(
          (receipt.method === "graceful" ? "Closed gracefully — " : "Force-stopped — ") +
            receipt.outcome,
          "ok",
        );
      } else {
        setActionStatus(
          receipt
            ? receipt.outcome
            : "Windows did not confirm the process exit. Verify manually.",
          "warn",
        );
      }
    } catch (err) {
      setActionStatus(cleanError(err, "The process could not be ended."), "bad");
    }
    // The snapshot must reflect reality after the action.
    scanAgents();
  }

  /* ---------------------------------------------------------------------
     Radar lifecycle
     --------------------------------------------------------------------- */

  function setPhase(phase) {
    if (els.radar) els.radar.dataset.phase = phase;
  }

  function radarPhase() {
    if (state.scanning) return "scanning";
    if (state.session && state.session.active) return "monitoring";
    return "idle";
  }

  function syncPhase() {
    setPhase(radarPhase());
    if (els.liveDot) {
      els.liveDot.dataset.state =
        state.scanning ? "scanning" : state.session && state.session.active ? "recording" : "idle";
    }
  }

  /* Fresh launch: an honest idle state. Nothing has been read, so nothing
     is reported — not even zeros that would read as "checked, clean". */
  function renderIdleRadar() {
    state.radar = null;
    state.scannedAt = null;
    clear(els.blips);
    if (els.detected) els.detected.textContent = "—";
    if (els.radarLabel) els.radarLabel.textContent = "not scanned";
    if (els.radarNote)
      els.radarNote.textContent =
        "Not scanned this session. Run a scan to enumerate supported agent processes.";
    if (els.scanAt) els.scanAt.textContent = "Not scanned this session";
    clear(els.agentList);
    els.agentList.classList.add("hidden");
    els.agentsEmpty.classList.remove("hidden");
    if (els.emptyTitle) els.emptyTitle.textContent = "Not scanned this session";
    if (els.emptyLead)
      els.emptyLead.textContent =
        "Scan Agents reads the local process table once and reports what it saw. Nothing is watched, started or stopped.";
    syncPhase();
  }

  async function scanAgents() {
    if (!invoke) return;
    if (state.scanning) {
      // One enumeration at a time; a request that lands mid-scan (e.g. the
      // refresh that follows a recording start/stop) re-runs afterwards so
      // the final snapshot reflects the newest lifecycle state.
      state.scanQueued = true;
      return;
    }
    state.scanning = true;
    const seq = ++scanSeq;
    syncPhase();
    showError("");
    setBusy(els.scanNow, true, "Scanning…");
    setBusy(els.refresh, true, "Scanning…");
    if (els.radarNote)
      els.radarNote.textContent = "Enumerating the local process table…";
    try {
      const payload = await invoke("agent_radar");
      if (seq !== scanSeq) return;
      // Session-artifact intelligence is a second, read-only command; the
      // radar still renders when it fails.
      try {
        const sessions = await invoke("operator_agent_sessions");
        if (seq !== scanSeq) return;
        state.sessionsByPid = {};
        (Array.isArray(sessions) ? sessions : []).forEach(function (detail) {
          state.sessionsByPid[detail.pid] = detail;
        });
      } catch (_) {
        state.sessionsByPid = {};
      }
      renderRadar(payload);
      showError("");
    } catch (err) {
      if (seq !== scanSeq) return;
      if (els.radarNote)
        els.radarNote.textContent =
          "The scan could not be read: " + cleanError(err, "backend unavailable");
      showError(cleanError(err, "The agent sample could not be read."));
    } finally {
      state.scanning = false;
      setBusy(els.scanNow, false);
      setBusy(els.refresh, false);
      syncPhase();
      if (state.scanQueued && !state.busy) {
        state.scanQueued = false;
        scanAgents();
      }
    }
  }

  function openAgentInspector(agent, proc) {
    const dlg = document.getElementById("ar-agent-inspector");
    const title = document.getElementById("ar-insp-title");
    const body = document.getElementById("ar-insp-body");
    const closeBtn = document.getElementById("ar-insp-close");
    if (!dlg || !body) return;

    const p = proc || (agent.processes && agent.processes[0]) || {};
    title.textContent = agent.label + " · Process Intelligence";
    body.innerHTML = "";

    const wrap = node("div", "ar-insp-content");

    // Top metadata card
    const card = node("div", "ar-insp-card");
    const badgeRow = node("div", "ar-insp-badges");
    if (agent.is_model_server) {
      badgeRow.appendChild(node("span", "chip chip-blue", "Model Server"));
    } else {
      badgeRow.appendChild(node("span", "chip chip-purple", "AI Coding Agent"));
    }
    const stBadge = node("span", "ar-agent-state", agent.state_label || "Present");
    stBadge.dataset.state = agent.state || "idle";
    badgeRow.appendChild(stBadge);

    const identLower = String(p.identity || "").toLowerCase();
    if (identLower.indexOf("suspect") !== -1) {
      badgeRow.appendChild(node("span", "chip chip-red", "Suspect Identity"));
    } else if (identLower.indexOf("accessdenied") !== -1 || identLower.indexOf("denied") !== -1) {
      badgeRow.appendChild(node("span", "chip chip-red", "Access Denied"));
    } else if (identLower.indexOf("unverified") !== -1 || identLower.indexOf("imagenameonly") !== -1 || identLower.indexOf("pathmissing") !== -1) {
      badgeRow.appendChild(node("span", "chip chip-amber", "Unverified Process"));
    } else if (identLower.indexOf("verified") !== -1) {
      badgeRow.appendChild(node("span", "chip chip-green", "Verified Tool"));
    } else {
      badgeRow.appendChild(node("span", "chip chip-amber", "Unconfirmed Identity"));
    }
    card.appendChild(badgeRow);

    if (agent.integration_note) {
      card.appendChild(node("p", "ar-insp-desc", agent.integration_note));
    }
    wrap.appendChild(card);

    // Process properties grid
    const pCard = node("div", "ar-insp-proc-card");
    pCard.appendChild(node("h4", "ar-insp-proc-title", "Process Telemetry · PID " + (p.pid || "N/A")));

    const grid = node("dl", "ar-insp-grid");
    function addProp(k, v, isCode) {
      grid.appendChild(node("dt", "ar-insp-key", k));
      const dd = node("dd", "ar-insp-val");
      if (isCode) {
        const c = node("code", "ar-insp-code", v || "N/A");
        dd.appendChild(c);
      } else {
        dd.textContent = v || "N/A";
      }
      grid.appendChild(dd);
    }

    addProp("Image Name", p.name || "Unknown");
    addProp("Executable Path", sanitizeDisplayPath(p.exe_path || "Unknown"), true);
    addProp("Identity Confidence", p.identity_label || p.identity || "Unknown");
    if (p.started_unix) {
      addProp("Start Time", new Date(p.started_unix * 1000).toLocaleString());
    }
    const mem = humanBytes(p.memory_usage_bytes);
    if (mem) addProp("Working Set (measured)", mem);
    if (p.cmdline_masked) {
      addProp("Command Line (Masked)", sanitizeDisplayPath(p.cmdline_masked), true);
    }
    addProp("Forensic Boundary", "Presence is not activity. Workspace modifications are recorded separately via Flight Recorder.");

    pCard.appendChild(grid);
    wrap.appendChild(pCard);

    // Operator Process & Session Controls
    const actCard = node("div", "ar-insp-proc-card");
    actCard.style.marginTop = "12px";
    actCard.appendChild(node("h4", "ar-insp-proc-title", "Operator Session Controls · PID Race Protected"));

    const actRow = node("div", "ar-insp-actions");
    actRow.style.display = "flex";
    actRow.style.gap = "10px";
    actRow.style.marginTop = "10px";

    const softBtn = node("button", "operator-action-btn btn-secondary", "Request Graceful Shutdown (WM_CLOSE)");
    softBtn.type = "button";
    softBtn.onclick = async function () {
      if (!p.pid) return;
      softBtn.disabled = true;
      softBtn.textContent = "Signaling...";
      try {
        const closed = await invoke("operator_agent_graceful_shutdown", { pid: p.pid });
        softBtn.textContent = closed ? "✔ Window Closed" : "Signal Sent";
        setTimeout(() => scanAgents(), 800);
      } catch (e) {
        alert("Graceful shutdown: " + e);
        softBtn.disabled = false;
        softBtn.textContent = "Request Graceful Shutdown (WM_CLOSE)";
      }
    };
    actRow.appendChild(softBtn);

    const termBtn = node("button", "operator-action-btn btn-danger", "End Process Tree");
    termBtn.type = "button";
    termBtn.onclick = async function () {
      if (!p.pid) return;
      const expectedToken = "TERMINATE_" + p.pid;
      const typed = prompt(
        "OPERATOR CONFIRMATION REQUIRED:\n" +
        "You are requesting the termination of process PID " + p.pid + " (" + (p.name || "") + ") and its child worker tree.\n\n" +
        "To confirm, type:\n" + expectedToken
      );
      if (typed !== expectedToken) {
        if (typed !== null) alert("Confirmation token mismatch. Termination aborted.");
        return;
      }

      termBtn.disabled = true;
      termBtn.textContent = "Terminating...";
      try {
        const res = await invoke("operator_agent_terminate", {
          pid: p.pid,
          expectedExe: p.name || "",
          expectedStartTime: p.started_unix || null,
          confirmationToken: typed,
        });
        alert(res.outcome || "Process tree terminated.");
        close();
        scanAgents();
      } catch (e) {
        alert("Termination refused: " + e);
        termBtn.disabled = false;
        termBtn.textContent = "End Process Tree";
      }
    };
    actRow.appendChild(termBtn);

    actCard.appendChild(actRow);
    wrap.appendChild(actCard);
    body.appendChild(wrap);

    dlg.classList.remove("hidden");
    if (typeof dlg.showModal === "function") {
      try { dlg.showModal(); } catch (_) {}
    }

    function close() {
      dlg.classList.add("hidden");
      if (typeof dlg.close === "function") {
        try { dlg.close(); } catch (_) {}
      }
    }

    if (closeBtn) closeBtn.onclick = close;
    dlg.onclick = function (e) {
      if (e.target === dlg) close();
    };
    dlg.onkeydown = function (e) {
      if (e.key === "Escape") close();
    };
  }

  /* Session-artifact intelligence: short, honest, from the real
     operator_agent_sessions survey — never invented per agent. */
  function sessionIntel(processes) {
    for (const proc of processes || []) {
      const detail = state.sessionsByPid[proc.pid];
      if (detail && detail.recovery) return detail.recovery;
    }
    return null;
  }

  function sessionIntelLine(recovery) {
    if (!recovery) return null;
    const kind = String(recovery.state_kind || "").toLowerCase();
    const count = Number(recovery.session_count) || 0;
    const age = recovery.latest_activity_unix ? relativeAge(recovery.latest_activity_unix) : null;
    let text;
    if (kind.indexOf("unsupported") !== -1) {
      text = "Session format unsupported for recovery";
      if (count) text += " · " + count + " artifact group(s)";
    } else if (kind.indexOf("active") !== -1) {
      text = "Active session artifacts" + (count ? " · " + count + " session(s)" : "");
      if (age) text += " · last activity " + age;
    } else if (kind.indexOf("recoverable") !== -1) {
      text = "Recoverable session artifacts" + (count ? " · " + count + " session(s)" : "");
      if (age) text += " · last activity " + age;
    } else if (kind.indexOf("existing") !== -1 || kind.indexOf("files") !== -1) {
      text = count ? "Session artifacts found (inactive)" : "No session artifacts found";
      if (age) text += " · last activity " + age;
    } else if (kind.indexOf("running") !== -1) {
      text = "Running · no session artifacts on disk";
    } else {
      text = recovery.status || "Session state unknown";
    }
    return text;
  }

  function renderRadar(payload) {
    state.radar = payload || null;
    if (!payload) return;
    state.scannedAt = Number(payload.generated_unix) || Math.floor(Date.now() / 1000);

    const agents = Array.isArray(payload.supported_agents)
      ? payload.supported_agents
      : [];
    const processCount = Array.isArray(payload.processes)
      ? payload.processes.length
      : 0;

    // Truthful classification: separate verified tools from unverified candidates & model servers
    let verifiedCount = 0;
    let candidateCount = 0;
    const modelServerCount = Number(payload.model_server_count) || 0;

    agents.forEach(function (a) {
      if (!a.detected) return;
      if (a.is_model_server) return;
      const isVerified = (a.processes || []).some(function (proc) {
        const idStr = String(proc.identity || "");
        return idStr.indexOf("Verified") !== -1 && idStr.indexOf("Unverified") === -1;
      });
      if (isVerified) {
        verifiedCount++;
      } else {
        candidateCount++;
      }
    });

    const detectedList = agents.filter(function (a) {
      return a.detected;
    });

    if (state.scannedAt && els.scanAt) {
      els.scanAt.textContent = "Snapshot " + clock(state.scannedAt);
    }

    if (verifiedCount > 0) {
      els.detected.textContent = String(verifiedCount);
      if (els.radarLabel) els.radarLabel.textContent = verifiedCount === 1 ? "verified agent" : "verified agents";
      els.radar.dataset.signal = "detected";
    } else if (candidateCount > 0) {
      els.detected.textContent = String(candidateCount);
      if (els.radarLabel) els.radarLabel.textContent = candidateCount === 1 ? "unverified candidate" : "unverified candidates";
      els.radar.dataset.signal = "detected";
    } else if (modelServerCount > 0) {
      els.detected.textContent = String(modelServerCount);
      if (els.radarLabel) els.radarLabel.textContent = modelServerCount === 1 ? "model server" : "model servers";
      els.radar.dataset.signal = "detected";
    } else {
      els.detected.textContent = "0";
      if (els.radarLabel) els.radarLabel.textContent = "agents detected";
      els.radar.dataset.signal = "none";
    }

    if (detectedCount(payload) === 0 && modelServerCount === 0) {
      els.radarNote.textContent =
        "No supported agent process in this snapshot. A process may start and stop between snapshots.";
      if (els.emptyTitle) els.emptyTitle.textContent = "No supported agents in this snapshot";
      if (els.emptyLead)
        els.emptyLead.textContent =
          "The process table was read at " +
          clock(state.scannedAt) +
          ". Start the tool normally, then scan again.";
    } else if (candidateCount > 0 && verifiedCount === 0) {
      els.radarNote.textContent =
        candidateCount +
        " agent candidate" +
        (candidateCount === 1 ? "" : "s") +
        " in " +
        processCount +
        " process" +
        (processCount === 1 ? "" : "es") +
        " (unverified identity — image name match only). Not confirmed as genuine.";
    } else if (verifiedCount > 0) {
      els.radarNote.textContent =
        verifiedCount +
        " verified agent" +
        (verifiedCount === 1 ? "" : "s") +
        (candidateCount > 0 ? " + " + candidateCount + " unverified" : "") +
        " in " +
        processCount +
        " process" +
        (processCount === 1 ? "" : "es") +
        ". Presence is not proof of activity.";
    } else {
      els.radarNote.textContent = modelServerCount + " local model server(s) running (Ollama).";
    }

    renderBlips(detectedList);
    renderAgentList(detectedList);

    els.agentList.classList.toggle("hidden", detectedList.length === 0);
    els.agentsEmpty.classList.toggle("hidden", detectedList.length > 0);

    if (payload.windows) {
      state.windows = payload.windows;
      renderWindows(payload.windows);
    }

    // A radar snapshot may only introduce a session the backend still
    // reports as active; it never resurrects a stopped recording over a
    // newer local state.
    if (payload.monitoring && payload.monitoring.active) {
      applySession(payload.monitoring);
    }
    syncPhase();
  }

  function detectedCount(payload) {
    return Number(payload.detected_count) || 0;
  }

  /* Deterministic, non-decorative placement: coding agents spread evenly
     on the outer band, model servers on the inner ring. No dots exist for
     agents the backend did not detect. */
  function renderBlips(detectedList) {
    clear(els.blips);
    const modelServers = detectedList.filter(function (a) {
      return a.is_model_server;
    });
    const coding = detectedList.filter(function (a) {
      return !a.is_model_server;
    });
    const showLabels = detectedList.length <= 5;

    coding.forEach(function (agent, i) {
      const angle = ((360 / Math.max(1, coding.length)) * i + 200) * (Math.PI / 180);
      const radius = 31 - (i % 2) * 6; // outer band, two lanes (percent of container)
      placeBlip(agent, angle, radius, false, showLabels, i);
    });
    modelServers.forEach(function (agent, i) {
      const angle = ((360 / Math.max(1, modelServers.length)) * i + 45) * (Math.PI / 180);
      placeBlip(agent, angle, 11, true, showLabels, i);
    });
  }

  function placeBlip(agent, angle, radius, isModel, showLabels, index) {
    const left = 50 + Math.cos(angle) * radius;
    const top = 50 + Math.sin(angle) * radius;
    const blip = node("button", "ar-blip" + (isModel ? " ar-blip-model-server" : ""));
    blip.type = "button";
    blip.dataset.agent = agent.id;
    blip.dataset.state = agent.state || "idle";
    const procs = agent.processes || [];
    const tone = isModel
      ? "info"
      : identityTone((procs[0] && procs[0].identity) || "");
    blip.dataset.tone = tone;
    blip.style.left = left + "%";
    blip.style.top = top + "%";
    if (left > 58) blip.dataset.side = "left";
    const isUnverified = tone === "warn";
    const labelText = agent.label + (isModel ? "" : isUnverified ? " ·unverified" : "");
    const label = node("span", "ar-blip-label", labelText);
    if (!showLabels) label.classList.add("is-compact");
    blip.appendChild(label);
    if ((agent.process_count || procs.length) > 1) {
      blip.appendChild(node("span", "ar-blip-count", String(agent.process_count || procs.length)));
    }
    blip.setAttribute(
      "aria-label",
      agent.label +
        " — " +
        (agent.process_count || procs.length) +
        " process(es), " +
        (agent.state_label || "present") +
        ". Activate to highlight details.",
    );
    blip.title =
      agent.label +
      " · " +
      (agent.process_count || procs.length || 1) +
      " process(es) · " +
      (agent.state_label || "present");
    blip.setAttribute("aria-pressed", String(state.selectedAgent === agent.id));
    if (state.selectedAgent === agent.id) blip.classList.add("is-selected");
    blip.addEventListener("click", function () {
      selectAgent(agent.id);
    });
    els.blips.appendChild(blip);
  }

  function selectAgent(id) {
    state.selectedAgent = state.selectedAgent === id ? null : id;
    const cards = els.agentList.querySelectorAll(".ar-agent-card");
    cards.forEach(function (card) {
      const match = state.selectedAgent && card.dataset.agent === state.selectedAgent;
      card.classList.toggle("is-selected", !!match);
      if (match) {
        expandCard(card, true);
        card.scrollIntoView({ block: "nearest", behavior: "smooth" });
      }
    });
    els.blips.querySelectorAll(".ar-blip").forEach(function (blip) {
      const on = state.selectedAgent && blip.dataset.agent === state.selectedAgent;
      blip.classList.toggle("is-selected", !!on);
      blip.setAttribute("aria-pressed", String(!!on));
    });
  }

  function expandCard(card, expand) {
    const body = card.querySelector(".ar-agent-body");
    const row = card.querySelector(".ar-agent-row");
    if (!body) return;
    const open = expand !== undefined ? expand : body.classList.contains("hidden");
    body.classList.toggle("hidden", !open);
    if (row) row.setAttribute("aria-expanded", String(open));
  }

  function renderAgentList(detectedList) {
    clear(els.agentList);
    detectedList.forEach(function (agent) {
      const li = node("li", "ar-agent-card");
      li.dataset.agent = agent.id;
      const procs = agent.processes || [];
      const tone = agent.is_model_server ? "info" : identityTone((procs[0] && procs[0].identity) || "");
      li.dataset.tone = tone;

      const row = node("button", "ar-agent-row");
      row.type = "button";
      row.setAttribute("aria-expanded", "false");
      row.appendChild(node("span", "ar-agent-dot"));
      row.appendChild(node("strong", "ar-agent-name", agent.label));

      const catChip = node("span", "chip", agent.is_model_server ? "Model server" : "Agent");
      catChip.classList.add(agent.is_model_server ? "chip-blue" : "chip-purple");
      row.appendChild(catChip);

      const badge = node("span", "ar-agent-state", agent.state_label || "Present");
      badge.dataset.state = agent.state || "idle";
      row.appendChild(badge);

      const memTotal = procs.reduce(function (sum, pr) {
        return sum + (Number(pr.memory_usage_bytes) || 0);
      }, 0);
      const memLabel = memTotal ? humanBytes(memTotal) : null;
      row.appendChild(
        node(
          "span",
          "ar-agent-meta",
          (agent.process_count || procs.length) +
            " proc" +
            ((agent.process_count || procs.length) === 1 ? "" : "s") +
            (memLabel ? " · " + memLabel : ""),
        ),
      );
      row.appendChild(node("span", "ar-agent-chevron", "▾", undefined));
      li.appendChild(row);

      const body = node("div", "ar-agent-body hidden");

      procs.forEach(function (proc) {
        const line = node("div", "ar-proc");
        const facts = node("div", "ar-proc-facts");
        facts.appendChild(node("span", "ar-proc-pid", "PID " + proc.pid));
        facts.appendChild(node("span", "ar-proc-name", proc.name || "process"));
        if (proc.started_unix) {
          facts.appendChild(node("span", "ar-proc-started", "started " + clock(proc.started_unix)));
        }
        const mem = humanBytes(proc.memory_usage_bytes);
        if (mem) facts.appendChild(node("span", "ar-proc-mem", mem + " working set"));
        const ident = node(
          "span",
          "ar-agent-note ar-proc-identity tone-" + identityTone(proc.identity),
          proc.identity_label || identityShort(proc.identity),
        );
        ident.title = proc.identity || "";
        facts.appendChild(ident);
        line.appendChild(facts);
        const path = node("code", "ar-proc-path", sanitizeDisplayPath(proc.exe_path));
        path.title = sanitizeDisplayPath(proc.exe_path);
        line.appendChild(path);
        // Scoped per-process controls: Details and a confirm-gated end for
        // THIS PID. Tree termination stays in the inspector, separately
        // authorised; nothing is terminated without the operator.
        const actions = node("div", "ar-proc-actions");
        const detailsBtn = node("button", "btn btn-ghost ar-proc-details", "Details");
        detailsBtn.type = "button";
        detailsBtn.addEventListener("click", function (ev) {
          ev.stopPropagation();
          openAgentInspector(agent, proc);
        });
        actions.appendChild(detailsBtn);
        const endBtn = node(
          "button",
          "btn btn-ghost ar-proc-end",
          proc.has_windows ? "End process" : "Force stop",
        );
        endBtn.type = "button";
        endBtn.title = proc.has_windows
          ? "Ask this process to close, then force-stop if needed. Confirmed first."
          : "Force-stop this process (it owns no windows to close gracefully). Confirmed first.";
        endBtn.addEventListener("click", function (ev) {
          ev.stopPropagation();
          endProcessFlow(agent, proc);
        });
        actions.appendChild(endBtn);
        line.appendChild(actions);
        body.appendChild(line);
      });

      const recovery = sessionIntel(procs);
      const intelLine = sessionIntelLine(recovery);
      if (intelLine) {
        const intel = node(
          "p",
          "ar-agent-session",
          "Session artifacts: " + intelLine,
        );
        if (recovery && recovery.details) intel.title = recovery.details;
        body.appendChild(intel);
      }

      if (agent.state_note) body.appendChild(node("p", "ar-agent-note", agent.state_note));
      if (agent.integration_note) body.appendChild(node("p", "ar-agent-note", agent.integration_note));

      li.appendChild(body);
      row.addEventListener("click", function () {
        expandCard(li);
      });
      els.agentList.appendChild(li);
    });

    if (state.selectedAgent) selectAgent(state.selectedAgent);
  }

  /* ---------------------------------------------------------------------
     Flight session
     --------------------------------------------------------------------- */

  function eventCounts(events) {
    const counts = {
      total: 0,
      created: 0,
      modified: 0,
      removed: 0,
      renamed: 0,
      observed: 0,
      correlated: 0,
      unknown: 0,
      net: 0,
    };
    (events || []).forEach(function (event) {
      counts.total += 1;
      if (counts[event.kind] !== undefined) counts[event.kind] += 1;
      const confidence = String(event.confidence || "").toLowerCase();
      if (counts[confidence] !== undefined) counts[confidence] += 1;
      const before = Number(event.size_before) || 0;
      const after = Number(event.size_after) || 0;
      if (event.kind === "created") counts.net += after;
      else if (event.kind === "removed") counts.net -= before;
      else if (event.kind === "modified") counts.net += after - before;
      else if (event.kind === "renamed") counts.net += after - before;
    });
    return counts;
  }

  function applySession(session) {
    if (!session) return;
    const changed =
      !state.session ||
      state.session.id !== session.id ||
      state.session.events.length !== session.events.length ||
      state.session.active !== session.active;
    state.session = session;
    if (changed) {
      if (state.scrub !== null) {
        state.scrub = Math.min(state.scrub, session.events.length);
      }
      if (state.selectedSeq !== null) {
        const still = session.events.some(function (e) {
          return e.seq === state.selectedSeq;
        });
        if (!still) state.selectedSeq = null;
      }
      if (state.caseSignature && state.caseSignature !== session.id + ":" + session.events.length) {
        resetCasePreview("Session data changed. Preview the case file again.");
      }
    }
    renderSession();
    syncPhase();
  }

  function renderSession() {
    const session = state.session;
    const active = !!(session && session.active);
    const hasSession = !!session;
    // A recovered previous recording starts collapsed: durable evidence,
    // clearly stopped, never resumed.
    const collapsed = hasSession && !active && state.recovered && !state.historyOpen;

    els.stop.classList.toggle("hidden", !active);
    els.start.classList.toggle("hidden", active);
    els.start.disabled = active || state.busy;
    els.workspace.disabled = active;
    els.sessionBadge.dataset.state = active ? "recording" : hasSession ? "stopped" : "idle";
    els.sessionBadge.textContent = active ? "Recording" : hasSession ? "Stopped" : "Idle";
    els.sessionSub.textContent = active
      ? "Recording metadata changes in the selected workspace."
      : hasSession
        ? "Session stopped. Review the evidence, then export if needed."
        : "Nothing is watched until you start a session.";

    if (els.history) {
      els.history.classList.toggle("hidden", !collapsed);
      if (collapsed && session) {
        const counts = eventCounts(session.events || []);
        const ended = session.ended_unix
          ? new Date(session.ended_unix * 1000).toLocaleString()
          : "unknown time";
        els.historyMeta.textContent =
          sanitizeDisplayPath(session.workspace || "") +
          " · " +
          counts.total +
          " event(s) · stopped " +
          ended;
      }
    }
    if (els.historyOpen) {
      els.historyOpen.classList.toggle("hidden", !collapsed);
    }

    els.flightEmpty.classList.toggle("hidden", hasSession && !collapsed);
    els.flight.classList.toggle("hidden", !hasSession || collapsed);

    // Replay and export exist only for a stopped session, on request.
    const toolsVisible = hasSession && !active && !collapsed;
    if (els.reviewToggle) {
      els.reviewToggle.classList.toggle("hidden", !toolsVisible);
      els.reviewToggle.textContent = state.reviewMode ? "Close review" : "Review timeline";
      els.reviewToggle.setAttribute("aria-pressed", String(state.reviewMode));
    }
    if (els.exportToggle) {
      els.exportToggle.classList.toggle("hidden", !toolsVisible);
      els.exportToggle.setAttribute("aria-pressed", String(state.exportOpen));
    }
    if (els.review) {
      els.review.classList.toggle("hidden", !state.reviewMode);
    }
    if (els.exportMenu) {
      els.exportMenu.classList.toggle("hidden", !state.exportOpen);
    }

    els.casePreview.disabled = !hasSession || active;
    els.caseExport.disabled =
      active ||
      !state.caseDataUrl ||
      !state.caseSignature ||
      state.caseSignature !== session.id + ":" + session.events.length;

    if (!hasSession) {
      if (els.sessionDuration) els.sessionDuration.textContent = "—";
      els.caseStatus.textContent = "Stop a monitoring session to unlock the case file.";
      return;
    }

    if (!els.workspace.value) els.workspace.value = session.workspace || "";
    if (!els.caseSummary.value && !els.caseSignature) {
      els.caseStatus.textContent = active
        ? "Recording in progress. Stop the session to build the case file."
        : "Session complete. Preview the allowlisted dossier, then export if desired.";
    }

    const events = Array.isArray(session.events) ? session.events : [];
    const visible =
      state.scrub === null ? events.slice() : events.slice(0, state.scrub);
    const counts = eventCounts(visible);

    els.statTotal.textContent = String(counts.total);
    els.statCreated.textContent = String(counts.created);
    els.statModified.textContent = String(counts.modified);
    els.statRemoved.textContent = String(counts.removed);
    els.statBytes.textContent = signedBytes(counts.net);
    if (els.sessionDuration) {
      els.sessionDuration.textContent = durationLabel(elapsed(session));
    }

    const failures = [];
    if (counts.unknown) failures.push(counts.unknown + " unknown-context");
    if (counts.correlated) {
      els.statConfidence.textContent =
        failures.length === 0 ? "Correlated" : "Correlated / partial";
    } else if (counts.total) {
      els.statConfidence.textContent = failures.length ? "Observed / partial" : "Observed";
    } else {
      els.statConfidence.textContent = "No changes";
    }

    const coverage = session.coverage || {};
    const complete = coverage.complete === true;
    els.coverage.dataset.state = complete ? "complete" : "partial";
    let coverageText = "";
    if (complete) {
      coverageText =
        "Coverage complete within recorder limits · " +
        (coverage.baseline_files || 0) +
        " baseline files · watcher " +
        (coverage.watcher_started ? "started" : "not confirmed") +
        (coverage.snapshot_reconciled ? " · final reconciliation used" : "");
    } else {
      coverageText =
        coverage.note ||
        "Observation coverage is incomplete. Missing changes cannot be inferred.";
    }
    if (coverage.overflow_batches) {
      coverageText +=
        " · " +
        coverage.overflow_batches +
        " watcher overflow batch(es)" +
        (coverage.dropped_events ? ", " + coverage.dropped_events + " dropped event(s)" : "");
    }
    els.coverage.textContent = coverageText;

    const max = events.length;
    els.scrub.max = String(max);
    els.scrub.value = String(state.scrub === null ? max : Math.min(state.scrub, max));
    els.scrub.disabled = max === 0;
    if (state.scrub === null) {
      els.scrubLabel.textContent = active ? "Live · " + durationLabel(elapsed(session)) : "Final";
    } else {
      els.scrubLabel.textContent = state.scrub + " / " + max + " observations";
    }

    renderTimeline(events);
    renderEvents(visible);
    if (state.selectedSeq !== null) renderEvidence();

    if (!active && !state.caseSignature) {
      els.caseStatus.textContent =
        "Session complete · " +
        counts.total +
        " observed change" +
        (counts.total === 1 ? "" : "s") +
        " · coverage " +
        (complete ? "complete within limits" : "incomplete") +
        ". Preview the allowlisted dossier before export.";
    }
  }

  function elapsed(session) {
    const start = Number(session.started_unix) || 0;
    const end = Number(session.ended_unix) || Math.floor(Date.now() / 1000);
    return Math.max(0, end - start);
  }

  function renderTimeline(events) {
    clear(els.timeline);
    if (!events.length) {
      els.timeline.appendChild(
        node("span", "ar-win-empty", "No observed changes to place on the timeline."),
      );
      return;
    }
    const session = state.session;
    const start = Number(session.started_unix) || Number(events[0].at_unix) || 0;
    const end = Number(session.ended_unix) ||
      Number(events[events.length - 1].at_unix) ||
      start;
    const span = Math.max(1, end - start);

    events.forEach(function (event, index) {
      const tick = node("button", "ar-tick");
      tick.type = "button";
      tick.dataset.kind = event.kind;
      tick.dataset.confidence = event.confidence;
      const offset = Math.min(100, Math.max(0, ((Number(event.at_unix) - start) / span) * 100));
      tick.style.left = offset + "%";
      tick.title = clock(event.at_unix) + " · " + event.kind + " · " + event.path;
      tick.setAttribute(
        "aria-label",
        "Observation " +
          (index + 1) +
          " of " +
          events.length +
          ": " +
          event.kind +
          " at " +
          clock(event.at_unix),
      );
      if (state.scrub !== null && index >= state.scrub) tick.classList.add("hidden");
      if (event.seq === state.selectedSeq) tick.classList.add("is-selected");
      tick.addEventListener("click", function () {
        state.scrub = index + 1;
        state.selectedSeq = event.seq;
        renderSession();
      });
      els.timeline.appendChild(tick);
    });
  }

  function filteredEvents(events) {
    if (state.filter === "all") return events;
    if (state.filter === "correlated") {
      return events.filter(function (event) {
        return String(event.confidence).toLowerCase() === "correlated";
      });
    }
    return events.filter(function (event) {
      return event.kind === state.filter;
    });
  }

  function renderEvents(visibleEvents) {
    const filtered = filteredEvents(visibleEvents);
    const reversed = filtered.slice().reverse();
    clear(els.eventList);

    reversed.forEach(function (event) {
      const li = node("li");
      const btn = node("button", "ar-event");
      btn.type = "button";
      btn.dataset.seq = String(event.seq);
      if (event.seq === state.selectedSeq) btn.setAttribute("aria-current", "true");
      btn.appendChild(node("span", "ar-event-time", clock(event.at_unix)));
      const kind = node("span", "ar-event-kind", event.kind);
      kind.dataset.kind = event.kind;
      btn.appendChild(kind);
      const path = node("span", "ar-event-path", event.path || "(renamed)");
      path.title = event.path || "";
      btn.appendChild(path);
      btn.appendChild(
        node("span", "ar-event-conf", confidenceLabel(event.confidence)),
      );
      btn.addEventListener("click", function () {
        state.selectedSeq = event.seq;
        renderSession();
      });
      li.appendChild(btn);
      els.eventList.appendChild(li);
    });

    els.eventList.classList.toggle("hidden", reversed.length === 0);
    els.eventEmpty.classList.toggle("hidden", reversed.length > 0);
    if (reversed.length === 0) {
      const title = els.eventEmpty.querySelector("strong");
      const body = els.eventEmpty.querySelector("span");
      if (visibleEvents.length === 0) {
        title.textContent = "No changes observed yet";
        body.textContent = session_active()
          ? "Save a file, run a build, or let the agent edit the workspace."
          : "This session recorded no supported filesystem changes within the recorder's limits.";
      } else {
        title.textContent = "No observations match this filter";
        body.textContent =
          "There are " + visibleEvents.length + " recorded observation(s) in this replay position. Choose All to see them.";
      }
    }
  }

  function session_active() {
    return !!(state.session && state.session.active);
  }

  function renderEvidence() {
    const session = state.session;
    if (!session) return;
    const event = (session.events || []).find(function (e) {
      return e.seq === state.selectedSeq;
    });
    if (!event) {
      els.evidenceCard.classList.add("hidden");
      els.evidenceEmpty.classList.remove("hidden");
      return;
    }

    clear(els.evidenceCard);
    els.evidenceEmpty.classList.add("hidden");
    els.evidenceCard.classList.remove("hidden");
    els.evidenceCard.appendChild(
      node("h3", null, "Observation " + event.seq + " · " + event.kind),
    );
    els.evidenceCard.appendChild(
      node("p", "ar-evidence-path", "Workspace-relative path (local only): " + (event.path || "(none)")),
    );

    const dl = node("dl");
    const rows = [
      ["Observed at", clock(event.at_unix)],
      ["Change", String(event.kind)],
      ["Evidence", confidenceLabel(event.confidence)],
      ["Source", SOURCE_LABELS[event.source] || String(event.source || "unknown")],
      ["Workspace", session.workspace ? sanitizeDisplayPath(session.workspace) : "Not recorded"],
      ["Size before", event.size_before === null || event.size_before === undefined ? "Not captured" : humanBytes(event.size_before)],
      ["Size after", event.size_after === null || event.size_after === undefined ? "Not captured" : humanBytes(event.size_after)],
    ];
    if (event.from_path) rows.push(["From path", event.from_path]);
    rows.push([
      "Agents in sample",
      (event.agents || []).length
        ? (event.agents || []).map(agentLabel).join(", ")
        : "None detected",
    ]);
    rows.forEach(function (row) {
      dl.appendChild(node("dt", null, row[0]));
      dl.appendChild(node("dd", null, row[1]));
    });
    els.evidenceCard.appendChild(dl);

    if (event.note) els.evidenceCard.appendChild(node("p", "ar-evidence-note", event.note));
    els.evidenceCard.appendChild(
      node(
        "p",
        "ar-evidence-note",
        event.confidence === "correlated"
          ? "Attribution is not established. Agent presence and this change share a time window only."
          : event.confidence === "unknown"
            ? "Coverage was incomplete. This observation may be missing context."
            : "No supported agent process was present in the same sample.",
      ),
    );
  }

  /* ---------------------------------------------------------------------
     Windows persistence comparison
     --------------------------------------------------------------------- */

  function renderWindows(changes) {
    if (!changes) return;
    state.windows = changes;
    const added = changes.added || [];
    const removed = changes.removed || [];
    const changed = changes.changed || [];
    const total = added.length + removed.length + changed.length;

    els.winState.textContent = changes.snapshot_saved_at_unix
      ? "Compared against trusted snapshot from " +
        clock(changes.snapshot_saved_at_unix) +
        " · " +
        (changes.entries_before || 0) +
        " → " +
        (changes.entries_after || 0) +
        " supported entries."
      : "No trusted persistence snapshot is stored yet. Capture one before making changes.";

    clear(els.winSummary);
    [
      ["Added", added.length],
      ["Removed", removed.length],
      ["Changed", changed.length],
      ["Total", total],
    ].forEach(function (row) {
      const chip = node("span", "ar-win-count", row[0]);
      chip.appendChild(node("b", null, String(row[1])));
      els.winSummary.appendChild(chip);
    });
    els.winSummary.classList.toggle("hidden", false);

    clear(els.winLists);
    [
      ["Added", added],
      ["Removed", removed],
      ["Changed", changed],
    ].forEach(function (group) {
      const section = node("section", "ar-win-group");
      section.appendChild(node("h3", null, group[0] + " · " + group[1].length));
      if (!group[1].length) {
        section.appendChild(node("p", "ar-win-empty", "Nothing in this group."));
      }
      group[1].forEach(function (item) {
        const card = node("div", "ar-win-item");
        card.appendChild(node("strong", null, item.name || "(unnamed entry)"));
        card.appendChild(node("span", null, item.source_label || item.source || ""));
        if (item.location_after || item.location_before) {
          card.appendChild(
            node("p", null, item.location_after || item.location_before),
          );
        }
        if (item.command_after || item.command_before) {
          const details = node("details");
          details.appendChild(node("summary", null, "Command / evidence"));
          details.appendChild(node("code", null, item.command_after || item.command_before));
          (item.review_hints || []).forEach(function (hint) {
            details.appendChild(node("p", null, hint));
          });
          if (item.evidence) details.appendChild(node("p", null, item.evidence));
          card.appendChild(details);
        } else {
          (item.review_hints || []).forEach(function (hint) {
            card.appendChild(node("p", null, hint));
          });
        }
        section.appendChild(card);
      });
      els.winLists.appendChild(section);
    });

    if (!changes.complete && changes.note) {
      els.winState.textContent += " " + changes.note;
    }
  }

  async function captureWindows() {
    if (!invoke) return;
    setBusy(els.winCapture, true, "Capturing…");
    try {
      const summary = await invoke("windows_change_capture");
      els.winState.textContent =
        "Trusted snapshot saved " +
        clock(summary.saved_at_unix) +
        " · " +
        summary.entry_count +
        " supported entries · " +
        (summary.note || "");
      scanAgents();
    } catch (err) {
      els.winState.textContent = cleanError(err, "The snapshot could not be captured.");
    } finally {
      setBusy(els.winCapture, false);
    }
  }

  async function compareWindows() {
    if (!invoke) return;
    setBusy(els.winCompare, true, "Comparing…");
    try {
      const changes = await invoke("windows_change_compare");
      renderWindows(changes);
    } catch (err) {
      els.winState.textContent = cleanError(err, "The comparison could not be completed.");
    } finally {
      setBusy(els.winCompare, false);
    }
  }

  /* ---------------------------------------------------------------------
     Case file
     --------------------------------------------------------------------- */

  function resetCasePreview(message) {
    state.caseModel = null;
    state.caseDataUrl = null;
    state.caseSignature = "";
    els.caseExport.disabled = true;
    clear(els.caseStage);
    if (message) els.caseStatus.textContent = message;
  }

  async function previewCase() {
    if (!invoke || !state.session || state.session.active) return;
    const include = !!(els.caseInclude && els.caseInclude.checked);
    const summary = els.caseSummary ? els.caseSummary.value.trim() : "";
    setBusy(els.casePreview, true, "Building…");
    try {
      const caseFile = await invoke("agent_case_file", {
        summary: summary,
        includeSummary: include,
      });
      if (!window.CureCaseFile || typeof window.CureCaseFile.render !== "function") {
        throw new Error("The case file renderer is not available in this build.");
      }
      const rendered = window.CureCaseFile.render(caseFile);
      state.caseModel = caseFile;
      state.caseDataUrl = rendered.dataUrl;
      state.caseSignature = state.session.id + ":" + state.session.events.length;

      clear(els.caseStage);
      const img = node("img", "ar-case-image");
      img.src = rendered.dataUrl;
      img.alt =
        "Agent case file preview. " +
        (caseFile.title || "Agent session case") +
        ". " +
        caseFile.changes_total +
        " observed changes. " +
        (caseFile.confidence_label || "");
      els.caseStage.appendChild(img);
      els.caseStage.appendChild(
        node(
          "p",
          "ar-case-note",
          "Preview only. The engine adds no paths, file names, prompts, credentials or machine identifiers. The optional note is free text and appears only when you include it.",
        ),
      );

      els.caseExport.disabled = false;
      const withheld = caseFile.summary_withheld
        ? " The operator note was withheld because it looked like it contained a path or address."
        : "";
      els.caseStatus.textContent =
        "Preview ready · " +
        caseFile.changes_total +
        " change(s) · " +
        (caseFile.confidence_label || "no confidence label") +
        "." +
        withheld;
    } catch (err) {
      resetCasePreview(cleanError(err, "The case file could not be built."));
    } finally {
      setBusy(els.casePreview, false);
    }
  }

  async function exportCase() {
    if (!invoke || !state.caseDataUrl) return;
    if (!state.caseSignature || state.caseSignature !== state.session.id + ":" + state.session.events.length) {
      resetCasePreview("The session changed. Preview the case file again before export.");
      return;
    }
    setBusy(els.caseExport, true, "Saving…");
    try {
      const base64 = state.caseDataUrl.split(",")[1] || "";
      const path = await invoke("storage_save_case_file", {
        name: "cure-agent-case-" + String(state.caseModel.case_id || "case"),
        dataBase64: base64,
      });
      els.caseStatus.textContent =
        "Case file written to " + path + " · Nothing was uploaded.";
    } catch (err) {
      els.caseStatus.textContent = cleanError(err, "The case file could not be written.");
    } finally {
      setBusy(els.caseExport, false);
    }
  }

  /* ---------------------------------------------------------------------
     Session actions
     --------------------------------------------------------------------- */

  async function startSession() {
    if (state.busy) return;
    const workspace = (els.workspace.value || "").trim();
    if (!workspace) {
      showError("Choose an existing workspace folder before monitoring.");
      els.workspace.focus();
      return;
    }
    state.busy = true;
    state.everActive = true;
    state.recovered = false;
    state.historyOpen = false;
    state.reviewMode = false;
    showError("");
    setBusy(els.start, true, "Capturing baseline…");
    try {
      const session = await invoke("flight_start", { workspace: workspace });
      state.session = session;
      state.scrub = null;
      state.selectedSeq = null;
      resetCasePreview("");
      renderSession();
      syncPhase();
      els.caseStatus.textContent =
        "Recording. Stop the session to review final reconciliation and unlock the case file.";
      scanAgents();
    } catch (err) {
      showError(cleanError(err, "The monitoring session could not start."));
    } finally {
      state.busy = false;
      setBusy(els.start, false);
      renderSession();
      syncPhase();
      // A stop requested while the start was still running is honoured as
      // soon as the session exists — a click is never silently dropped.
      if (state.stopQueued) {
        state.stopQueued = false;
        stopSession();
      }
    }
  }

  async function stopSession() {
    if (state.busy) {
      state.stopQueued = true;
      return;
    }
    // This invocation covers any queued request; a double click must not
    // fire a second stop that would only produce a spurious error.
    state.stopQueued = false;
    state.busy = true;
    setBusy(els.stop, true, "Reconciling…");
    try {
      const session = await invoke("flight_stop");
      state.session = session;
      state.scrub = null;
      state.selectedSeq = null;
      renderSession();
      syncPhase();
      els.caseStatus.textContent = session.coverage && session.coverage.complete
        ? "Recording stopped. Final reconciliation finished within recorder limits. Preview the case file."
        : "Recording stopped. Coverage is incomplete; read the coverage note before interpreting the result.";
      scanAgents();
    } catch (err) {
      showError(cleanError(err, "The monitoring session could not be stopped cleanly."));
    } finally {
      state.busy = false;
      setBusy(els.stop, false);
      renderSession();
      syncPhase();
    }
  }

  async function refreshStatus(quiet) {
    if (!invoke) return;
    try {
      const session = await invoke("flight_status");
      if (session) {
        // A session the operator never started in this process is durable
        // history recovered from disk: collapsed and clearly stopped.
        if (!session.active && !state.everActive) state.recovered = true;
        if (session.active) state.everActive = true;
        applySession(session);
      }
      if (!state.session) renderSession();
    } catch (err) {
      if (!quiet) showError(cleanError(err, "Session state is unavailable."));
    }
  }

  /* ---------------------------------------------------------------------
     Wiring
     --------------------------------------------------------------------- */

  if (els.refresh) {
    els.refresh.addEventListener("click", function () {
      scanAgents();
    });
  }
  if (els.scanNow) {
    els.scanNow.addEventListener("click", function () {
      scanAgents();
    });
  }
  els.start.addEventListener("click", startSession);
  els.stop.addEventListener("click", stopSession);
  els.workspace.addEventListener("keydown", function (event) {
    if (event.key === "Enter" && !els.start.classList.contains("hidden")) {
      event.preventDefault();
      startSession();
    }
  });
  els.winCapture.addEventListener("click", captureWindows);
  els.winCompare.addEventListener("click", compareWindows);
  els.casePreview.addEventListener("click", previewCase);
  els.caseExport.addEventListener("click", exportCase);

  const exportDocFormat = function (format) {
    if (!invoke || !state.caseModel) return;
    invoke("export_agent_case_file", { case: state.caseModel, format: format }).then(function (path) {
      els.caseStatus.textContent = "Exported " + format.toUpperCase() + " to: " + path;
    }).catch(function (e) {
      showError(cleanError(e, "Export failed."));
    });
  };

  const btnMd = $("ar-case-export-md");
  if (btnMd) btnMd.addEventListener("click", () => exportDocFormat("markdown"));
  const btnJson = $("ar-case-export-json");
  if (btnJson) btnJson.addEventListener("click", () => exportDocFormat("json"));
  const btnHtml = $("ar-case-export-html");
  if (btnHtml) btnHtml.addEventListener("click", () => exportDocFormat("html"));

  els.scrub.addEventListener("input", function () {
    const max = Number(els.scrub.max) || 0;
    const value = Number(els.scrub.value) || 0;
    state.scrub = value >= max ? null : value;
    if (state.scrub !== null) {
      const visible = (state.session.events || []).slice(0, state.scrub);
      const last = visible[visible.length - 1];
      state.selectedSeq = last ? last.seq : null;
    }
    renderSession();
  });

  els.filters.addEventListener("click", function (event) {
    const button = event.target.closest("[data-filter]");
    if (!button) return;
    state.filter = button.getAttribute("data-filter") || "all";
    els.filters.querySelectorAll("[data-filter]").forEach(function (chip) {
      const on = chip === button;
      chip.classList.toggle("is-on", on);
      chip.setAttribute("aria-pressed", String(on));
    });
    renderSession();
  });

  els.caseSummary.addEventListener("input", function () {
    if (state.caseSignature) resetCasePreview("Inputs changed. Preview the case file again.");
  });
  els.caseInclude.addEventListener("change", function () {
    if (state.caseSignature) resetCasePreview("Inputs changed. Preview the case file again.");
  });

  // Replay controls exist only in explicit review mode.
  if (els.reviewToggle) {
    els.reviewToggle.addEventListener("click", function () {
      state.reviewMode = !state.reviewMode;
      if (state.reviewMode) state.historyOpen = true;
      else {
        // Closing review returns the list to the live position.
        state.scrub = null;
        state.selectedSeq = null;
      }
      renderSession();
    });
  }
  if (els.exportToggle) {
    els.exportToggle.addEventListener("click", function () {
      state.exportOpen = !state.exportOpen;
      if (state.exportOpen) state.historyOpen = true;
      renderSession();
    });
  }
  if (els.historyOpen) {
    els.historyOpen.addEventListener("click", function () {
      state.historyOpen = true;
      renderSession();
    });
  }

  if (window.__CURE_FORCE_MOCK) {
    els.demo.classList.remove("hidden");
  }

  if (!invoke) {
    els.radarNote.textContent = "The Agent Radar is available in the desktop build.";
    els.sessionSub.textContent = "This build has no local engine.";
    els.start.disabled = true;
    if (els.scanNow) els.scanNow.disabled = true;
    return;
  }

  let tickTimer = 0;
  function tick() {
    if (!tickTimer) return;
    if (view.classList.contains("hidden")) return;
    // In-process session state is cheap and safe to poll; the process
    // table is NEVER re-enumerated on a timer — detection happens only
    // when the operator clicks Scan Agents / Refresh.
    refreshStatus(true);
    if (state.session && state.session.active && els.sessionDuration) {
      els.sessionDuration.textContent = durationLabel(elapsed(state.session));
    }
    if (state.session && state.session.active) {
      syncPhase();
    }
  }

  if (listen) {
    listen("flight-update", function () {
      if (view.classList.contains("hidden")) return;
      refreshStatus(true);
    }).catch(function () {
      /* A missing listener only means the UI falls back to its interval. */
    });
  }

  window.addEventListener("beforeunload", function () {
    if (tickTimer) clearInterval(tickTimer);
  });

  window.CureAgentRadar = {
    refresh: function () {
      // Navigation must not auto-enumerate the process table. Session
      // state is refreshed; the radar keeps its honest last-snapshot.
      refreshStatus(true);
      syncPhase();
    },
    scan: function () {
      return scanAgents();
    },
    state: state,
  };

  renderIdleRadar();
  renderSession();
  refreshStatus(true);
  tickTimer = window.setInterval(tick, 3000);
})();
