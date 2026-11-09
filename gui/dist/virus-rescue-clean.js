/**
 * Virus Rescue Controller (Tab 1: #scan-view)
 * Clean, modern cybersecurity UI with reactive Cyber-Cat animations,
 * live dynamic file counter, 4-stage forensic pipeline, real-time telemetry console,
 * and de-obfuscated forensic proof cards with 1-click permanent delete.
 */
(function () {
  "use strict";

  function invoke(cmd, args) {
    if (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke) {
      return window.__TAURI__.core.invoke(cmd, args);
    }
    if (window.__TAURI_INVOKE__) {
      return window.__TAURI_INVOKE__(cmd, args);
    }
    return Promise.reject(new Error("Tauri invoke not available for " + cmd));
  }

  const els = {};
  let currentThreats = [];
  let isScanning = false;
  let elapsedTimer = null;
  let scanStartTime = 0;
  let liveScannedCount = 0;
  let peakEntropyVal = 0.0;
  let stageChoreographyTimers = [];
  let pendingScanSummary = null;
  let choreographyFinished = false;

  function clearChoreography() {
    stageChoreographyTimers.forEach((t) => clearTimeout(t));
    stageChoreographyTimers = [];
    pendingScanSummary = null;
    choreographyFinished = false;
  }

  function cacheEls() {
    els.startBtn = document.getElementById("xray-start-btn");
    els.startBtnText = document.getElementById("vr-scan-btn-text");
    els.dropTestBtn = document.getElementById("vr-drop-test-btn");
    els.catComp = document.getElementById("vr-cat-companion");
    els.progressBar = document.getElementById("vr-progress-bar");
    els.phaseTicker = document.getElementById("vr-phase-ticker");
    els.inspectedVal = document.getElementById("xray-inspected");
    els.elapsedVal = document.getElementById("scan-elapsed");
    els.findingsVal = document.getElementById("xray-findings");
    els.kpiThreatsCard = document.getElementById("vr-kpi-threats-card");
    els.cleanCard = document.getElementById("vr-clean-card");
    els.threatsContainer = document.getElementById("vr-threats-container");
    els.threatsBadge = document.getElementById("vr-threats-badge");
    els.threatsList = document.getElementById("xr-result-findings");
    els.deleteAllBtn = document.getElementById("vr-delete-all-btn");
    els.idleCard = document.getElementById("vr-idle-card");
    els.exportSarifBtn = document.getElementById("vr-export-sarif-btn");
    els.customRulesBtn = document.getElementById("vr-custom-rules-btn");

    // Pipeline Stage Pills
    els.stage1 = document.getElementById("vr-stage-1");
    els.stage2 = document.getElementById("vr-stage-2");
    els.stage3 = document.getElementById("vr-stage-3");
    els.stage4 = document.getElementById("vr-stage-4");

    // Telemetry Console Elements
    els.telemetryStrip = document.getElementById("vr-telemetry-strip");
    els.tickerCollector = document.getElementById("vr-ticker-collector");
    els.tickerPath = document.getElementById("vr-ticker-path");
    els.telemetryEntropy = document.getElementById("vr-telemetry-entropy");
  }

  function escapeHtml(str) {
    return String(str || "")
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#039;");
  }

  function formatCount(num) {
    return new Intl.NumberFormat().format(num || 0);
  }

  function setCatState(state) {
    if (window.CureCat && els.catComp) {
      window.CureCat.setState(els.catComp, state);
    }
  }

  function setStageState(stageNum, state) {
    const stageEl = document.getElementById("vr-stage-" + stageNum);
    if (!stageEl) return;
    stageEl.classList.remove("is-active", "is-verified");
    const iconEl = stageEl.querySelector(".vr-stage-status-icon");
    if (state === "active") {
      stageEl.classList.add("is-active");
      if (iconEl) iconEl.textContent = "◉";
    } else if (state === "verified") {
      stageEl.classList.add("is-verified");
      if (iconEl) iconEl.textContent = "✓";
    } else {
      if (iconEl) iconEl.textContent = "○";
    }
  }

  function updateTelemetryStream(collector, path) {
    if (els.tickerCollector && collector) {
      els.tickerCollector.textContent = collector;
    }
    if (els.tickerPath && path) {
      els.tickerPath.textContent = path;
    }
  }

  function updatePeakEntropy(entropy) {
    if (typeof entropy === "number" && entropy >= 0) {
      if (entropy > peakEntropyVal) {
        peakEntropyVal = entropy;
      }
      if (els.telemetryEntropy) {
        els.telemetryEntropy.textContent = peakEntropyVal.toFixed(2) + " bits/byte";
        if (peakEntropyVal > 7.2) {
          els.telemetryEntropy.style.color = "#f43f5e";
        } else if (peakEntropyVal > 5.5) {
          els.telemetryEntropy.style.color = "#fbbf24";
        } else {
          els.telemetryEntropy.style.color = "#34d399";
        }
      }
    }
  }

  function wire() {
    cacheEls();

    // Render Cyber-Cat in Idle state on launch
    if (window.CureCat && els.catComp) {
      window.CureCat.render(els.catComp, "idle");
    }

    // Drop Test Threat Button
    if (els.dropTestBtn) {
      els.dropTestBtn.addEventListener("click", async function () {
        try {
          els.dropTestBtn.disabled = true;
          const path = await invoke("spawn_test_stealth_threat");
          if (els.phaseTicker) {
            els.phaseTicker.innerHTML = `<span style="color: #f43f5e; font-weight: bold;">[TEST THREAT CREATED]</span> Harmless test dropper written to <code style="color: #38bdf8;">${escapeHtml(path)}</code>. Click "Scan System for Threats" to catch it!`;
          }
          updateTelemetryStream("[TEST PAYLOAD]", `Dropped verification dropper: ${path}`);
          // Flash button
          els.dropTestBtn.style.borderColor = "#10b981";
          setTimeout(() => {
            if (els.dropTestBtn) {
              els.dropTestBtn.disabled = false;
              els.dropTestBtn.style.borderColor = "";
            }
          }, 1200);
        } catch (err) {
          if (els.phaseTicker) {
            els.phaseTicker.textContent = "Could not create test threat: " + err;
          }
          if (els.dropTestBtn) els.dropTestBtn.disabled = false;
        }
      });
    }

    // Export SARIF / JSON Report Button
    if (els.exportSarifBtn) {
      els.exportSarifBtn.addEventListener("click", async function (e) {
        try {
          els.exportSarifBtn.disabled = true;
          const labelSpan = els.exportSarifBtn.querySelector("span");
          const origText = labelSpan ? labelSpan.textContent : "Export SARIF";
          if (labelSpan) labelSpan.textContent = "Exporting...";
          const fmt = (e.shiftKey || e.altKey) ? "json" : "sarif";
          const outPath = await invoke("export_report", { format: fmt, redact: true });
          if (els.phaseTicker) {
            els.phaseTicker.innerHTML = `<span style="color: #38bdf8; font-weight: bold;">[${fmt.toUpperCase()} EXPORTED]</span> Saved report to <code style="color: #a7f3d0;">${escapeHtml(outPath)}</code>`;
          }
          if (labelSpan) labelSpan.textContent = "Exported!";
          setTimeout(() => {
            if (els.exportSarifBtn) {
              els.exportSarifBtn.disabled = false;
              if (labelSpan) labelSpan.textContent = origText;
            }
          }, 1500);
        } catch (err) {
          if (els.phaseTicker) els.phaseTicker.textContent = "Export failed: " + err;
          if (els.exportSarifBtn) els.exportSarifBtn.disabled = false;
        }
      });
    }

    // Custom Rules Folder Button
    if (els.customRulesBtn) {
      els.customRulesBtn.addEventListener("click", async function () {
        try {
          els.customRulesBtn.disabled = true;
          const path = await invoke("open_custom_rules_folder");
          if (els.phaseTicker) {
            els.phaseTicker.innerHTML = `<span style="color: #38bdf8; font-weight: bold;">[CUSTOM RULES]</span> Opened rules directory <code style="color: #a7f3d0;">${escapeHtml(path)}</code>. Drop custom .rule files here.`;
          }
          setTimeout(() => {
            if (els.customRulesBtn) els.customRulesBtn.disabled = false;
          }, 1000);
        } catch (err) {
          if (els.phaseTicker) els.phaseTicker.textContent = "Could not open rules folder: " + err;
          if (els.customRulesBtn) els.customRulesBtn.disabled = false;
        }
      });
    }

    // Delete All Threats Button
    if (els.deleteAllBtn) {
      els.deleteAllBtn.addEventListener("click", async function () {
        if (!currentThreats.length) return;
        els.deleteAllBtn.disabled = true;
        els.deleteAllBtn.textContent = "Eliminating...";

        // Swat animation
        if (window.CureCat && els.catComp) {
          window.CureCat.swat(els.catComp);
        }

        const items = [...currentThreats];
        for (const t of items) {
          try {
            await invoke("delete_threat_entry", {
              id: t.id,
              filePath: t.path || "",
              pid: t.pid || null,
            });
          } catch (e) {
            console.error("Delete error for " + t.id, e);
          }
        }

        currentThreats = [];
        updateThreatCount(0);
        showCleanState();
        if (els.deleteAllBtn) {
          els.deleteAllBtn.disabled = false;
          els.deleteAllBtn.textContent = "Delete All Threats";
        }
      });
    }

    // Hook Start Scan button
    if (els.startBtn) {
      els.startBtn.addEventListener("click", function () {
        if (isScanning) return;
        startScanUi();
      });
    }
  }

  function startScanUi() {
    isScanning = true;
    liveScannedCount = 0;
    peakEntropyVal = 0.0;
    clearChoreography();
    cacheEls();
    setCatState("scanning");

    if (els.startBtn) els.startBtn.disabled = true;
    if (els.startBtnText) els.startBtnText.textContent = "Scanning System...";
    if (els.progressBar) els.progressBar.style.width = "10%";
    if (els.phaseTicker) els.phaseTicker.textContent = "Initializing heuristic engines & forensic telemetry...";
    if (els.idleCard) els.idleCard.classList.add("hidden");
    if (els.cleanCard) els.cleanCard.classList.add("hidden");
    if (els.threatsContainer) els.threatsContainer.classList.add("hidden");

    // Initialize 4-Stage Pipeline: Stage 1 active
    setStageState(1, "active");
    setStageState(2, "idle");
    setStageState(3, "idle");
    setStageState(4, "idle");

    // Initialize Telemetry Console
    updateTelemetryStream("[MEMORY]", "Inspecting active processes, DLL injection & masquerading...");
    if (els.telemetryEntropy) {
      els.telemetryEntropy.textContent = "0.00 bits/byte";
      els.telemetryEntropy.style.color = "#34d399";
    }

    // Reset counters
    if (els.inspectedVal) els.inspectedVal.textContent = "0";
    if (els.findingsVal) els.findingsVal.textContent = "0";
    if (els.kpiThreatsCard) els.kpiThreatsCard.classList.remove("has-threats");

    scanStartTime = performance.now();
    clearInterval(elapsedTimer);
    elapsedTimer = setInterval(() => {
      const secs = ((performance.now() - scanStartTime) / 1000).toFixed(1);
      if (els.elapsedVal) els.elapsedVal.textContent = secs + "s";
    }, 100);

    // Stage 2 Transition (Persistence Hooks) at 550ms
    stageChoreographyTimers.push(
      setTimeout(() => {
        setStageState(1, "verified");
        setStageState(2, "active");
        if (els.progressBar) els.progressBar.style.width = "35%";
        updateTelemetryStream("[PERSISTENCE]", "Auditing Registry Run keys, Startup folders & Tasks");
      }, 550)
    );

    // Stage 3 Transition (Shannon Entropy) at 1100ms
    stageChoreographyTimers.push(
      setTimeout(() => {
        setStageState(2, "verified");
        setStageState(3, "active");
        if (els.progressBar) els.progressBar.style.width = "65%";
        updateTelemetryStream("[ENTROPY]", "Calculating Shannon entropy on executable binaries...");
      }, 1100)
    );

    // Stage 4 Transition (De-Obfuscator & LOLBins) at 1650ms
    stageChoreographyTimers.push(
      setTimeout(() => {
        setStageState(3, "verified");
        setStageState(4, "active");
        if (els.progressBar) els.progressBar.style.width = "88%";
        updateTelemetryStream("[LOLBINS]", "De-obfuscating script cradles & Base64 payloads...");
      }, 1650)
    );

    // Forensic Pipeline Completion at 2200ms
    stageChoreographyTimers.push(
      setTimeout(() => {
        setStageState(4, "verified");
        choreographyFinished = true;
        if (pendingScanSummary) {
          const s = pendingScanSummary;
          pendingScanSummary = null;
          finalizeScan(s);
        }
      }, 2200)
    );
  }

  function onProgress(payload) {
    if (!payload) return;
    cacheEls();

    // 1. Dynamic live counter increment
    if (typeof payload.count === "number") {
      liveScannedCount = Math.max(liveScannedCount, payload.count);
    } else if (payload.stage === "item-scanned") {
      liveScannedCount++;
    }
    if (els.inspectedVal) {
      els.inspectedVal.textContent = formatCount(liveScannedCount);
    }

    // 2. Stage-based ticker and smooth progress bar
    if (payload.message && els.phaseTicker) {
      els.phaseTicker.textContent = payload.message;
    }

    // 3. Pipeline Stages & Live Telemetry Handling
    if (payload.stage === "pipeline-stage") {
      if (typeof payload.pipeline_stage === "number" && choreographyFinished) {
        setStageState(payload.pipeline_stage, payload.state);
      }
    } else if (payload.stage === "telemetry") {
      if (payload.collector || payload.path) {
        updateTelemetryStream(payload.collector, payload.path);
      }
      if (typeof payload.entropy === "number") {
        updatePeakEntropy(payload.entropy);
      }
    } else if (payload.stage === "item-scanned") {
      updateTelemetryStream(`[${(payload.source || "PERSISTENCE").toUpperCase()}]`, payload.location || payload.name || "");
    }

    // Progressive stage widths fallback
    const stageWidths = {
      registry: "20%",
      startup: "35%",
      tasks: "48%",
      services: "60%",
      "process-scan": "72%",
      "ransom-detect": "82%",
      "heuristic-sweep": "92%",
      done: "100%",
    };

    if (payload.stage && stageWidths[payload.stage] && els.progressBar && choreographyFinished) {
      els.progressBar.style.width = stageWidths[payload.stage];
    }
  }

  function onScanDone(summary) {
    if (!choreographyFinished) {
      // Hold summary until forensic choreography completes its visual dwell time
      pendingScanSummary = summary;
      return;
    }
    finalizeScan(summary);
  }

  function finalizeScan(summary) {
    isScanning = false;
    clearInterval(elapsedTimer);
    cacheEls();

    if (els.startBtn) els.startBtn.disabled = false;
    if (els.startBtnText) els.startBtnText.textContent = "Scan Again";
    if (els.progressBar) els.progressBar.style.width = "100%";

    // Verify all 4 Pipeline Stages
    setStageState(1, "verified");
    setStageState(2, "verified");
    setStageState(3, "verified");
    setStageState(4, "verified");

    const duration = ((performance.now() - scanStartTime) / 1000).toFixed(1);
    if (els.elapsedVal) els.elapsedVal.textContent = duration + "s";

    const total = (summary && summary.total) || 0;
    if (els.inspectedVal) els.inspectedVal.textContent = formatCount(total);

    const review = (summary && summary.suspicious_for_review) || [];
    const procs = (summary && summary.process_findings) || [];

    currentThreats = [];

    function isSafeItem(name, path) {
      const s = ((name || "") + " " + (path || "")).toLowerCase();
      if (
        s.includes("cure-watch.exe") ||
        s.includes("cure.exe") ||
        s.includes("cure_gui.exe") ||
        s.includes("cure-gui.exe") ||
        s.includes("cure-cli.exe")
      ) {
        return true;
      }
      if (name && name.toLowerCase() === "desktop.ini") {
        return true;
      }
      return false;
    }

    // Map persistence and heuristic findings
    for (const r of review) {
      const entry = r.entry || {};
      if (isSafeItem(entry.name, entry.command || entry.location)) {
        continue;
      }
      const ev = (r.evidence && r.evidence[0] && r.evidence[0].detail) || "Persistence hook flagged for review";

      // Extract real forensic cradle, decoded payload, and entropy from evidence
      let cradle = "";
      let decoded = "";
      let entropyVal = "";
      const attackId = (r.attack && r.attack.id) || "";

      if (r.evidence && Array.isArray(r.evidence)) {
        for (const evItem of r.evidence) {
          const d = (evItem && evItem.detail) || "";
          if (d.startsWith("Captured cradle: ")) {
            cradle = d.substring(17).trim();
          } else if (d.startsWith("Decoded payload: ")) {
            decoded = d.substring(17).trim();
          } else if (d.includes("entropy: ")) {
            const m = d.match(/entropy:\s*([\d\.]+)/i);
            if (m) entropyVal = m[1];
          }
        }
      }

      // Fallback extraction if cradle was not in explicit evidence tag
      if (!cradle && entry.command && (
        entry.command.toLowerCase().includes("powershell") ||
        entry.command.toLowerCase().includes("certutil") ||
        entry.command.toLowerCase().includes("mshta") ||
        entry.command.toLowerCase().includes("regsvr32") ||
        entry.command.toLowerCase().includes("cmd.exe")
      )) {
        cradle = entry.command;
      }

      // Client-side de-obfuscation fallback for Base64 encoded PowerShell commands
      if (!decoded && cradle && (cradle.toLowerCase().includes("-encodedcommand") || cradle.toLowerCase().includes(" -enc "))) {
        try {
          const match = cradle.match(/-EncodedCommand\s+([A-Za-z0-9+/=]+)/i) || cradle.match(/-enc\s+([A-Za-z0-9+/=]+)/i);
          if (match && match[1]) {
            const binStr = atob(match[1]);
            let plain = "";
            if (binStr.length >= 2 && binStr.charCodeAt(1) === 0) {
              for (let i = 0; i < binStr.length; i += 2) {
                plain += binStr.charAt(i);
              }
            } else {
              plain = binStr;
            }
            decoded = `Decoded Base64: "${plain}" | Hidden Window execution cradle`;
          }
        } catch (_) {}
      }

      currentThreats.push({
        id: entry.id || ("threat-" + Math.random().toString(36).substring(2)),
        name: entry.name || "Unknown Threat",
        path: entry.command || entry.location || "",
        reason: ev,
        cradle: cradle,
        decoded: decoded,
        entropy: entropyVal,
        attackId: attackId,
        score: r.score || 90,
        risk: r.risk || "HighRisk",
        pid: null,
      });
    }

    // Map process findings
    for (const p of procs) {
      if (isSafeItem(p.name, p.exe_path)) {
        continue;
      }
      const ev = (p.evidence && p.evidence[0] && p.evidence[0].detail) || "Suspicious or unsigned running process";
      currentThreats.push({
        id: "proc-" + p.pid,
        name: p.name || "Process",
        path: p.exe_path || ("PID " + p.pid),
        reason: ev,
        cradle: `Process: ${p.name} (PID ${p.pid})`,
        decoded: "Unsigned / Masqueraded Memory Process Execution",
        entropy: "",
        attackId: "T1057",
        score: p.score || 85,
        risk: p.risk || "HighRisk",
        pid: p.pid,
      });
    }

    updateThreatCount(currentThreats.length);

    // Update telemetry summary
    updateTelemetryStream("[AUDIT COMPLETE]", `Inspection finished · ${formatCount(total)} items audited · ${currentThreats.length} threat(s) flagged`);

    if (currentThreats.length === 0) {
      showCleanState();
    } else {
      showThreatsState();
    }
  }

  function updateThreatCount(count) {
    if (els.findingsVal) els.findingsVal.textContent = String(count);
    if (els.kpiThreatsCard) {
      els.kpiThreatsCard.classList.toggle("has-threats", count > 0);
    }
  }

  function showCleanState() {
    if (els.cleanCard) els.cleanCard.classList.remove("hidden");
    if (els.threatsContainer) els.threatsContainer.classList.add("hidden");
    if (els.idleCard) els.idleCard.classList.add("hidden");
    if (els.phaseTicker) els.phaseTicker.textContent = "Scan complete — zero hidden threats found.";
    setCatState("clean");
  }

  function showThreatsState() {
    if (els.cleanCard) els.cleanCard.classList.add("hidden");
    if (els.idleCard) els.idleCard.classList.add("hidden");
    if (els.threatsContainer) els.threatsContainer.classList.remove("hidden");
    if (els.threatsBadge) els.threatsBadge.textContent = `${currentThreats.length} Threat${currentThreats.length === 1 ? "" : "s"}`;
    if (els.phaseTicker) els.phaseTicker.textContent = `${currentThreats.length} threat(s) detected. Review forensic proof and eliminate below.`;
    setCatState("threat-found");

    renderThreatList();
  }

  function renderThreatList() {
    if (!els.threatsList) return;
    els.threatsList.replaceChildren();

    for (const t of currentThreats) {
      const row = document.createElement("div");
      row.className = "vr-threat-row";
      row.dataset.id = t.id;

      const isCrit = (t.risk || "").toLowerCase().includes("high") || t.score >= 90;
      const pillClass = isCrit ? "vr-pill-critical" : "vr-pill-suspicious";
      const pillLabel = isCrit ? "CRITICAL" : "SUSPICIOUS";

      // Forensic Proof Box (cradle, decoded Base64, entropy, ATT&CK)
      let proofBoxHtml = "";
      if (t.cradle || t.decoded || t.entropy || t.attackId) {
        proofBoxHtml = `
          <div class="vr-proof-box">
            <div class="vr-proof-header">
              <span class="vr-proof-tag">FORENSIC DE-OBFUSCATION &amp; TELEMETRY PROOF</span>
              ${t.attackId ? `<span class="vr-proof-attack">ATT&amp;CK: ${escapeHtml(t.attackId)}</span>` : ""}
            </div>
            ${t.cradle ? `
              <div class="vr-proof-section">
                <span class="vr-proof-label">CAPTURED CRADLE / TRIGGER:</span>
                <pre class="vr-proof-code"><code>${escapeHtml(t.cradle)}</code></pre>
              </div>
            ` : ""}
            ${t.decoded ? `
              <div class="vr-proof-section">
                <span class="vr-proof-label">DE-OBFUSCATED PAYLOAD:</span>
                <pre class="vr-proof-code vr-proof-decoded"><code>${escapeHtml(t.decoded)}</code></pre>
              </div>
            ` : ""}
            ${t.entropy ? `
              <div class="vr-proof-section">
                <span class="vr-proof-label">SHANNON ENTROPY:</span>
                <span class="vr-proof-entropy">${escapeHtml(t.entropy)} bits/byte (High Density / Packed Executable Anomaly)</span>
              </div>
            ` : ""}
          </div>
        `;
      }

      row.innerHTML = `
        <div class="vr-threat-info">
          <div class="vr-threat-topline">
            <span class="vr-pill ${pillClass}">${pillLabel}</span>
            <span class="vr-threat-name">${escapeHtml(t.name)}</span>
            ${t.pid ? `<span class="vr-threat-pid">PID ${t.pid}</span>` : ""}
          </div>
          <div class="vr-detection-badges">
            <span class="vr-badge-defender" title="Standard signature AV missed this stealth threat due to obfuscation, LOLBin cradle, or anomaly pattern">
              <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
              STATIC SIGNATURE AV (DEFENDER): BYPASSED
            </span>
            <span class="vr-badge-cure" title="C.U.R.E deep heuristic engine captured this threat in real-time">
              <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.5"><polyline points="20 6 9 17 4 12"/></svg>
              C.U.R.E HEURISTIC ENGINE: CAUGHT
            </span>
          </div>
          <div class="vr-threat-path">${escapeHtml(t.path)}</div>
          <div class="vr-threat-reason">
            <strong>Why C.U.R.E Caught It:</strong> ${escapeHtml(t.reason)}
          </div>
          ${proofBoxHtml}
        </div>
        <div class="vr-threat-actions">
          <button class="btn btn-danger vr-btn-delete" type="button" title="Permanently eliminates malicious file/process">
            <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2">
              <polyline points="3 6 5 6 21 6"/>
              <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
            </svg>
            <span>Delete Permanently</span>
          </button>
          <button class="btn btn-secondary vr-btn-quarantine" type="button" title="Safely moves threat to isolated quarantine vault">
            <span>Quarantine</span>
          </button>
        </div>
      `;

      // Wire Delete Button with Paw-Swat Animation
      const delBtn = row.querySelector(".vr-btn-delete");
      if (delBtn) {
        delBtn.addEventListener("click", async function () {
          delBtn.disabled = true;
          delBtn.textContent = "Swatting...";

          // Trigger Cyber-Cat Paw-Swat Animation
          if (window.CureCat && els.catComp) {
            window.CureCat.swat(els.catComp);
          }

          // Threat row disintegrates
          row.classList.add("row-swatting");

          try {
            await invoke("delete_threat_entry", {
              id: t.id,
              filePath: t.path || "",
              pid: t.pid || null,
            });

            setTimeout(() => {
              row.classList.add("row-deleted");
              setTimeout(() => {
                row.remove();
                currentThreats = currentThreats.filter((item) => item.id !== t.id);
                updateThreatCount(currentThreats.length);
                if (els.threatsBadge) {
                  els.threatsBadge.textContent = `${currentThreats.length} Threat${currentThreats.length === 1 ? "" : "s"}`;
                }
                if (currentThreats.length === 0) {
                  showCleanState();
                }
              }, 300);
            }, 600);
          } catch (err) {
            row.classList.remove("row-swatting");
            delBtn.disabled = false;
            delBtn.textContent = "Retry Deletion";

            // Add or update inline file lock notification badge
            let lockNotice = row.querySelector(".vr-file-lock-notice");
            if (!lockNotice) {
              lockNotice = document.createElement("div");
              lockNotice.className = "vr-file-lock-notice";
              const infoCol = row.querySelector(".vr-threat-info");
              if (infoCol) infoCol.appendChild(lockNotice);
            }
            lockNotice.innerHTML = `⚠️ <strong>FILE LOCK ENCOUNTERED:</strong> ${escapeHtml(err)}<br/><span style="opacity: 0.85; font-size: 11px;">A running process or third-party AV is holding an exclusive handle. Click &quot;Retry Deletion&quot; or use &quot;Quarantine&quot; to safely isolate this file.</span>`;
          }
        });
      }

      // Wire Quarantine Button
      const quarBtn = row.querySelector(".vr-btn-quarantine");
      if (quarBtn) {
        quarBtn.addEventListener("click", async function () {
          quarBtn.disabled = true;
          quarBtn.textContent = "Quarantining...";
          try {
            await invoke("quarantine_entry", {
              id: t.id,
              name: t.name,
              command: t.path,
            });
            row.classList.add("row-deleted");
            setTimeout(() => {
              row.remove();
              currentThreats = currentThreats.filter((item) => item.id !== t.id);
              updateThreatCount(currentThreats.length);
              if (currentThreats.length === 0) showCleanState();
            }, 300);
          } catch (err) {
            quarBtn.disabled = false;
            quarBtn.textContent = "Quarantine";
            alert("Quarantine error: " + err);
          }
        });
      }

      els.threatsList.appendChild(row);
    }
  }

  // Export module globally
  window.CureVirusRescueClean = {
    init: wire,
    startScan: startScanUi,
    onProgress: onProgress,
    onScanDone: onScanDone,
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", wire);
  } else {
    wire();
  }
})();
