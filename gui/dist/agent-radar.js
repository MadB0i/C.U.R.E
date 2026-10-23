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
    codex_cli: "Codex CLI",
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

    els.detected.textContent = String(detected);
    els.radar.dataset.signal = detected > 0 ? "detected" : "none";

    if (detected === 0) {
      els.radarNote.textContent =
        "No supported agent process detected in this sample. A process may start and stop between samples.";
    } else {
      els.radarNote.textContent =
        detected +
        " supported agent" +
        (detected === 1 ? "" : "s") +
        " present in " +
        processCount +
        " process" +
        (processCount === 1 ? "" : "es") +
        ". Presence is not proof of activity.";
    }

    clear(els.blips);
    const positions = [
      { angle: 212, radius: 30 },
      { angle: 330, radius: 38 },
    ];
    agents.filter(function (agent) {
      return agent.detected;
    }).forEach(function (agent, index) {
      const pos = positions[index % positions.length];
      const rad = (pos.angle * Math.PI) / 180;
      const left = 50 + Math.cos(rad) * pos.radius;
      const blip = node("div", "ar-blip");
      blip.dataset.state = agent.state || "idle";
      blip.style.left = left + "%";
      blip.style.top = 50 + Math.sin(rad) * pos.radius + "%";
      if (left > 58) blip.dataset.side = "left";
      const label = node("span", "ar-blip-label", agent.label);
      blip.appendChild(label);
      blip.title =
        agent.label +
        " · " +
        (agent.process_count || agent.processes.length) +
        " process(es) · " +
        (agent.state_label || "present");
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
        const path = node(
          "span",
          "ar-agent-path",
          proc.exe_path || "path unavailable",
        );
        path.title = proc.exe_path || "path unavailable";
        li.appendChild(line);
        li.appendChild(path);
        li.appendChild(
          node("span", "ar-agent-note", proc.identity_label || ""),
        );
      });

      li.appendChild(
        node("span", "ar-agent-note", agent.integration_note || ""),
      );
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
    try {
      const payload = await invoke("agent_radar");
      renderRadar(payload);
      showError("");
    } catch (err) {
      if (!quiet) showError(cleanError(err, "The agent sample could not be read."));
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
          "Preview only. This image contains no paths, file names, prompts, credentials or machine identifiers.",
        ),
      );

      els.caseExport.disabled = false;
      const withheld = caseFile.summary_withheld
        ? " The operator note was withheld because it looked like it contained private data."
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
    showError("");
    setBusy(els.start, true, "Capturing baseline…");
    try {
      const session = await invoke("flight_start", { workspace: workspace });
      state.session = session;
      state.scrub = null;
      state.selectedSeq = null;
      resetCasePreview("");
      renderSession();
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
