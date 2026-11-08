/**
 * Clean Virus Rescue Controller
 * Manages Tab 1: Clean scan progress with Cyber-Cat, live counters, and clean findings / all-clear state
 */
(function () {
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

  function cacheEls() {
    els.startBtn = document.getElementById("xray-start-btn");
    els.catComp = document.getElementById("vr-cat-companion");
    els.cleanCat = document.getElementById("vr-clean-cat");
    els.progressBar = document.getElementById("vr-progress-bar");
    els.cleanCard = document.getElementById("vr-clean-card");
    els.threatsContainer = document.getElementById("vr-threats-container");
    els.threatsList = document.getElementById("xr-result-findings");
    els.threatsBadge = document.getElementById("vr-threats-badge");
    els.idleCard = document.getElementById("vr-idle-card");
    els.threatCount = document.getElementById("xray-findings");
    els.inspectedCount = document.getElementById("xray-inspected");
  }

  function wire() {
    cacheEls();

    if (window.CureCat && els.catComp) {
      window.CureCat.render(els.catComp, "idle");
    }

    if (els.startBtn) {
      els.startBtn.addEventListener("click", function () {
        if (window.CureCat && els.catComp) {
          window.CureCat.setState(els.catComp, "scanning");
        }
        if (els.progressBar) {
          els.progressBar.style.width = "20%";
        }
        if (els.idleCard) els.idleCard.classList.add("hidden");
        if (els.cleanCard) els.cleanCard.classList.add("hidden");
      });
    }
  }

  function onScanDone(summary) {
    cacheEls();

    if (els.progressBar) {
      els.progressBar.style.width = "100%";
    }

    if (window.CureCat && els.catComp) {
      window.CureCat.setState(els.catComp, "idle");
    }

    const review = (summary && summary.suspicious_for_review) || [];
    const procs = (summary && summary.process_findings) || [];
    const findingsCount = review.length + procs.length;

    if (els.threatCount) {
      els.threatCount.textContent = String(findingsCount);
      const parentCard = els.threatCount.closest(".vr-counter-threats");
      if (parentCard) {
        parentCard.classList.toggle("has-threats", findingsCount > 0);
      }
    }

    if (findingsCount === 0) {
      // Clean System State
      if (els.cleanCard) {
        els.cleanCard.classList.remove("hidden");
        if (window.CureCat && els.cleanCat) {
          window.CureCat.render(els.cleanCat, "clean");
        }
      }
      if (els.threatsContainer) els.threatsContainer.classList.add("hidden");
      if (els.idleCard) els.idleCard.classList.add("hidden");
    } else {
      // Threats Found State
      if (els.cleanCard) els.cleanCard.classList.add("hidden");
      if (els.idleCard) els.idleCard.classList.add("hidden");
      if (els.threatsContainer) els.threatsContainer.classList.remove("hidden");
      if (els.threatsBadge) els.threatsBadge.textContent = findingsCount + " threat(s) detected";

      renderThreatItems(review, procs);
    }
  }

  function renderThreatItems(entries, procs) {
    if (!els.threatsList) return;
    els.threatsList.textContent = "";

    // Render file persistence findings
    entries.forEach(function (entry) {
      const li = document.createElement("li");
      li.className = "vr-threat-row";

      const info = document.createElement("div");
      info.className = "vr-threat-info";

      const topline = document.createElement("div");
      topline.className = "vr-threat-topline";

      const name = document.createElement("span");
      name.className = "vr-threat-name";
      name.textContent = entry.name || "Suspicious Persistence Item";

      const pill = document.createElement("span");
      const isCrit = entry.risk === "HighRisk" || entry.risk === "Critical";
      pill.className = "vr-pill " + (isCrit ? "vr-pill-critical" : "vr-pill-suspicious");
      pill.textContent = isCrit ? "Critical" : "Suspicious";

      topline.append(name, pill);

      const path = document.createElement("span");
      path.className = "vr-threat-path";
      path.textContent = entry.location || entry.command || "Unknown location";
      path.title = path.textContent;

      const reason = document.createElement("span");
      reason.className = "vr-threat-reason";
      reason.textContent = entry.description || "Unsigned background hook detected in startup vector";

      info.append(topline, path, reason);

      const actions = document.createElement("div");
      actions.className = "vr-threat-actions";

      const qBtn = document.createElement("button");
      qBtn.className = "btn-quarantine";
      qBtn.textContent = "Quarantine & Remove";
      qBtn.addEventListener("click", function () {
        if (window.CureCat && els.catComp) {
          window.CureCat.swat(els.catComp);
        }
        invoke("quarantine_entry", {
          id: entry.id,
          _name: entry.name || "",
          _command: entry.command || "",
        })
          .then(function () {
            li.style.opacity = "0.4";
            actions.textContent = "Quarantined ✓";
          })
          .catch(function (err) {
            alert("Quarantine error: " + err);
          });
      });

      actions.appendChild(qBtn);

      li.append(info, actions);
      els.threatsList.appendChild(li);
    });

    // Render process findings
    procs.forEach(function (proc) {
      const li = document.createElement("li");
      li.className = "vr-threat-row";

      const info = document.createElement("div");
      info.className = "vr-threat-info";

      const topline = document.createElement("div");
      topline.className = "vr-threat-topline";

      const name = document.createElement("span");
      name.className = "vr-threat-name";
      name.textContent = proc.name || "Suspicious Active Process";

      const pill = document.createElement("span");
      pill.className = "vr-pill vr-pill-critical";
      pill.textContent = "Active Process";

      topline.append(name, pill);

      const path = document.createElement("span");
      path.className = "vr-threat-path";
      path.textContent = "PID " + proc.pid + " — " + (proc.exe_path || "");

      const reason = document.createElement("span");
      reason.className = "vr-threat-reason";
      reason.textContent = proc.reason || "Suspicious memory or process signature";

      info.append(topline, path, reason);

      const actions = document.createElement("div");
      actions.className = "vr-threat-actions";

      const killBtn = document.createElement("button");
      killBtn.className = "btn-kill";
      killBtn.textContent = "Kill Process";
      killBtn.addEventListener("click", function () {
        if (window.CureCat && els.catComp) {
          window.CureCat.swat(els.catComp);
        }
        invoke("kill_high_risk_processes", {
          processes: [[proc.name || "", Number(proc.pid)]],
        })
          .then(function () {
            li.style.opacity = "0.4";
            actions.textContent = "Terminated ✓";
          })
          .catch(function (err) {
            alert("Kill process error: " + err);
          });
      });

      actions.appendChild(killBtn);

      li.append(info, actions);
      els.threatsList.appendChild(li);
    });
  }

  window.CureVirusRescueClean = {
    init: wire,
    onScanDone: onScanDone,
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", wire);
  } else {
    wire();
  }
})();
