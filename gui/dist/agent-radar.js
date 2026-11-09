/* ==========================================================================
   C.U.R.E — AI Agent & Bot Scanner (Page 2)
   --------------------------------------------------------------------------
   Product principles:
   - Local only. Zero network telemetry.
   - No auto-scan on tab switch: scanning runs only upon explicit operator click.
   - Honest evidence: groups child processes by executable identity and
     reports Authenticode signature verification.
   - Direct control: 1-click reveal in Explorer and safe process termination.
   ========================================================================== */
(function () {
  "use strict";

  const TAU = window.__TAURI__;
  const invoke =
    TAU && TAU.core && typeof TAU.core.invoke === "function"
      ? TAU.core.invoke
      : null;

  function $(id) {
    return document.getElementById(id);
  }

  function sanitizeDisplayPath(p) {
    if (!p) return "path unavailable";
    return String(p)
      .replace(/([A-Za-z]:\\Users\\)[^\\]+/gi, "$1<user>")
      .replace(/(\/home\/)[^\/]+/gi, "$1<user>")
      .replace(/(\/Users\/)[^\/]+/gi, "$1<user>");
  }

  let els = {};
  let lastGroups = null;
  let isScanning = false;

  function cacheEls() {
    els = {
      catComp: $("ar-cat-companion"),
      scanBtn: $("ar-scan-now"),
      scanBtnText: $("ar-scan-btn-text"),
      progressBar: $("ar-progress-bar"),
      ticker: $("ar-phase-ticker"),
      kpiAgents: $("ar-kpi-agents-count"),
      kpiProcsMem: $("ar-kpi-procs-memory"),
      kpiUnverified: $("ar-kpi-unverified-count"),
      kpiTrustCard: $("ar-kpi-unverified-card"),
      idleCard: $("ar-idle-card"),
      cleanCard: $("ar-clean-card"),
      agentsCard: $("ar-agents-card"),
      groupedList: $("ar-grouped-list"),
    };
  }

  let cleanResetTimer = null;
  function setOculusState(state, customLabel, customSub) {
    if (cleanResetTimer) {
      clearTimeout(cleanResetTimer);
      cleanResetTimer = null;
    }
    const oculus = $("ar-oculus-core");
    const badgeText = $("ar-sensor-label");
    const subText = $("ar-sub-status");
    if (!oculus) return;
    oculus.classList.remove("is-scanning", "is-clean", "is-locked", "is-idle");
    if (state === "scanning") {
      oculus.classList.add("is-scanning");
      if (badgeText) badgeText.textContent = customLabel || "AI RADAR: SCANNING PROCESSES";
      if (subText) subText.textContent = customSub || "360° MEMORY RADAR";
    } else if (state === "clean") {
      oculus.classList.add("is-clean");
      if (badgeText) badgeText.textContent = customLabel || "AI RADAR: ZERO AGENTS (CLEAN)";
      if (subText) subText.textContent = customSub || "ALL SYSTEMS SECURED";
      cleanResetTimer = setTimeout(() => {
        if (!isScanning && (!lastGroups || lastGroups.length === 0)) {
          setOculusState("idle");
        }
      }, 5000);
    } else if (state === "threat-found" || state === "locked") {
      oculus.classList.add("is-locked");
      if (badgeText) badgeText.textContent = customLabel || "TARGET LOCKED: AGENT DETECTED";
      if (subText) subText.textContent = customSub || "CRIMSON ALERT · PROCESS MONITORED";
    } else {
      oculus.classList.add("is-idle");
      if (badgeText) badgeText.textContent = customLabel || "AI RADAR: STANDBY";
      if (subText) subText.textContent = customSub || "AUTHENTICODE VERIFIED";
    }
  }

  function setProgress(pct, message) {
    if (els.progressBar) {
      els.progressBar.style.width = pct + "%";
    }
    if (els.ticker && message) {
      els.ticker.textContent = message;
    }
  }

  async function fetchRadarData() {
    if (invoke) {
      return await invoke("agent_radar");
    }
    // Browser fallback / demo mode
    return new Promise((resolve) => {
      setTimeout(() => {
        resolve({
          supported_agents: [
            {
              id: "antigravity",
              label: "Antigravity",
              detected: true,
              is_model_server: false,
              category: "Agent",
              processes: [
                {
                  pid: 1968,
                  name: "Antigravity.exe",
                  exe_path: "C:\\Users\\Operator\\AppData\\Local\\Programs\\Antigravity\\Antigravity.exe",
                  memory_usage_bytes: 420 * 1024 * 1024,
                  identity: "Verified",
                },
                {
                  pid: 2716,
                  name: "Antigravity.exe",
                  exe_path: "C:\\Users\\Operator\\AppData\\Local\\Programs\\Antigravity\\Antigravity.exe",
                  memory_usage_bytes: 180 * 1024 * 1024,
                  identity: "Verified",
                },
                {
                  pid: 6308,
                  name: "Antigravity.exe",
                  exe_path: "C:\\Users\\Operator\\AppData\\Local\\Programs\\Antigravity\\Antigravity.exe",
                  memory_usage_bytes: 247 * 1024 * 1024,
                  identity: "Verified",
                },
              ],
            },
          ],
        });
      }, 700);
    });
  }

  function groupProcesses(data) {
    const groups = [];
    const supported = data && data.supported_agents ? data.supported_agents : [];

    for (const panel of supported) {
      const procs = panel.processes || [];
      if (!panel.detected && procs.length === 0) continue;
      if (procs.length === 0) continue;

      let totalBytes = 0;
      const pids = [];
      let minPid = Infinity;
      let primaryExe = "";
      let hasMismatch = false;
      let allVerified = true;

      for (const p of procs) {
        pids.push(p.pid);
        if (p.pid < minPid) minPid = p.pid;
        if (!primaryExe && p.exe_path) primaryExe = p.exe_path;
        if (p.memory_usage_bytes) totalBytes += p.memory_usage_bytes;

        const ident = (p.identity || "").toLowerCase();
        if (ident === "suspectmismatch" || ident === "suspect_mismatch") {
          hasMismatch = true;
        }
        if (ident !== "verified") {
          allVerified = false;
        }
      }

      let trust = "unverified";
      let trustLabel = "🟡 UNSIGNED / LOCAL";
      if (hasMismatch) {
        trust = "suspicious";
        trustLabel = "🔴 SUSPICIOUS BOT";
      } else if (allVerified) {
        trust = "verified";
        trustLabel = "🟢 VERIFIED SIGNED";
      }

      const totalRamMb = Math.round(totalBytes / (1024 * 1024));

      groups.push({
        id: panel.id || panel.label.toLowerCase(),
        name: panel.label,
        isModelServer: !!panel.is_model_server,
        primaryPid: minPid === Infinity ? (pids[0] || 0) : minPid,
        pids: pids,
        primaryExePath: primaryExe || (panel.label + ".exe"),
        totalRamMb: totalRamMb,
        processes: procs,
        trust: trust,
        trustLabel: trustLabel,
      });
    }

    return groups;
  }

  function renderGroupedCards(groups) {
    if (!els.groupedList) return;
    els.groupedList.innerHTML = "";

    for (const g of groups) {
      const item = document.createElement("div");
      item.className = "ar-grouped-item";
      item.dataset.agentId = g.id;

      const typeBadgeClass = g.isModelServer ? "ar-type-server" : "ar-type-agent";
      const typeBadgeText = g.isModelServer ? "MODEL SERVER" : "AI AGENT";

      let trustBadgeClass = "ar-trust-unverified";
      if (g.trust === "verified") trustBadgeClass = "ar-trust-verified";
      if (g.trust === "suspicious") trustBadgeClass = "ar-trust-suspicious";

      item.innerHTML = `
        <div class="ar-item-left">
          <div class="ar-item-nameline">
            <span class="ar-agent-name">${g.name}</span>
            <span class="ar-badge-type ${typeBadgeClass}">${typeBadgeText}</span>
            <span class="ar-badge-trust ${trustBadgeClass}">${g.trustLabel}</span>
          </div>
          <div class="ar-item-specs">
            <span class="ar-spec-pill">Primary PID: <strong>${g.primaryPid}</strong></span>
            <span class="ar-spec-pill">${g.pids.length} process${g.pids.length === 1 ? "" : "es"}</span>
            <span class="ar-spec-pill">${g.totalRamMb} MB RAM</span>
          </div>
          <div class="ar-item-path"></div>
        </div>
        <div class="ar-item-actions">
          <button class="ar-btn-reveal" type="button" title="Reveal executable in Windows Explorer">
            <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
            </svg>
            <span>Open Folder</span>
          </button>
          <button class="ar-btn-stop" type="button" title="Safely terminate all child processes for this agent">
            <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <circle cx="12" cy="12" r="10"/>
              <rect x="9" y="9" width="6" height="6"/>
            </svg>
            <span>Stop Agent</span>
          </button>
        </div>
      `;

      // Set path safely via textContent to preserve <user> and prevent HTML tag parsing
      const pathEl = item.querySelector(".ar-item-path");
      if (pathEl) {
        pathEl.textContent = sanitizeDisplayPath(g.primaryExePath);
        pathEl.title = g.primaryExePath;
      }

      // Wire Open Folder button
      const revealBtn = item.querySelector(".ar-btn-reveal");
      if (revealBtn) {
        revealBtn.addEventListener("click", async function (e) {
          e.stopPropagation();
          try {
            if (invoke) {
              await invoke("storage_reveal", { path: g.primaryExePath });
            } else {
              alert("Reveal folder: " + g.primaryExePath);
            }
          } catch (err) {
            alert("Could not open folder: " + err);
          }
        });
      }

      // Wire Stop Agent button
      const stopBtn = item.querySelector(".ar-btn-stop");
      if (stopBtn) {
        stopBtn.addEventListener("click", async function (e) {
          e.stopPropagation();
          stopBtn.disabled = true;
          stopBtn.querySelector("span").textContent = "Stopping...";

          // Disintegration animation on card
          item.classList.add("ar-item-swatting");

          // Instant visual feedback on Oculus
          setOculusState("scanning", "TERMINATING AGENT PROCESSES…", `PIDS: ${g.pids.join(", ")}`);

          try {
            if (invoke) {
              await invoke("stop_agent_pids", { pids: g.pids });
            }
            setTimeout(async function () {
              // Re-scan to refresh live state
              await scanAgents();
            }, 650);
          } catch (err) {
            item.classList.remove("ar-item-swatting");
            stopBtn.disabled = false;
            stopBtn.querySelector("span").textContent = "Stop Agent";
            alert("Could not stop agent: " + err);
            setOculusState("locked", "TERMINATION REFUSED", "PROCESS LOCKED BY OS");
          }
        });
      }

      els.groupedList.appendChild(item);
    }
  }

  function updateKpis(groups) {
    const totalAgents = groups.length;
    let totalProcs = 0;
    let totalRam = 0;
    let unverifiedCount = 0;

    for (const g of groups) {
      totalProcs += g.pids.length;
      totalRam += g.totalRamMb;
      if (g.trust !== "verified") {
        unverifiedCount++;
      }
    }

    if (els.kpiAgents) els.kpiAgents.textContent = totalAgents;
    if (els.kpiProcsMem) els.kpiProcsMem.textContent = totalProcs + " procs · " + totalRam + " MB";
    if (els.kpiUnverified) els.kpiUnverified.textContent = unverifiedCount;

    if (els.kpiTrustCard) {
      if (unverifiedCount > 0) {
        els.kpiTrustCard.classList.add("has-unverified");
      } else {
        els.kpiTrustCard.classList.remove("has-unverified");
      }
    }
  }

  async function scanAgents() {
    if (isScanning) return;
    isScanning = true;
    cacheEls();
    setOculusState("idle");

    if (els.scanBtn) {
      els.scanBtn.disabled = true;
    }
    if (els.scanBtnText) {
      els.scanBtnText.textContent = "Scanning Memory...";
    }

    setOculusState("scanning");
    setProgress(25, "Enumerating process memory & handles...");

    try {
      const data = await fetchRadarData();
      setProgress(75, "Validating Authenticode signatures & identities...");

      const groups = groupProcesses(data);
      lastGroups = groups;
      updateKpis(groups);

      // Hide idle state
      if (els.idleCard) els.idleCard.classList.add("hidden");

      if (groups.length === 0) {
        // Zero active agents
        if (els.cleanCard) els.cleanCard.classList.remove("hidden");
        if (els.agentsCard) els.agentsCard.classList.add("hidden");
        setOculusState("clean", "AI RADAR: ZERO AGENTS (CLEAN)", "ALL SYSTEMS SECURED");
        setProgress(100, "Scan complete — zero active AI agents or model servers in memory");
      } else {
        // Active agents detected
        if (els.cleanCard) els.cleanCard.classList.add("hidden");
        if (els.agentsCard) els.agentsCard.classList.remove("hidden");
        renderGroupedCards(groups);

        const unverified = groups.filter((g) => g.trust !== "verified").length;
        const totalProcs = groups.reduce((acc, g) => acc + g.pids.length, 0);

        // ALWAYS switch Oculus to RED ALERT (locked) when ANY active agent is found!
        const alertLabel = `TARGET LOCKED: ${groups.length} AGENT(S) DETECTED`;
        const alertSub = unverified > 0 ? `${unverified} UNSIGNED / THREAT RISK` : `${totalProcs} PROCESSES MONITORED`;
        setOculusState("locked", alertLabel, alertSub);

        setProgress(100, `Scan complete — ${groups.length} agent(s) detected across ${totalProcs} processes`);
      }
    } catch (err) {
      setProgress(100, "Scan failed: " + err);
      setOculusState("idle");
    } finally {
      isScanning = false;
      if (els.scanBtn) {
        els.scanBtn.disabled = false;
      }
      if (els.scanBtnText) {
        els.scanBtnText.textContent = "Scan Active Agents";
      }
    }
  }

  function wire() {
    cacheEls();



    if (els.scanBtn) {
      els.scanBtn.addEventListener("click", function () {
        scanAgents();
      });
    }
  }

  // Export module globally
  window.CureAgentRadar = {
    refresh: function () {
      cacheEls();
      // Keep honest idle state or last scanned snapshot — do NOT auto-scan
      if (lastGroups) {
        updateKpis(lastGroups);
      }
    },
    scan: scanAgents,
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", wire);
  } else {
    wire();
  }
})();
