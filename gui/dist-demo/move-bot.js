/* ==========================================================================
   C.U.R.E MOVE BOT — one-click cross-volume relocation (Stage UI-09)
   The operator sees that a folder is filling drive C:, presses one button,
   and this module does the rest: it checks what is safe, plans the move,
   asks ONCE in plain words, then copies, verifies, switches Windows over,
   and (only if the operator ticked the box) frees the space on C:.
   It drives exactly the same engine commands as the manual Rescue flow —
   no new powers, just hands that do the work.
   ========================================================================== */
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
    return val.toFixed(i === 0 ? 0 : 1) + " " + units[i];
  }

  const els = {};
  let opener = null;
  let running = false;
  let progressUnlisten = null;

  function grab() {
    els.overlay = document.getElementById("mb-overlay");
    els.confirmBody = document.getElementById("mb-confirm-body");
    els.confirmItems = document.getElementById("mb-confirm-items");
    els.confirmNote = document.getElementById("mb-confirm-note");
    els.cleanupBox = document.getElementById("mb-cleanup-box");
    els.cleanupTick = document.getElementById("mb-cleanup-tick");
    els.confirmGo = document.getElementById("mb-confirm-go");
    els.confirmCancel = document.getElementById("mb-confirm-cancel");
    els.runBody = document.getElementById("mb-run-body");
    els.runSteps = document.getElementById("mb-run-steps");
    els.runBar = document.getElementById("mb-run-bar-fill");
    els.runDetail = document.getElementById("mb-run-detail");
    els.runTitle = document.getElementById("mb-run-title");
    els.doneBody = document.getElementById("mb-done-body");
    els.doneText = document.getElementById("mb-done-text");
    els.doneDetail = document.getElementById("mb-done-detail");
    els.doneFinish = document.getElementById("mb-done-finish");
    els.errorBody = document.getElementById("mb-error-body");
    els.errorText = document.getElementById("mb-error-text");
    els.errorClose = document.getElementById("mb-error-close");
  }

  function show(phase) {
    for (const id of ["mb-confirm-body", "mb-run-body", "mb-done-body", "mb-error-body"]) {
      const el = document.getElementById(id);
      if (el) el.classList.toggle("hidden", id !== phase);
    }
  }

  function openOverlay() {
    if (els.overlay) {
      els.overlay.classList.remove("hidden");
      const go = document.getElementById("mb-confirm-go");
      if (go && !go.classList.contains("hidden")) go.focus();
    }
  }

  function closeOverlay() {
    if (running) return; // never abandon a running move by accident
    if (progressUnlisten) {
      try {
        progressUnlisten();
      } catch (_) {}
      progressUnlisten = null;
    }
    if (els.overlay) els.overlay.classList.add("hidden");
    if (opener) {
      opener.focus();
      opener = null;
    }
  }

  function stepRow(label) {
    const li = document.createElement("li");
    li.className = "mb-step is-active";
    const dot = document.createElement("span");
    dot.className = "mb-step-dot";
    const text = document.createElement("span");
    text.textContent = label;
    li.append(dot, text);
    els.runSteps.appendChild(li);
    return {
      done: function (note) {
        li.classList.remove("is-active");
        li.classList.add("is-done");
        if (note) text.textContent = label + " — " + note;
      },
      fail: function (note) {
        li.classList.remove("is-active");
        li.classList.add("is-failed");
        if (note) text.textContent = label + " — " + note;
      },
      el: li,
      text: text,
    };
  }

  function setProgress(percent, detail) {
    if (els.runBar) els.runBar.style.width = Math.min(Math.max(percent, 0), 100) + "%";
    if (els.runDetail && detail !== undefined) els.runDetail.textContent = detail;
  }

  /* ---------- the one-click flow ------------------------------------- */

  async function start(prefilterIds) {
    if (running) return;
    grab();
    opener = document.activeElement;
    show("mb-confirm-body");
    if (els.confirmGo) {
      els.confirmGo.disabled = true;
      els.confirmGo.textContent = "Checking…";
    }
    if (els.confirmItems) {
      els.confirmItems.textContent = "";
      els.confirmItems.appendChild(loadingRow("Looking at your drives…"));
    }
    if (els.confirmNote) els.confirmNote.textContent = "";
    if (els.cleanupBox) els.cleanupBox.classList.add("hidden");
    openOverlay();

    let status = null;
    try {
      status = await invoke("storage_rescue_status", {
        sourceDrive: "C:",
        targetDrive: "",
      });
    } catch (err) {
      return botError(
        "I could not read your drives, so I stopped before touching anything.",
        clean(err),
      );
    }

    const drives = (status && status.drives) || [];
    const source = drives.find((d) => d.is_system) || drives[0];
    let target = drives.find(
      (d) => d.drive_letter === (status.recommended_target_drive || "").toUpperCase(),
    );
    if (!target) target = drives.find((d) => !d.is_system);

    let candidates = (status && status.candidates) || [];
    if (Array.isArray(prefilterIds) && prefilterIds.length) {
      const wanted = new Set(prefilterIds.map((id) => String(id).toLowerCase()));
      candidates = candidates.filter((c) => wanted.has(String(c.id).toLowerCase()));
    }
    candidates = candidates.filter((c) => c.safety === "safe");

    if (!candidates.length) {
      if (els.confirmItems) {
        els.confirmItems.textContent = "";
        const p = document.createElement("p");
        p.className = "mb-plain";
        p.textContent =
          "I looked at your C: drive — right now there is nothing I can safely move. " +
          "Personal folders (Videos, Pictures, Downloads) show up here automatically once they take up real space.";
      }
      if (els.confirmGo) els.confirmGo.classList.add("hidden");
      if (els.confirmCancel) els.confirmCancel.textContent = "Close";
      if (els.confirmGo) els.confirmGo.disabled = false;
      return;
    }

    if (els.confirmGo) {
      els.confirmGo.disabled = false;
      els.confirmGo.textContent = "Move it for me";
      els.confirmGo.classList.remove("hidden");
    }
    if (els.confirmCancel) els.confirmCancel.textContent = "Cancel";

    const totalBytes = candidates.reduce((a, c) => a + (c.size_bytes || 0), 0);

    if (els.confirmItems) {
      els.confirmItems.textContent = "";
      for (const c of candidates) {
        const row = document.createElement("div");
        row.className = "mb-item-row";
        const name = document.createElement("span");
        name.className = "mb-item-name";
        name.textContent = c.name;
        const size = document.createElement("span");
        size.className = "mb-item-size";
        size.textContent = formatBytes(c.size_bytes);
        row.append(name, size);
        els.confirmItems.appendChild(row);
      }
    }

    const targetLabel = target
      ? "drive " + target.drive_letter + " (" + formatBytes(target.free_bytes) + " free)"
      : "your second drive";
    const sourceLabel = source ? "drive " + source.drive_letter : "drive C:";
    if (els.confirmNote) {
      els.confirmNote.textContent =
        "I will move " +
        candidates.length +
        (candidates.length === 1 ? " folder" : " folders") +
        " (" +
        formatBytes(totalBytes) +
        ") from " +
        sourceLabel +
        " to " +
        targetLabel +
        ". I copy everything first, check it bit by bit, then switch Windows over. Nothing is deleted until the copy is fully verified, and you can undo.";
    }
    if (els.cleanupBox) els.cleanupBox.classList.remove("hidden");
    const cleanupTick = document.getElementById("mb-cleanup-tick");
    if (cleanupTick) cleanupTick.checked = false;

    pendingPlan = {
      candidates: candidates,
      targetDrive: target ? target.drive_letter : (status.recommended_target_drive || "D:"),
    };
  }

  function loadingRow(text) {
    const p = document.createElement("p");
    p.className = "mb-plain";
    p.textContent = text;
    return p;
  }

  let pendingPlan = null;

  async function confirmMove() {
    if (!pendingPlan || running) return;
    const cleanupAlso = !!(els.cleanupTick && els.cleanupTick.checked);
    running = true;
    show("mb-run-body");
    if (els.runSteps) els.runSteps.textContent = "";
    setProgress(0, "");
    if (els.runTitle)
      els.runTitle.textContent =
        "Moving " + pendingPlan.candidates.length + (pendingPlan.candidates.length === 1 ? " folder" : " folders") + " — I am on it.";

    const byId = new Map(pendingPlan.candidates.map((c) => [String(c.id).toLowerCase(), c]));
    const results = [];
    let failed = null;

    // Listen once for copy progress; events carry the candidate id.
    if (window.__TAURI__ && window.__TAURI__.event && window.__TAURI__.event.listen) {
      try {
        progressUnlisten = await window.__TAURI__.event.listen(
          "rescue-migration-progress",
          (ev) => {
            const p = ev.payload || {};
            if (els.runBar) els.runBar.style.width = Math.min(p.percent || 0, 100) + "%";
            if (els.runDetail)
              els.runDetail.textContent =
                (p.files_copied || 0) + " / " + (p.total_files || 0) + " files · " +
                formatBytes(p.bytes_copied || 0) + " of " + formatBytes(p.total_bytes || 0);
          },
        );
      } catch (_) {}
    }

    for (const cand of pendingPlan.candidates) {
      const copyStep = stepRow("Copying " + cand.name);
      let receipt = null;
      try {
        receipt = await invoke("storage_rescue_execute", {
          candidateId: cand.id,
          sourcePath: cand.source_path,
          targetPath: cand.suggested_target_path,
          sourceDrive: "C:",
          targetDrive: pendingPlan.targetDrive,
        });
        copyStep.done(formatBytes(receipt.total_bytes_copied || cand.size_bytes || 0) + " copied, checksums match");
      } catch (err) {
        copyStep.fail("stopped safely");
        failed = {
          phase: "copy",
          candidate: cand,
          message: clean(err),
        };
        break;
      }

      const switchStep = stepRow("Switching Windows to the new location");
      let activation = null;
      try {
        activation = await invoke("storage_rescue_activate", {
          candidateId: cand.id,
          targetPath: receipt.items && receipt.items[0] ? receipt.items[0].target_path : cand.suggested_target_path,
        });
        if (activation.activation_succeeded && activation.application_validation_passed) {
          switchStep.done("done");
        } else {
          switchStep.fail("Windows refused the switch");
          failed = {
            phase: "activate",
            candidate: cand,
            message: (activation.notes || []).join("; ") || "the new location could not be validated",
            receipt: receipt,
            activation: activation,
          };
          break;
        }
      } catch (err) {
        switchStep.fail("Windows refused the switch");
        failed = {
          phase: "activate",
          candidate: cand,
          message: clean(err),
          receipt: receipt,
        };
        break;
      }

      let cleaned = null;
      if (cleanupAlso && activation.cleanup_unlocked) {
        const freeStep = stepRow("Freeing the space on C:");
        try {
          cleaned = await invoke("storage_rescue_cleanup_source", {
            candidateId: cand.id,
            sourcePath: cand.source_path,
            targetPath:
              receipt.items && receipt.items[0] ? receipt.items[0].target_path : cand.suggested_target_path,
            confirmationToken: activation.required_cleanup_token || "",
            expectedNonce: activation.cleanup_nonce || "",
          });
          freeStep.done(formatBytes(cleaned.actual_bytes_freed || 0) + " freed");
        } catch (err) {
          freeStep.fail("could not free it — originals kept, you can free them later");
        }
      }

      results.push({
        candidate: cand,
        receipt: receipt,
        activation: activation,
        cleanup: cleaned,
      });
    }

    if (progressUnlisten) {
      try {
        progressUnlisten();
      } catch (_) {}
      progressUnlisten = null;
    }
    running = false;

    if (failed) {
      showBotFailure(failed, results);
      return;
    }

    showBotDone(results);
  }

  function showBotFailure(failure, results) {
    show("mb-error-body");
    const copyFailed = failure.phase === "copy";
    const headline = copyFailed
      ? "I stopped before anything changed. Your files are exactly where they were."
      : "The copy is safe on " + pendingPlan.targetDrive + ", but Windows would not switch over.";
    const detail = document.createElement("div");
    detail.className = "mb-error-detail";
    const line1 = document.createElement("p");
    line1.textContent = headline;
    const line2 = document.createElement("p");
    line2.className = "mb-plain";
    line2.textContent = "Reason: " + failure.message;
    detail.append(line1, line2);
    const box = document.getElementById("mb-error-text");
    if (box) {
      box.textContent = "";
      box.appendChild(detail);
    }
    const undo = document.getElementById("mb-error-undo");
    if (undo) {
      if (!copyFailed && failure.receipt) {
        undo.classList.remove("hidden");
        undo.onclick = async function () {
          undo.disabled = true;
          undo.textContent = "Putting everything back…";
          try {
            await invoke("storage_rescue_rollback", {
              candidateId: failure.candidate.id,
              originalPath: failure.candidate.source_path,
            });
            undo.textContent = "Undone — back to the original setup.";
          } catch (err) {
            undo.textContent = "Undo failed: " + clean(err);
          }
          const close = document.getElementById("mb-error-close");
          if (close) close.classList.remove("hidden");
        };
      } else {
        undo.classList.add("hidden");
      }
    }
    const close = document.getElementById("mb-error-close");
    if (close) {
      close.classList.remove("hidden");
      close.focus();
    }
  }

  function showBotDone(results) {
    show("mb-done-body");
    const totalBytes = results.reduce(
      (a, r) => a + ((r.receipt && r.receipt.total_bytes_copied) || r.candidate.size_bytes || 0),
      0,
    );
    const freed = results.reduce(
      (a, r) => a + ((r.cleanup && r.cleanup.actual_bytes_freed) || 0),
      0,
    );
    if (els.doneText) {
      els.doneText.textContent =
        "All moved and verified. " +
        formatBytes(totalBytes) +
        " now lives on drive " +
        pendingPlan.targetDrive +
        "." +
        (freed > 0 ? " I also freed " + formatBytes(freed) + " on C:." : "");
    }
    if (els.doneDetail) {
      els.doneDetail.textContent = "";
      for (const r of results) {
        const row = document.createElement("div");
        row.className = "mb-item-row";
        const name = document.createElement("span");
        name.className = "mb-item-name";
        name.textContent = r.candidate.name;
        const where = document.createElement("span");
        where.className = "mb-item-where";
        where.textContent =
          r.receipt && r.receipt.items && r.receipt.items[0]
            ? r.receipt.items[0].target_path
            : r.candidate.suggested_target_path;
        row.append(name, where);
        els.doneDetail.appendChild(row);
      }
    }
    const finish = document.getElementById("mb-done-finish");
    if (finish) finish.focus();
  }

  function botError(headline, detail) {
    running = false;
    show("mb-error-body");
    const box = document.getElementById("mb-error-text");
    if (box) {
      box.textContent = "";
      const h = document.createElement("p");
      h.textContent = headline;
      const d = document.createElement("p");
      d.className = "mb-plain";
      d.textContent = detail || "";
      box.append(h, d);
    }
    const undo = document.getElementById("mb-error-undo");
    if (undo) undo.classList.add("hidden");
    const close = document.getElementById("mb-error-close");
    if (close) {
      close.classList.remove("hidden");
      close.focus();
    }
  }

  /* ---------- bindings ------------------------------------------------ */

  function bind() {
    grab();
    const go = document.getElementById("mb-confirm-go");
    if (go) go.addEventListener("click", confirmMove);
    const cancel = document.getElementById("mb-confirm-cancel");
    if (cancel) cancel.addEventListener("click", closeOverlay);
    const doneFinish = document.getElementById("mb-done-finish");
    if (doneFinish)
      doneFinish.addEventListener("click", function () {
        pendingPlan = null;
        closeOverlay();
        if (window.storageRescue && window.storageRescue.onActivate) {
          window.storageRescue.onActivate();
        }
      });
    const errorClose = document.getElementById("mb-error-close");
    if (errorClose)
      errorClose.addEventListener("click", function () {
        pendingPlan = null;
        closeOverlay();
        if (window.storageRescue && window.storageRescue.onActivate) {
          window.storageRescue.onActivate();
        }
      });
    const cleanupTick = document.getElementById("mb-cleanup-tick");
    if (cleanupTick) {
      cleanupTick.addEventListener("change", function () {
        if (els.confirmNote) {
          els.confirmNote.classList.toggle("mb-note-strong", cleanupTick.checked);
        }
      });
    }
    document.addEventListener("keydown", function (ev) {
      if (ev.key !== "Escape" || !els.overlay || els.overlay.classList.contains("hidden")) return;
      if (running) return; // never abandon a running move
      pendingPlan = null;
      closeOverlay();
    });
    // Entry points are wired in wireEntries() once the DOM is ready.
  }

  function wireEntries() {
    const srBtn = document.getElementById("sr-btn-move-bot");
    if (srBtn) {
      srBtn.addEventListener("click", function () {
        start();
      });
    }
    const dtBtn = document.getElementById("dt-move-bot");
    if (dtBtn) {
      dtBtn.addEventListener("click", function () {
        start();
      });
    }
  }

  window.CureMoveBot = {
    start: start,
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", function () {
      bind();
      wireEntries();
    });
  } else {
    bind();
    wireEntries();
  }
})();
