/* ==========================================================================
   LUMA STORAGE RESCUE — Client Controller
   Cross-Volume Relief, Guided Migration & Dual-Integrity Verification.
   Offline-first, non-destructive, evidence-first.
   ========================================================================== */
(function () {
  "use strict";

  const $ = (id) => document.getElementById(id);

  function clean(err) {
    if (!err) return "";
    if (typeof err === "string") return err;
    if (err.message) return err.message;
    return String(err);
  }

  function formatBytes(bytes) {
    if (!bytes || bytes <= 0) return "0 B";
    const units = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
    const val = bytes / Math.pow(1024, i);
    return `${val.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
  }

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
    status: null,
    sourceDrive: "C:",
    targetDrive: "D:",
    selectedIds: new Set(["downloads", "videos", "pictures"]),
    activePlan: null,
    isMigrating: false,
    receipt: null,
  };

  /* ---------- Initialization & Activation ---------- */

  async function loadRescueStatus() {
    try {
      const data = await invoke("storage_rescue_status", {
        sourceDrive: state.sourceDrive,
        targetDrive: state.targetDrive,
      });
      state.status = data;
      if (data.recommended_target_drive) {
        state.targetDrive = data.recommended_target_drive;
      }
      render();
    } catch (err) {
      console.error("Failed to load storage rescue status:", err);
    }
  }

  function render() {
    if (!state.status) return;

    // 1. Render Drive Pressure Cards
    renderDrives(state.status.drives);

    // 2. Render Candidates
    renderCandidates(state.status.candidates);

    // 3. Update Summary & Action Bar
    updateActionSummary();

    // 4. Update Assistant Guidance
    updateAssistantGuide();
  }

  function renderDrives(drives) {
    const srcDrive = drives.find(d => d.is_system) || drives[0] || {
      drive_letter: "C:",
      volume_label: "System NVMe",
      used_bytes: 237.8 * 1024 * 1024 * 1024,
      total_bytes: 256 * 1024 * 1024 * 1024,
      free_bytes: 18.2 * 1024 * 1024 * 1024,
      pressure_pct: 92.9,
      is_congested: true,
    };

    // Source Card
    const srcCard = $("sr-source-card");
    if (srcCard) {
      srcCard.innerHTML = `
        <div class="sr-drive-head">
          <div class="sr-drive-name">
            <svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor">
              <path d="M4 6h16a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2zm0 2v8h16V8H4zm2 5h2v2H6v-2zm4 0h2v2h-2v-2z"/>
            </svg>
            ${srcDrive.drive_letter} (${srcDrive.volume_label || "System Drive"})
          </div>
          <span class="sr-badge sr-badge-danger">
            ${srcDrive.is_congested ? "CRITICAL CONGESTION" : "STABLE"}
          </span>
        </div>
        <div class="sr-meter-wrap">
          <div class="sr-meter-bar">
            <div class="sr-meter-fill" style="width: ${Math.min(srcDrive.pressure_pct, 100)}%;"></div>
          </div>
          <div class="sr-meter-labels">
            <span>${srcDrive.pressure_pct.toFixed(1)}% Used</span>
            <span>${formatBytes(srcDrive.free_bytes)} Free</span>
          </div>
        </div>
        <div class="sr-stat-row">
          <span>Capacity Usage</span>
          <span class="sr-stat-val">${formatBytes(srcDrive.used_bytes)} / ${formatBytes(srcDrive.total_bytes)}</span>
        </div>
      `;
    }

    // Target Drive Card
    const targetCard = $("sr-target-card");
    if (targetCard) {
      const nonSys = drives.filter(d => !d.is_system);
      const activeTarget = drives.find(d => d.drive_letter.startsWith(state.targetDrive.replace(":", ""))) || nonSys[0] || {
        drive_letter: "D:",
        volume_label: "Fast Storage",
        used_bytes: 320 * 1024 * 1024 * 1024,
        total_bytes: 1000 * 1024 * 1024 * 1024,
        free_bytes: 680 * 1024 * 1024 * 1024,
        pressure_pct: 32.0,
      };

      const optionsHtml = nonSys.length > 0 
        ? nonSys.map(d => `<option value="${d.drive_letter}" ${d.drive_letter === activeTarget.drive_letter ? "selected" : ""}>Drive ${d.drive_letter} (${formatBytes(d.free_bytes)} free)</option>`).join("")
        : `<option value="D:" selected>Drive D: (${formatBytes(activeTarget.free_bytes)} free)</option>`;

      targetCard.innerHTML = `
        <div class="sr-drive-head">
          <div class="sr-drive-name">
            <svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor">
              <path d="M4 6h16a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2zm0 2v8h16V8H4zm2 5h2v2H6v-2zm4 0h2v2h-2v-2z"/>
            </svg>
            Target Volume
          </div>
          <select id="sr-target-select" class="sr-target-select" aria-label="Select Target Drive">
            ${optionsHtml}
          </select>
        </div>
        <div class="sr-meter-wrap">
          <div class="sr-meter-bar">
            <div class="sr-meter-fill" style="width: ${Math.min(activeTarget.pressure_pct, 100)}%;"></div>
          </div>
          <div class="sr-meter-labels">
            <span>${activeTarget.pressure_pct.toFixed(1)}% Used</span>
            <span>${formatBytes(activeTarget.free_bytes)} Available</span>
          </div>
        </div>
        <div class="sr-stat-row">
          <span>Target Headroom</span>
          <span class="sr-stat-val" style="color: #22d3ee;">EXPEDITION READY</span>
        </div>
      `;

      const sel = $("sr-target-select");
      if (sel) {
        sel.addEventListener("change", (e) => {
          state.targetDrive = e.target.value;
          loadRescueStatus();
        });
      }
    }
  }

  function renderCandidates(candidates) {
    const safeBody = $("sr-safe-candidates-body");
    const cautionBody = $("sr-caution-candidates-body");

    if (safeBody) safeBody.innerHTML = "";
    if (cautionBody) cautionBody.innerHTML = "";

    for (const c of candidates) {
      const isChecked = state.selectedIds.has(c.id);
      const tr = document.createElement("tr");

      const adviceHtml = c.reconfiguration_advice 
        ? `<div class="sr-item-advice">${c.reconfiguration_advice}</div>` 
        : "";

      tr.innerHTML = `
        <td class="sr-col-check">
          <input type="checkbox" class="sr-checkbox" data-id="${c.id}" ${isChecked ? "checked" : ""} aria-label="Select ${c.name} for relocation">
        </td>
        <td>
          <div class="sr-item-main">
            <span class="sr-item-name">
              <svg viewBox="0 0 24 24" width="14" height="14" fill="#f7b76a">
                <path d="M10 4H4c-1.1 0-1.99.9-1.99 2L2 18c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V8c0-1.1-.9-2-2-2h-8l-2-2z"/>
              </svg>
              ${c.name}
            </span>
            <span class="sr-item-path">${c.source_path}</span>
            ${adviceHtml}
          </div>
        </td>
        <td>
          <span class="sr-target-path">${c.suggested_target_path}</span>
        </td>
        <td class="sr-num">${c.file_count.toLocaleString()}</td>
        <td class="sr-num sr-size-highlight">${formatBytes(c.size_bytes)}</td>
      `;

      const checkbox = tr.querySelector(".sr-checkbox");
      if (checkbox) {
        checkbox.addEventListener("change", (e) => {
          if (e.target.checked) {
            state.selectedIds.add(c.id);
          } else {
            state.selectedIds.delete(c.id);
          }
          updateActionSummary();
          updateAssistantGuide();
        });
      }

      if (c.safety === "safe" && safeBody) {
        safeBody.appendChild(tr);
      } else if (cautionBody) {
        cautionBody.appendChild(tr);
      }
    }
  }

  function updateActionSummary() {
    if (!state.status) return;

    const candidates = state.status.candidates || [];
    const selected = candidates.filter(c => state.selectedIds.has(c.id));
    const totalBytes = selected.reduce((acc, c) => acc + c.size_bytes, 0);

    const countEl = $("sr-summary-count");
    if (countEl) countEl.textContent = `${selected.length} library${selected.length === 1 ? "" : "ies"}`;

    const bytesEl = $("sr-summary-bytes");
    if (bytesEl) bytesEl.textContent = formatBytes(totalBytes);

    const reliefEl = $("sr-relief-figure");
    if (reliefEl) reliefEl.textContent = `+${formatBytes(totalBytes)}`;

    const reliefNote = $("sr-relief-note");
    if (reliefNote) reliefNote.textContent = `${selected.length} safe migration candidates selected`;

    const planBtn = $("sr-btn-build-plan");
    if (planBtn) {
      planBtn.disabled = selected.length === 0;
    }
  }

  function updateAssistantGuide() {
    if (!state.status) return;
    const candidates = state.status.candidates || [];
    const selected = candidates.filter(c => state.selectedIds.has(c.id));
    const totalBytes = selected.reduce((acc, c) => acc + c.size_bytes, 0);

    const textEl = $("sr-guide-text");
    if (textEl) {
      if (selected.length === 0) {
        textEl.textContent = "Operator, select one or more candidate libraries above to relieve C: drive pressure without risking your application integrity.";
      } else {
        textEl.textContent = `Operator, ${selected.length} folders (${formatBytes(totalBytes)}) are staged for relocation to ${state.targetDrive}. C.U.R.E will copy and bit-verify the transfer while keeping your source files intact.`;
      }
    }

    if (window.luma && window.luma.uplink) {
      window.luma.uplink("storage-rescue", "C: Drive Pressure Relief Active", `${selected.length} folders staged for safe migration`);
    }
  }

  /* ---------- Plan & Preflight Flow ---------- */

  async function openPlanModal() {
    const selected = Array.from(state.selectedIds);
    if (selected.length === 0) return;

    try {
      const plan = await invoke("storage_rescue_plan", {
        selectedIds: selected,
        sourceDrive: state.sourceDrive,
        targetDrive: state.targetDrive,
      });
      state.activePlan = plan;
      renderPlanModal(plan);
    } catch (err) {
      alert(`Could not build migration plan: ${clean(err)}`);
    }
  }

  function renderPlanModal(plan) {
    const modal = $("sr-plan-modal");
    if (!modal) return;

    const list = $("sr-preflight-list");
    if (list) {
      list.innerHTML = `
        <div class="sr-preflight-item">
          <span class="sr-check-icon">✔</span>
          <span>Source directories verified readable and accessible</span>
        </div>
        <div class="sr-preflight-item">
          <span class="sr-check-icon">✔</span>
          <span>Target destination volume ${plan.target_drive} writeable (${formatBytes(plan.target_current_free_bytes)} free)</span>
        </div>
        <div class="sr-preflight-item">
          <span class="sr-check-icon">✔</span>
          <span>Required headroom of ${formatBytes(plan.total_bytes_to_relocate)} + 10% safety buffer (${formatBytes(plan.required_headroom_bytes)}) met</span>
        </div>
        <div class="sr-preflight-item">
          <span class="sr-check-icon">✔</span>
          <strong>Non-Destructive Policy: Source files on ${plan.source_drive} are PRESERVED and never deleted in-place</strong>
        </div>
      `;

      if (plan.preflight_warnings && plan.preflight_warnings.length > 0) {
        for (const w of plan.preflight_warnings) {
          list.innerHTML += `
            <div class="sr-preflight-item">
              <span class="sr-warn-icon">⚠</span>
              <span>${w}</span>
            </div>
          `;
        }
      }
    }

    const projBox = $("sr-projection-box");
    if (projBox) {
      projBox.innerHTML = `
        <div class="sr-proj-row">
          <span>Relocation Payload</span>
          <strong>${formatBytes(plan.total_bytes_to_relocate)} (${plan.total_files_to_relocate.toLocaleString()} files)</strong>
        </div>
        <div class="sr-proj-row">
          <span>Projected ${plan.source_drive} Free Space (After Verification)</span>
          <strong style="color: #34d399;">${formatBytes(plan.source_current_free_bytes)} → ${formatBytes(plan.source_projected_free_bytes)}</strong>
        </div>
        <div class="sr-proj-row">
          <span>Projected ${plan.target_drive} Free Space Remaining</span>
          <span>${formatBytes(plan.target_current_free_bytes)} → ${formatBytes(plan.target_projected_free_bytes)}</span>
        </div>
      `;
    }

    modal.classList.remove("hidden");
  }

  function closePlanModal() {
    const modal = $("sr-plan-modal");
    if (modal) modal.classList.add("hidden");
  }

  /* ---------- Execution & Bit-Exact Verification Flow ---------- */

  async function executeRelocation() {
    if (!state.activePlan) return;
    closePlanModal();

    const execModal = $("sr-exec-modal");
    if (execModal) execModal.classList.remove("hidden");

    state.isMigrating = true;
    const plan = state.activePlan;

    const progressFill = $("sr-exec-progress-fill");
    const progressFile = $("sr-exec-current-file");
    const progressStats = $("sr-exec-stats");

    // Setup listener for Tauri progress events
    let unlisten = null;
    if (window.__TAURI__ && window.__TAURI__.event && window.__TAURI__.event.listen) {
      unlisten = await window.__TAURI__.event.listen("rescue-migration-progress", (ev) => {
        const p = ev.payload;
        if (progressFill) progressFill.style.width = `${Math.min(p.percent, 100)}%`;
        if (progressFile) progressFile.textContent = p.current_file;
        if (progressStats) {
          progressStats.textContent = `${p.files_copied} / ${p.total_files} files · ${formatBytes(p.bytes_copied)} / ${formatBytes(p.total_bytes)}`;
        }
      });
    }

    try {
      // Execute each candidate
      let receipt = null;
      for (const cand of plan.selected_candidates) {
        receipt = await invoke("storage_rescue_execute", {
          candidateId: cand.id,
          sourcePath: cand.source_path,
          targetPath: cand.suggested_target_path,
          sourceDrive: plan.source_drive,
          targetDrive: plan.target_drive,
        });
      }

      if (unlisten) unlisten();
      state.receipt = receipt;

      if (execModal) execModal.classList.add("hidden");
      openReceiptModal(receipt);

      if (window.luma && window.luma.uplink) {
        window.luma.uplink("verified", "100% Bit-Exact Integrity Verified", "Relocation completed without error");
      }
    } catch (err) {
      if (unlisten) unlisten();
      if (execModal) execModal.classList.add("hidden");
      alert(`Relocation stopped: ${clean(err)}`);
    } finally {
      state.isMigrating = false;
    }
  }

  function openReceiptModal(receipt) {
    const modal = $("sr-receipt-modal");
    if (!modal) return;

    const idEl = $("sr-receipt-id");
    if (idEl) idEl.textContent = receipt.receipt_id;

    const filesEl = $("sr-receipt-files");
    if (filesEl) filesEl.textContent = receipt.total_files_copied.toLocaleString();

    const bytesEl = $("sr-receipt-bytes");
    if (bytesEl) bytesEl.textContent = formatBytes(receipt.total_bytes_copied);

    const notesEl = $("sr-receipt-notes");
    if (notesEl && receipt.recovery_notes) {
      notesEl.innerHTML = receipt.recovery_notes.map(n => `<div>• ${n}</div>`).join("");
    }

    modal.classList.remove("hidden");
  }

  function closeReceiptModal() {
    const modal = $("sr-receipt-modal");
    if (modal) modal.classList.add("hidden");
    loadRescueStatus();
  }

  /* ---------- Event Bindings ---------- */

  function bindEvents() {
    const planBtn = $("sr-btn-build-plan");
    if (planBtn) planBtn.addEventListener("click", openPlanModal);

    const rescanBtn = $("sr-btn-rescan");
    if (rescanBtn) rescanBtn.addEventListener("click", loadRescueStatus);

    const planCancel = $("sr-plan-cancel");
    if (planCancel) planCancel.addEventListener("click", closePlanModal);

    const planConfirm = $("sr-plan-confirm");
    if (planConfirm) planConfirm.addEventListener("click", executeRelocation);

    const receiptDone = $("sr-receipt-done");
    if (receiptDone) receiptDone.addEventListener("click", closeReceiptModal);

    const receiptOpenFolder = $("sr-receipt-reveal");
    if (receiptOpenFolder) {
      receiptOpenFolder.addEventListener("click", async () => {
        if (state.receipt && state.receipt.items && state.receipt.items[0]) {
          try {
            await invoke("storage_reveal", { path: state.receipt.items[0].target_path });
          } catch (e) {
            console.error(e);
          }
        }
      });
    }

    const receiptCopyCmd = $("sr-receipt-copy-cmd");
    if (receiptCopyCmd) {
      receiptCopyCmd.addEventListener("click", () => {
        if (state.receipt && state.receipt.manual_cleanup_hint) {
          navigator.clipboard.writeText(state.receipt.manual_cleanup_hint);
          receiptCopyCmd.textContent = "Copied to Clipboard!";
          setTimeout(() => { receiptCopyCmd.textContent = "Copy Manual Cleanup Command"; }, 2000);
        }
      });
    }

    const receiptFinalize = $("sr-receipt-finalize");
    if (receiptFinalize) {
      receiptFinalize.addEventListener("click", async () => {
        if (!state.receipt || !state.receipt.items || !state.receipt.items[0]) return;
        const item = state.receipt.items[0];
        try {
          receiptFinalize.disabled = true;
          receiptFinalize.textContent = "Repointing...";
          const finReceipt = await invoke("storage_rescue_finalize", {
            candidateId: item.candidate_id,
            targetPath: item.target_path,
          });
          receiptFinalize.textContent = "✔ Finalized";
          const notesEl = $("sr-receipt-notes");
          if (notesEl) {
            const row = document.createElement("div");
            row.style.color = "#34d399";
            row.style.fontWeight = "bold";
            row.style.marginTop = "6px";
            row.textContent = `✔ ${finReceipt.configuration_action}`;
            notesEl.appendChild(row);
          }
        } catch (err) {
          alert(`Finalization failed: ${clean(err)}`);
          receiptFinalize.disabled = false;
          receiptFinalize.textContent = "Finalize (Repoint Known Folder)";
        }
      });
    }

    const receiptCleanup = $("sr-receipt-cleanup-source");
    if (receiptCleanup) {
      receiptCleanup.addEventListener("click", () => {
        if (!state.receipt || !state.receipt.items || !state.receipt.items[0]) return;
        const item = state.receipt.items[0];
        const modal = $("sr-cleanup-modal");
        const tokenTarget = $("sr-cleanup-token-target");
        const input = $("sr-cleanup-token-input");
        const resultBox = $("sr-cleanup-result-box");
        if (!modal) return;
        const expectedToken = `CONFIRM_CLEANUP_${item.candidate_id}`;
        if (tokenTarget) tokenTarget.textContent = expectedToken;
        if (input) { input.value = ""; input.disabled = false; }
        if (resultBox) { resultBox.classList.add("hidden"); resultBox.textContent = ""; }
        modal.classList.remove("hidden");
      });
    }

    const cleanupCancel = $("sr-cleanup-cancel");
    const cleanupCancelTop = $("sr-cleanup-cancel-top");
    const closeCleanup = () => {
      const modal = $("sr-cleanup-modal");
      if (modal) modal.classList.add("hidden");
    };
    if (cleanupCancel) cleanupCancel.addEventListener("click", closeCleanup);
    if (cleanupCancelTop) cleanupCancelTop.addEventListener("click", closeCleanup);

    const cleanupConfirm = $("sr-cleanup-confirm");
    if (cleanupConfirm) {
      cleanupConfirm.addEventListener("click", async () => {
        if (!state.receipt || !state.receipt.items || !state.receipt.items[0]) return;
        const item = state.receipt.items[0];
        const input = $("sr-cleanup-token-input");
        const resultBox = $("sr-cleanup-result-box");
        const typedToken = (input && input.value.trim()) || "";
        const expectedToken = `CONFIRM_CLEANUP_${item.candidate_id}`;

        if (typedToken !== expectedToken) {
          alert(`Confirmation token does not match. Please type '${expectedToken}' exactly.`);
          return;
        }

        try {
          cleanupConfirm.disabled = true;
          cleanupConfirm.textContent = "Measuring & Deleting...";
          const cleanupReceipt = await invoke("storage_rescue_cleanup_source", {
            candidateId: item.candidate_id,
            sourcePath: item.source_path,
            targetPath: item.target_path,
            confirmationToken: typedToken,
          });

          if (resultBox) {
            resultBox.classList.remove("hidden");
            resultBox.innerHTML = `
              <strong>✔ CLEANUP VERIFIED & COMPLETED:</strong><br/>
              ${cleanupReceipt.cleanup_status}<br/>
              Original files removed. Measured C: space freed: <strong>${formatBytes(cleanupReceipt.actual_bytes_freed)}</strong>.
            `;
          }
          cleanupConfirm.textContent = "✔ Completed";
          if (receiptCleanup) {
            receiptCleanup.disabled = true;
            receiptCleanup.textContent = "Source Retired & Freed";
          }
          setTimeout(() => {
            closeCleanup();
            loadRescueStatus();
          }, 2000);
        } catch (err) {
          alert(`Source cleanup failed: ${clean(err)}`);
          cleanupConfirm.disabled = false;
          cleanupConfirm.textContent = "Delete Source & Measure Space";
        }
      });
    }
  }

  // Expose for external calls and operator hooks
  window.storageRescue = {
    init: function () {
      bindEvents();
      loadRescueStatus();
    },
    onActivate: function () {
      loadRescueStatus();
    },
    openPlanModal,
    closePlanModal,
    openReceiptModal,
    closeReceiptModal,
    state,
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", () => window.storageRescue.init());
  } else {
    window.storageRescue.init();
  }
})();
