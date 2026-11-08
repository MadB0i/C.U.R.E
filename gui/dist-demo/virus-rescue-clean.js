/**
 * Virus Rescue Controller (Tab 1: #scan-view)
 * Clean, modern cyber-security UI with reactive Cyber-Cat animations,
 * live dynamic file counter, heuristic detection, and 1-click permanent delete.
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
    cacheEls();
    setCatState("scanning");

    if (els.startBtn) els.startBtn.disabled = true;
    if (els.startBtnText) els.startBtnText.textContent = "Scanning System...";
    if (els.progressBar) els.progressBar.style.width = "5%";
    if (els.phaseTicker) els.phaseTicker.textContent = "Initializing heuristic engines & persistence inspection...";
    if (els.idleCard) els.idleCard.classList.add("hidden");
    if (els.cleanCard) els.cleanCard.classList.add("hidden");
    if (els.threatsContainer) els.threatsContainer.classList.add("hidden");

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
  }

  function onProgress(payload) {
    if (!payload) return;
    cacheEls();

    // 1. Dynamic live counter increment from second 0.1s
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

    const stageWidths = {
      registry: "15%",
      startup: "28%",
      tasks: "42%",
      services: "55%",
      "process-scan": "70%",
      "ransom-detect": "82%",
      "heuristic-sweep": "92%",
      done: "100%",
    };

    if (payload.stage && stageWidths[payload.stage] && els.progressBar) {
      els.progressBar.style.width = stageWidths[payload.stage];
    }
  }

  function onScanDone(summary) {
    isScanning = false;
    clearInterval(elapsedTimer);
    cacheEls();

    if (els.startBtn) els.startBtn.disabled = false;
    if (els.startBtnText) els.startBtnText.textContent = "Scan Again";
    if (els.progressBar) els.progressBar.style.width = "100%";

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
      currentThreats.push({
        id: entry.id || ("threat-" + Math.random().toString(36).substring(2)),
        name: entry.name || "Unknown Threat",
        path: entry.command || entry.location || "",
        reason: ev,
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
        score: p.score || 85,
        risk: p.risk || "HighRisk",
        pid: p.pid,
      });
    }

    updateThreatCount(currentThreats.length);

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
    if (els.phaseTicker) els.phaseTicker.textContent = `${currentThreats.length} threat(s) detected. Review and eliminate below.`;
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

      row.innerHTML = `
        <div class="vr-threat-info">
          <div class="vr-threat-topline">
            <span class="vr-pill ${pillClass}">${pillLabel}</span>
            <span class="vr-threat-name">${escapeHtml(t.name)}</span>
            ${t.pid ? `<span class="vr-threat-pid">PID ${t.pid}</span>` : ""}
          </div>
          <div class="vr-threat-path">${escapeHtml(t.path)}</div>
          <div class="vr-threat-reason">
            <strong>Why C.U.R.E Caught It:</strong> ${escapeHtml(t.reason)}
          </div>
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
            delBtn.textContent = "Delete Permanently";
            alert("Could not delete threat: " + err);
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
