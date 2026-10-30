/* ==========================================================================
   C.U.R.E — AI Agent Radar, Flight Recorder, Windows Change Radar
   --------------------------------------------------------------------------
   A process sighting is not an activity claim. A filesystem change observed
   while an agent was running is correlated, never attributed. This module
   only renders what the backend returns; it does not invent sessions,
   changes, timestamps or verdicts.
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
    blips: $("ar-blips"),
    radarNote: $("ar-radar-note"),
    agentList: $("ar-agent-list"),
    agentsEmpty: $("ar-agents-empty"),
    refresh: $("ar-refresh"),
    jump: $("ar-jump-monitor"),
    sessionSub: $("ar-session-sub"),
    sessionBadge: $("ar-session-badge"),
    workspace: $("ar-workspace"),
    start: $("ar-start"),
    stop: $("ar-stop"),
    error: $("ar-workspace-error"),
    flightEmpty: $("ar-flight-empty"),
    flight: $("ar-flight-live"),
    statTotal: $("ar-stat-total"),
    statCreated: $("ar-stat-created"),
    statModified: $("ar-stat-modified"),
    statRemoved: $("ar-stat-removed"),
    statBytes: $("ar-stat-bytes"),
    statConfidence: $("ar-stat-confidence"),
    coverage: $("ar-coverage"),
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
  };

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

  /* ---------------------------------------------------------------------
     Radar
     --------------------------------------------------------------------- */


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
    if (p.cmdline_masked) {
      addProp("Command Line (Masked)", sanitizeDisplayPath(p.cmdline_masked), true);
    }
    addProp("Forensic Boundary", "Presence is not activity. Workspace modifications are recorded separately via Flight Recorder.");

    pCard.appendChild(grid);
    wrap.appendChild(pCard);
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

  function renderRadar(payload) {
    state.radar = payload || null;
    if (!payload) return;

    const agents = Array.isArray(payload.supported_agents)
      ? payload.supported_agents
      : [];
    const detected = Number(payload.detected_count) || 0;
    const processCount = Array.isArray(payload.processes)
      ? payload.processes.length
      : 0;

    // Truthful classification: separate verified tools from unverified candidates & model servers
    let verifiedCount = 0;
    let candidateCount = 0;
    let modelServerCount = Number(payload.model_server_count) || 0;

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

    const radarLabelEl = $("ar-radar-label");
    if (verifiedCount > 0) {
      els.detected.textContent = String(verifiedCount);
      if (radarLabelEl) radarLabelEl.textContent = verifiedCount === 1 ? "verified agent" : "verified agents";
      els.radar.dataset.signal = "detected";
    } else if (candidateCount > 0) {
      els.detected.textContent = String(candidateCount);
      if (radarLabelEl) radarLabelEl.textContent = candidateCount === 1 ? "unverified candidate" : "unverified candidates";
      els.radar.dataset.signal = "detected";
    } else if (modelServerCount > 0) {
      els.detected.textContent = String(modelServerCount);
      if (radarLabelEl) radarLabelEl.textContent = modelServerCount === 1 ? "model server" : "model servers";
      els.radar.dataset.signal = "detected";
    } else {
      els.detected.textContent = "0";
      if (radarLabelEl) radarLabelEl.textContent = "agents detected";
      els.radar.dataset.signal = "none";
    }

    if (detected === 0 && modelServerCount === 0) {
      els.radarNote.textContent =
        "No supported agent process detected in this sample. A process may start and stop between samples.";
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

    clear(els.blips);
    const positions = [
      { angle: 212, radius: 32 },
      { angle: 330, radius: 38 },
      { angle: 140, radius: 34 },
      { angle: 35,  radius: 36 },
    ];
    let agentIdx = 0;
    let modelServerIdx = 0;

    agents.forEach(function (agent) {
      if (!agent.detected) return;
      let left, top;
      const isModelServer = !!agent.is_model_server;

      if (isModelServer) {
        // Dedicated Inner Orbit for Model Servers (radius 18%)
        const mAngle = (modelServerIdx * 120 + 45) * (Math.PI / 180);
        left = 50 + Math.cos(mAngle) * 18;
        top = 50 + Math.sin(mAngle) * 18;
        modelServerIdx++;
      } else {
        const pos = positions[agentIdx % positions.length];
        const rad = (pos.angle * Math.PI) / 180;
        left = 50 + Math.cos(rad) * pos.radius;
        top = 50 + Math.sin(rad) * pos.radius;
        agentIdx++;
      }

      const blip = node("div", "ar-blip" + (isModelServer ? " ar-blip-model-server" : ""));
      blip.dataset.state = agent.state || "idle";
      blip.style.left = left + "%";
      blip.style.top = top + "%";
      blip.style.cursor = "pointer";
      if (left > 58) blip.dataset.side = "left";

      const isUnverified = !isModelServer && !(agent.processes || []).some(function (p) {
        return (p.identity || "").indexOf("Verified") !== -1 && (p.identity || "").indexOf("Unverified") === -1;
      });
      const labelText = agent.label + (isModelServer ? " [Model Server]" : (isUnverified ? " [Unverified]" : " [Verified]"));
      const label = node("span", "ar-blip-label", labelText);
      blip.appendChild(label);

      blip.title =
        agent.label +
        " · " +
        (agent.process_count || (agent.processes && agent.processes.length) || 1) +
        " process(es) · " +
        (agent.state_label || "present") +
        " — Click to inspect process intelligence";

      blip.addEventListener("click", function () {
        openAgentInspector(agent, (agent.processes && agent.processes[0]) || null);
      });

      els.blips.appendChild(blip);
    });

    clear(els.agentList);
    let anyCard = false;
    agents.forEach(function (agent) {
      if (!agent.detected) return;
      anyCard = true;
      const li = node("li", "ar-agent-card");
      const top = node("div", "ar-agent-top");
      top.appendChild(node("strong", "ar-agent-name", agent.label));
      if (agent.is_model_server) {
        const catBadge = node("span", "chip chip-blue", "Model Server");
        catBadge.title = "Local inference server, not an autonomous agent.";
        top.appendChild(catBadge);
      } else if (agent.state === "unverified_process") {
        const catBadge = node("span", "chip chip-amber", "Unverified Process");
        catBadge.title = "Executable matches image name but tool identity is unconfirmed (unsigned or unverified binary).";
        top.appendChild(catBadge);
      } else if (agent.state === "suspect_mismatch") {
        const catBadge = node("span", "chip chip-red", "Suspect Binary");
        catBadge.title = "Process image name matches but metadata/signer contradicts the genuine tool.";
        top.appendChild(catBadge);
      } else if (agent.state === "access_denied") {
        const catBadge = node("span", "chip chip-red", "Access Denied");
        catBadge.title = "Process query access denied. Cannot verify identity.";
        top.appendChild(catBadge);
      } else {
        const catBadge = node("span", "chip chip-purple", "AI Agent");
        top.appendChild(catBadge);
      }
      const badge = node("span", "ar-agent-state", agent.state_label || "Present");
      badge.dataset.state = agent.state || "idle";
      top.appendChild(badge);
      li.appendChild(top);

      const detail = node(
        "div",
        "ar-agent-detail",
        (agent.process_count || 0) +
          " process" +
          ((agent.process_count || 0) === 1 ? "" : "es") +
          " · " +
          (agent.state_note || ""),
      );
      li.appendChild(detail);

      (agent.processes || []).forEach(function (proc) {
        const line = node(
          "span",
          "ar-agent-proc",
          "PID " +
            proc.pid +
            " · " +
            (proc.name || "process") +
            (proc.started_unix ? " · started " + clock(proc.started_unix) : ""),
        );
        // sanitizeDisplayPath hoisted to module level for consistent redaction
        const path = node(
          "span",
          "ar-agent-path",
          sanitizeDisplayPath(proc.exe_path),
        );
        path.title = sanitizeDisplayPath(proc.exe_path);
        li.appendChild(line);
        li.appendChild(path);
        li.appendChild(
          (function () {
            const identSpan = node("span", "ar-agent-note", proc.identity_label || "");
            const identLower = String(proc.identity || "").toLowerCase();
            if (identLower.indexOf("suspect") !== -1) {
              identSpan.className = "ar-agent-note ar-agent-note-danger";
            } else if (identLower.indexOf("unverified") !== -1) {
              identSpan.className = "ar-agent-note ar-agent-note-warn";
            }
            return identSpan;
          })(),
        );
      });

      li.appendChild(
        node("span", "ar-agent-note", agent.integration_note || ""),
      );

      const inspBtn = node("button", "btn btn-ghost ar-agent-btn-inspect", "Inspect Details");
      inspBtn.type = "button";
      inspBtn.addEventListener("click", function (ev) {
        ev.stopPropagation();
        openAgentInspector(agent, (agent.processes && agent.processes[0]) || null);
      });
      li.appendChild(inspBtn);
      li.addEventListener("click", function () {
        openAgentInspector(agent, (agent.processes && agent.processes[0]) || null);
      });
      els.agentList.appendChild(li);
    });

    els.agentList.classList.toggle("hidden", !anyCard);
    els.agentsEmpty.classList.toggle("hidden", anyCard);

    if (payload.windows) {
      state.windows = payload.windows;
      renderWindows(payload.windows);
    }

    if (payload.monitoring && (!state.session || payload.monitoring.active)) {
      applySession(payload.monitoring);
    }
  }

  async function refreshRadar(quiet) {
    if (!invoke) return;
    if (!quiet && els.refresh) setBusy(els.refresh, true, "Sampling…");
    if (!quiet && window.Luma) window.Luma.scanning("Scanning running processes for AI agents…");
    try {
      const payload = await invoke("agent_radar");
      renderRadar(payload);
      showError("");
      const det = (payload.detected_count || 0) + (payload.model_server_count || 0);
      if (!quiet && window.Luma) {
        if (det > 0) window.Luma.excited(det + " agent(s) discovered");
        else window.Luma.idle("No active AI agents detected");
      }
    } catch (err) {
      if (!quiet) {
        showError(cleanError(err, "The agent sample could not be read."));
        if (window.Luma) window.Luma.warning("Process query access limited");
      }
    } finally {
      if (!quiet && els.refresh) setBusy(els.refresh, false);
    }
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
  }

  function renderSession() {
    const session = state.session;
    const active = !!(session && session.active);
    const hasSession = !!session;

    els.stop.classList.toggle("hidden", !active);
    els.start.classList.toggle("hidden", active);
    els.start.disabled = active || state.busy;
    els.workspace.disabled = active;
    els.sessionBadge.dataset.state = active ? "recording" : hasSession ? "stopped" : "idle";
    els.sessionBadge.textContent = active ? "Recording" : hasSession ? "Stopped" : "Idle";
    els.sessionSub.textContent = active
      ? "Recording metadata changes in the selected workspace."
      : hasSession
        ? "Session stopped. Review the evidence, then build a case file."
        : "Nothing is watched until you start a session.";

    els.flightEmpty.classList.toggle("hidden", hasSession);
    els.flight.classList.toggle("hidden", !hasSession);
    els.casePreview.disabled = !hasSession || active;
    els.caseExport.disabled =
      active ||
      !state.caseDataUrl ||
      !state.caseSignature ||
      state.caseSignature !== session.id + ":" + session.events.length;

    if (!hasSession) {
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
      const path = node("span", "ar-event-path", event.path || "(renamed)");
      path.title = event.path || "";
      btn.appendChild(path);
      const kind = node("span", "ar-event-kind", event.kind);
      kind.dataset.kind = event.kind;
      btn.appendChild(kind);
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
          ? "Save a file, run a build, or let the agent edit the workspace. New metadata changes appear here as they are observed."
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
      if (window.Luma) window.Luma.curious("Windows baseline snapshot captured");
      refreshRadar(true);
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
      if (window.Luma) {
        const diffs = (changes.added || []).length + (changes.removed || []).length + (changes.changed || []).length;
        if (diffs > 0) window.Luma.warning(diffs + " persistence difference(s) detected");
        else window.Luma.success("No unexpected persistence changes");
      }
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
      if (window.Luma) window.Luma.success("Evidence card exported");
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
    showError("");
    setBusy(els.start, true, "Capturing baseline…");
    try {
      const session = await invoke("flight_start", { workspace: workspace });
      state.session = session;
      state.scrub = null;
      state.selectedSeq = null;
      resetCasePreview("");
      renderSession();
      if (window.Luma) window.Luma.curious("Flight recorder monitoring workspace metadata");
      els.caseStatus.textContent =
        "Recording. Stop the session to review final reconciliation and unlock the case file.";
      refreshRadar(true);
    } catch (err) {
      showError(cleanError(err, "The monitoring session could not start."));
    } finally {
      state.busy = false;
      setBusy(els.start, false);
      renderSession();
    }
  }

  async function stopSession() {
    if (state.busy) return;
    state.busy = true;
    setBusy(els.stop, true, "Reconciling…");
    try {
      const session = await invoke("flight_stop");
      state.session = session;
      state.scrub = null;
      state.selectedSeq = null;
      renderSession();
      if (window.Luma) window.Luma.success("Recorded session complete and reconciled");
      els.caseStatus.textContent = session.coverage && session.coverage.complete
        ? "Recording stopped. Final reconciliation finished within recorder limits. Preview the case file."
        : "Recording stopped. Coverage is incomplete; read the coverage note before interpreting the result.";
      refreshRadar(true);
    } catch (err) {
      showError(cleanError(err, "The monitoring session could not be stopped cleanly."));
    } finally {
      state.busy = false;
      setBusy(els.stop, false);
      renderSession();
    }
  }

  async function refreshStatus(quiet) {
    if (!invoke) return;
    try {
      const session = await invoke("flight_status");
      if (session) applySession(session);
      if (!state.session) renderSession();
    } catch (err) {
      if (!quiet) showError(cleanError(err, "Session state is unavailable."));
    }
  }

  /* ---------------------------------------------------------------------
     Wiring
     --------------------------------------------------------------------- */

  els.refresh.addEventListener("click", function () {
    refreshRadar(false);
  });
  els.start.addEventListener("click", startSession);
  els.stop.addEventListener("click", stopSession);
  els.jump.addEventListener("click", function () {
    els.workspace.focus();
    els.workspace.scrollIntoView({ block: "center", behavior: "smooth" });
  });
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
      if (window.Luma) window.Luma.success("Case exported as " + format.toUpperCase());
    }).catch(function (e) {
      showError(cleanError(e, "Export failed."));
    });
  };

  const btnScanNow = $("ar-scan-now");
  if (btnScanNow) {
    btnScanNow.addEventListener("click", function () {
      refreshRadar(false);
    });
  }

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

  if (window.__CURE_FORCE_MOCK) {
    els.demo.classList.remove("hidden");
  }

  if (!invoke) {
    els.radarNote.textContent = "The Agent Radar is available in the desktop build.";
    els.sessionSub.textContent = "This build has no local engine.";
    els.start.disabled = true;
    return;
  }

  let tickTimer = 0;
  function tick() {
    if (!tickTimer) return;
    if (view.classList.contains("hidden")) return;
    refreshRadar(true);
    refreshStatus(true);
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
      refreshRadar(false);
      refreshStatus(false);
    },
    state: state,
  };

  renderSession();
  refreshRadar(true);
  refreshStatus(true);
  tickTimer = window.setInterval(tick, 3000);
})();
