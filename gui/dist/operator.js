/* ==========================================================================
   C.U.R.E OPERATOR — On-Device Security & Recovery Operator
   Replaces decorative mascots with an actionable, evidence-first operator.
   ========================================================================== */
(function () {
  "use strict";

  const $ = (id) => document.getElementById(id);

  function invoke(cmd, args) {
    if (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke) {
      return window.__TAURI__.core.invoke(cmd, args);
    }
    if (window.__TAURI_INVOKE__) {
      return window.__TAURI_INVOKE__(cmd, args);
    }
    return Promise.reject(new Error(`Tauri invoke not available for ${cmd}`));
  }

  const state = {
    posture: null,
    sessions: [],
    auditLogs: [],
    currentView: "overview",
  };

  /* ---------- Initialization & Mounting ---------- */

  async function loadOperatorStatus() {
    try {
      const posture = await invoke("operator_status");
      state.posture = posture;
      renderAllOperatorMounts();
    } catch (err) {
      console.warn("Could not fetch operator status:", err);
    }
  }

  function renderAllOperatorMounts() {
    const mounts = document.querySelectorAll(".companion, .operator-mount");
    for (const mount of mounts) {
      renderOperatorNode(mount);
    }
  }

  function renderOperatorNode(container) {
    if (!container) return;
    const viewKind = container.getAttribute("data-companion") || "general";
    const posture = state.posture || {
      highest_threat_level: "level_0_observe",
      c_drive_congested: false,
      c_drive_pressure_pct: 0,
      active_agent_count: 0,
      unverified_agent_count: 0,
      recommendations: [],
    };

    let levelClass = "level-0-observe";
    let levelLabel = "LEVEL 0: OBSERVE";
    if (posture.highest_threat_level === "level_3_emergency_containment") {
      levelClass = "level-3-containment";
      levelLabel = "LEVEL 3: CONTAINMENT";
    } else if (posture.highest_threat_level === "level_2_action_recommended") {
      levelClass = "level-2-action";
      levelLabel = "LEVEL 2: ACTION RECOMMENDED";
    } else if (posture.highest_threat_level === "level_1_review") {
      levelClass = "level-1-review";
      levelLabel = "LEVEL 1: REVIEW";
    }

    // Pick top recommendation matching the view or highest priority
    const rec = posture.recommendations && posture.recommendations[0] ? posture.recommendations[0] : {
      title: "System Telemetry Stable",
      description: "Host persistence and storage telemetry are within normal operating thresholds.",
      action_label: "Inspect Posture",
      action_type: "inspect",
      risk_assessment: "Normal monitoring.",
    };

    container.innerHTML = `
      <div class="operator-node">
        <div class="operator-node-head">
          <div class="operator-id">
            <span class="operator-pulse-dot"></span>
            <span>C.U.R.E OPERATOR</span>
          </div>
          <span class="operator-level-badge ${levelClass}">${levelLabel}</span>
        </div>
        <div class="operator-recommendation-card">
          <div class="operator-rec-title">
            <svg viewBox="0 0 24 24" width="14" height="14" fill="#00f5ff">
              <path d="M12 2L1 21h22L12 2zm0 3.5L20.3 19H3.7L12 5.5zM11 10v4h2v-4h-2zm0 6v2h2v-2h-2z"/>
            </svg>
            ${rec.title}
          </div>
          <div class="operator-rec-desc">${rec.description}</div>
          <div class="operator-rec-actions">
            <button class="operator-action-btn" data-action="${rec.action_type || 'inspect'}">
              ${rec.action_label || 'View Action'}
            </button>
            <span style="font-size: 0.68rem; color: #64748b;">${rec.risk_assessment}</span>
          </div>
        </div>
      </div>
    `;

    const btn = container.querySelector(".operator-action-btn");
    if (btn) {
      btn.addEventListener("click", () => {
        handleOperatorAction(rec);
      });
    }
  }

  function handleOperatorAction(rec) {
    if (!rec) return;
    if (rec.action_type === "storage_rescue") {
      // Navigate to storage workstation rescue tab
      const navStorage = $("nav-cleanup");
      if (navStorage) navStorage.click();
      setTimeout(() => {
        const rescueTab = $("sw-tab-rescue");
        if (rescueTab) rescueTab.click();
      }, 150);
    } else if (rec.action_type === "agent_session") {
      // Navigate to Agents radar view
      const navAgents = $("nav-agents");
      if (navAgents) navAgents.click();
    } else {
      // Default: show overview or scan
      const navScan = $("nav-scan");
      if (navScan) navScan.click();
    }
  }

  /* ---------- Backward-Compatibility Bridge for Older Scripts ---------- */
  window.operator = {
    init: function () {
      loadOperatorStatus();
    },
    refresh: function () {
      loadOperatorStatus();
    },
    state,
  };

  // Graceful no-op and telemetry bridge for any residual luma calls
  window.luma = {
    applyState: function (stateName) {
      // Reflect state transition in operator status bar if applicable
      const dots = document.querySelectorAll(".operator-pulse-dot");
      for (const d of dots) {
        if (stateName === "scanning") d.style.background = "#a855f7";
        else if (stateName === "warning") d.style.background = "#f43f5e";
        else d.style.background = "#00f5ff";
      }
    },
    uplink: function (mode, title, detail) {
      // Bridge uplink notifications to console logs
      const badges = document.querySelectorAll(".operator-level-badge");
      for (const b of badges) {
        if (title) b.title = `${title}: ${detail || ''}`;
      }
    },
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", () => window.operator.init());
  } else {
    window.operator.init();
  }
})();
