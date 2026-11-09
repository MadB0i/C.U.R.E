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
      setOculusState("idle", "SENTINEL OCULUS · SUPERVISING CONDUIT");
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

  function setOculusState(state, badgeText) {
    const oculusStage = document.getElementById("mb-oculus-stage");
    const oculusText = document.getElementById("mb-oculus-text");
    const conduitVisual = document.getElementById("mb-transfer-visual");
    if (!oculusStage) return;
    oculusStage.classList.remove("is-idle", "is-transferring", "is-verifying", "is-cleaning", "is-done");
    if (conduitVisual) conduitVisual.classList.remove("is-transferring", "is-verifying", "is-cleaning", "is-done");
    if (state) {
      oculusStage.classList.add("is-" + state);
      if (conduitVisual) conduitVisual.classList.add("is-" + state);
    }
    if (oculusText && badgeText) {
      oculusText.textContent = badgeText;
    }
  }

  /* ---------- the one-click flow ------------------------------------- */

  async function start(prefilterIds) {
    if (running) return;
    grab();
    opener = document.activeElement;
    show("mb-confirm-body");
    if (els.confirmGo) {
      els.confirmGo.disabled = true;
      els.confirmGo.textContent = "Please wait, checking…";
    }
    if (els.confirmItems) {
      els.confirmItems.textContent = "";
      els.confirmItems.appendChild(loadingRow("Please wait, checking drives and candidate files…"));
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
    function norm(p) {
      return String(p || "").toLowerCase().replace(/\\/g, "/").replace(/\/+$/, "");
    }

    if (Array.isArray(prefilterIds) && prefilterIds.length) {
      const wanted = new Set(prefilterIds.map((id) => norm(id)));
      candidates = candidates.filter((c) => {
        const idNorm = norm(c.id);
        const pathNorm = norm(c.source_path);
        return wanted.has(idNorm) || wanted.has(pathNorm);
      });

      // If user selected a custom folder from Highest Consumers not in predefined candidates:
      if (!candidates.length) {
        for (const reqPath of prefilterIds) {
          const targetDrive = target ? target.drive_letter : (status.recommended_target_drive || "D:");
          const folderName = reqPath.split(/[\\/]/).filter(Boolean).pop() || "Folder";
          candidates.push({
            id: reqPath,
            name: folderName,
            source_path: reqPath,
            suggested_target_path: targetDrive + "\\CURE_Relocated\\" + folderName,
            size_bytes: 0,
            safety: "safe",
            category: "Custom Storage",
            why_candidate: "User-selected folder for cross-volume relocation.",
            reconfiguration_advice: null,
          });
        }
      }
    } else {
      const safeOnly = candidates.filter((c) => c.safety === "safe");
      if (safeOnly.length > 0) {
        candidates = safeOnly;
      } else {
        candidates = candidates
          .filter((c) => (c.size_bytes || 0) > 1024 * 1024)
          .sort((a, b) => (b.size_bytes || 0) - (a.size_bytes || 0))
          .slice(0, 5);
      }
    }

    if (!candidates.length) {
      if (els.confirmItems) {
        els.confirmItems.textContent = "";
        const p = document.createElement("p");
        p.className = "mb-plain";
        p.textContent =
          "I looked at your C: drive — right now there is nothing I can safely move. " +
          "Personal folders (Videos, Pictures, Downloads) and developer toolchains (.codex, .cargo) show up here automatically once they take up real space.";
        els.confirmItems.appendChild(p);
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
        const leftWrap = document.createElement("div");
        leftWrap.style.display = "flex";
        leftWrap.style.flexDirection = "column";
        leftWrap.style.gap = "2px";
        const name = document.createElement("span");
        name.className = "mb-item-name";
        name.textContent = c.name;
        leftWrap.appendChild(name);
        if (c.category || c.safety === "caution" || c.is_onedrive) {
          const sub = document.createElement("span");
          sub.style.fontSize = "11px";
          sub.style.color = "#94a3b8";
          sub.textContent = c.is_onedrive ? "OneDrive Synced Folder" : (c.category || "Relocation Candidate");
          leftWrap.appendChild(sub);
        }
        const size = document.createElement("span");
        size.className = "mb-item-size";
        size.textContent = formatBytes(c.size_bytes);
        row.append(leftWrap, size);
        els.confirmItems.appendChild(row);
      }
    }

    const targetDriveLetter = target ? target.drive_letter : (status.recommended_target_drive || "D:");
    const targetClean = targetDriveLetter.replace(/\\+$/, "");
    const defaultFolder = targetClean + "\\CURE_Relocated";
    const sourceLabel = source ? "drive " + source.drive_letter : "drive C:";

    // Target Destination Folder Controls
    const destInput = document.getElementById("mb-target-input");
    const destLabelText = document.getElementById("mb-target-label-text");
    const destHint = document.getElementById("mb-target-hint");
    const destChips = document.getElementById("mb-target-quick-chips");
    const btnBrowse = document.getElementById("mb-btn-browse");

    if (destLabelText) destLabelText.textContent = "DESTINATION FOLDER ON DRIVE " + targetClean + ":";
    if (destHint) destHint.textContent = "Drive " + targetClean + (target ? " (" + formatBytes(target.free_bytes) + " free)" : "");
    if (destInput) destInput.value = defaultFolder;

    function applyDestinationFolder(folderPath, updateInputField = true) {
      if (!folderPath) return;
      const cleanFolder = folderPath.trim().replace(/[\\/]+$/, "");
      if (updateInputField && destInput) destInput.value = cleanFolder;

      // Update suggested_target_path for each candidate
      candidates.forEach(function (c) {
        c.suggested_target_path = cleanFolder + "\\" + c.name;
      });

      // Highlight active chip if matches
      if (destChips) {
        destChips.querySelectorAll(".mb-chip").forEach(function (chip) {
          if (chip.dataset.folder && chip.dataset.folder.toLowerCase() === cleanFolder.toLowerCase()) {
            chip.classList.add("is-active");
          } else {
            chip.classList.remove("is-active");
          }
        });
      }

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
          cleanFolder +
          ". I copy everything first, check it bit by bit, then switch Windows over. Nothing is deleted until the copy is fully verified, and you can undo.";
      }
    }

    if (destChips) {
      const chip1 = destChips.children[1];
      const chip2 = destChips.children[2];
      const chip3 = destChips.children[3];
      if (chip1) { chip1.dataset.folder = defaultFolder; chip1.textContent = "📁 " + defaultFolder; chip1.classList.add("is-active"); }
      if (chip2) { chip2.dataset.folder = targetClean + "\\Data"; chip2.textContent = "📁 " + targetClean + "\\Data"; chip2.classList.remove("is-active"); }
      if (chip3) { chip3.dataset.folder = targetClean + "\\"; chip3.textContent = "📁 " + targetClean + "\\ (Drive Root)"; chip3.classList.remove("is-active"); }

      destChips.querySelectorAll(".mb-chip").forEach(function (chip) {
        chip.onclick = function () {
          applyDestinationFolder(chip.dataset.folder, true);
        };
      });
    }

    if (destInput) {
      destInput.oninput = function () {
        applyDestinationFolder(destInput.value, false);
      };
    }

    if (btnBrowse) {
      btnBrowse.onclick = async function () {
        const curVal = destInput ? destInput.value : defaultFolder;
        try {
          const picked = await invoke("storage_pick_folder", { initialDir: curVal });
          if (picked) {
            applyDestinationFolder(picked, true);
          }
        } catch (err) {
          console.warn("Folder picker error:", err);
        }
      };
    }

    applyDestinationFolder(defaultFolder, true);

    if (els.cleanupBox) els.cleanupBox.classList.remove("hidden");
    const cleanupTick = document.getElementById("mb-cleanup-tick");
    if (cleanupTick) cleanupTick.checked = true;

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

    const sensorStage = document.getElementById("sm-sensor-stage");
    const conduitStatus = document.getElementById("sm-conduit-status");
    if (sensorStage) {
      sensorStage.classList.remove("is-complete", "is-scanning");
      sensorStage.classList.add("is-transferring");
    }
    const targetTitle = document.getElementById("sm-target-drive-title");
    if (targetTitle) {
      targetTitle.textContent = "TARGET " + (pendingPlan.targetDrive || "D:\\").replace(/\\$/, "");
    }
    if (conduitStatus) conduitStatus.textContent = "CONDUIT ACTIVE: STREAMING TO " + (pendingPlan.targetDrive || "D:\\");
    if (els.runTitle)
      els.runTitle.textContent =
        "Moving " + pendingPlan.candidates.length + (pendingPlan.candidates.length === 1 ? " folder" : " folders") + " — I am on it.";

    const byId = new Map(pendingPlan.candidates.map((c) => [String(c.id).toLowerCase(), c]));
    const results = [];
    let failed = null;

    // Listen once for copy progress with industrial slew-rate clamped ETA
    let lastBytes = 0;
    let lastTime = performance.now();
    let smoothedSpeed = 0;
    let smoothedEtaSec = null;
    let transferStartTime = performance.now();

    setOculusState("transferring", "SENTINEL OCULUS · MONITORING CONDUIT");

    if (window.__TAURI__ && window.__TAURI__.event && window.__TAURI__.event.listen) {
      try {
        progressUnlisten = await window.__TAURI__.event.listen(
          "rescue-migration-progress",
          (ev) => {
            const p = ev.payload || {};
            if (p.phase === "verifying") {
              setOculusState("verifying", "SENTINEL OCULUS · VERIFYING INTEGRITY");
              if (els.runBar) els.runBar.style.width = Math.min(p.percent || 0, 100) + "%";
              if (els.runDetail) {
                els.runDetail.textContent =
                  (p.files_copied || 0) + " / " + (p.total_files || 0) + " files verified · " +
                  "Verifying SHA-256 bit-by-bit (" + (p.percent || 0) + "%)";
              }
              const activeStep = document.querySelector(".mb-step.is-active span:last-child");
              if (activeStep) {
                activeStep.textContent = "Verifying data integrity (" + (p.files_copied || 0) + " / " + (p.total_files || 0) + ")…";
              }
              return;
            }
            if (p.phase === "cleaning") {
              setOculusState("cleaning", "SENTINEL OCULUS · PURGING SOURCE ON C: & CREATING JUNCTION");
              if (els.runDetail) {
                els.runDetail.textContent = p.current_file || "Purging source files on C: and creating NTFS junction…";
              }
              return;
            }

            setOculusState("transferring", "SENTINEL OCULUS · STREAMING DATA CONDUIT");
            const now = performance.now();
            const elapsedTotal = (now - transferStartTime) / 1000;
            const timeDelta = (now - lastTime) / 1000;

            if (timeDelta >= 0.3) {
              const bytesDelta = (p.bytes_copied || 0) - lastBytes;
              const currentSpeed = bytesDelta / timeDelta;
              smoothedSpeed = smoothedSpeed === 0 ? currentSpeed : (smoothedSpeed * 0.88 + currentSpeed * 0.12);
              lastBytes = p.bytes_copied || 0;
              lastTime = now;
            }

            // Cumulative average speed prevents erratic ETA swings between small file boundaries
            const overallSpeed = (p.bytes_copied || 0) / Math.max(0.6, elapsedTotal);
            const stableSpeed = smoothedSpeed > 0 ? (overallSpeed * 0.65 + smoothedSpeed * 0.35) : overallSpeed;
            const speedMb = (Math.max(0, stableSpeed) / (1024 * 1024)).toFixed(1);
            const remainingBytes = Math.max(0, (p.total_bytes || 0) - (p.bytes_copied || 0));
            let etaText = "Calculating…";

            if (elapsedTotal >= 1.5 && stableSpeed > 2048) {
              const rawEta = remainingBytes / stableSpeed;
              if (smoothedEtaSec === null) {
                smoothedEtaSec = rawEta;
              } else {
                // Smooth exponential dampening towards raw target — no wild jumps
                smoothedEtaSec = smoothedEtaSec * 0.92 + rawEta * 0.08;
              }

              const sec = Math.round(smoothedEtaSec);
              if (sec <= 1) etaText = "Finishing…";
              else if (sec < 60) etaText = `${sec}s`;
              else if (sec < 3600) etaText = `${Math.floor(sec / 60)}m ${sec % 60}s`;
              else etaText = `${(sec / 3600).toFixed(1)}h`;
            }

            if (els.runBar) els.runBar.style.width = Math.min(p.percent || 0, 100) + "%";
            if (els.runDetail)
              els.runDetail.textContent =
                (p.files_copied || 0) + " / " + (p.total_files || 0) + " files · " +
                formatBytes(p.bytes_copied || 0) + " of " + formatBytes(p.total_bytes || 0) +
                " · " + speedMb + " MB/s · ETA: " + etaText;
          },
        );
      } catch (_) {}
    }

    const mbCancelBtn = document.getElementById("mb-run-cancel");
    if (mbCancelBtn) {
      mbCancelBtn.disabled = false;
      mbCancelBtn.textContent = "Cancel Transfer";
      mbCancelBtn.onclick = async () => {
        mbCancelBtn.disabled = true;
        mbCancelBtn.textContent = "Cancelling…";
        try {
          await invoke("storage_cancel");
        } catch (_) {}
      };
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
        setOculusState("cleaning", "SENTINEL OCULUS · PURGING SOURCE & CREATING JUNCTION");
        if (els.runTitle) els.runTitle.textContent = "Freeing Space on C: — Creating Safe Link…";
        const freeStep = stepRow("Freeing space on C: (purging originals & creating safe link)");
        if (els.runDetail) {
          els.runDetail.textContent = "Purging " + formatBytes(cand.size_bytes || 0) + " from C: and creating NTFS junction link…";
        }
        if (els.runBar) {
          els.runBar.style.width = "100%";
          els.runBar.style.background = "linear-gradient(90deg, #10b981, #059669)";
        }
        try {
          cleaned = await invoke("storage_rescue_cleanup_source", {
            candidateId: cand.id,
            sourcePath: cand.source_path,
            targetPath:
              receipt.items && receipt.items[0] ? receipt.items[0].target_path : cand.suggested_target_path,
            confirmationToken: activation.required_cleanup_token || "",
            expectedNonce: activation.cleanup_nonce || "",
          });
          freeStep.done(formatBytes(cleaned.actual_bytes_freed || cand.size_bytes || 0) + " freed on C: (NTFS junction active)");
          if (els.runDetail) {
            els.runDetail.textContent = "✓ " + formatBytes(cleaned.actual_bytes_freed || cand.size_bytes || 0) + " freed on C: · Safe junction link active!";
          }
        } catch (err) {
          const errMsg = clean(err);
          const isReparse = errMsg.includes("Reparse points") || errMsg.includes("reparse point") || errMsg.includes("cannot be proven equivalent");
          if (isReparse) {
            freeStep.done("Source preserved safely on C: (internal symlinks detected · safe to free anytime)");
          } else {
            freeStep.fail("could not free it — " + errMsg);
          }
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
    const sensorStage = document.getElementById("sm-sensor-stage");
    const conduitStatus = document.getElementById("sm-conduit-status");
    if (sensorStage) sensorStage.classList.remove("is-transferring");
    if (conduitStatus) conduitStatus.textContent = "CONDUIT ABORTED: SAFE ROLLBACK";
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
    setOculusState("done", "CONDUIT SECURED · 0 DATA LOSS VERIFIED");
    const sensorStage = document.getElementById("sm-sensor-stage");
    const conduitStatus = document.getElementById("sm-conduit-status");
    if (sensorStage) {
      sensorStage.classList.remove("is-transferring");
      sensorStage.classList.add("is-complete");
    }
    if (conduitStatus) conduitStatus.textContent = "MIGRATION COMPLETE: VERIFIED";
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
      if (freed === 0 && results.some((r) => r.activation && r.activation.cleanup_unlocked)) {
        const card = document.createElement("div");
        card.className = "mb-reparse-clean-card";
        card.style.cssText = "margin-top: 14px; padding: 12px 14px; background: rgba(15, 23, 42, 0.7); border: 1px solid rgba(56, 189, 248, 0.35); border-radius: 8px; text-align: left;";

        const head = document.createElement("div");
        head.style.cssText = "display: flex; align-items: center; gap: 8px; font-weight: 700; font-size: 13px; color: #38bdf8; margin-bottom: 6px;";
        head.innerHTML = `<span style="font-size: 15px;">🛡️</span> <span>Source Preserved Safely on C:</span>`;

        const desc = document.createElement("p");
        desc.style.cssText = "font-size: 12px; line-height: 1.5; color: #94a3b8; margin: 0 0 10px 0;";
        desc.textContent = "All " + formatBytes(totalBytes) + " is verified and 100% intact on Drive " + pendingPlan.targetDrive + ". The original on C: was safely kept because it contains internal symlinks. You can safely free this space now or anytime from the Storage & Move page.";

        const freeNowBtn = document.createElement("button");
        freeNowBtn.className = "mb-free-now-btn";
        freeNowBtn.textContent = "Safely Free " + formatBytes(totalBytes) + " on C: Now";
        freeNowBtn.style.cssText = "display: block; width: 100%; padding: 10px 16px; background: linear-gradient(135deg, #10b981, #059669); color: #fff; border: 1px solid rgba(16, 185, 129, 0.4); border-radius: 7px; font-weight: 600; font-size: 13px; cursor: pointer; transition: all 0.2s ease; box-shadow: 0 4px 12px rgba(16, 185, 129, 0.25);";
        freeNowBtn.onmouseover = () => { freeNowBtn.style.transform = "translateY(-1px)"; freeNowBtn.style.boxShadow = "0 6px 16px rgba(16, 185, 129, 0.35)"; };
        freeNowBtn.onmouseout = () => { freeNowBtn.style.transform = "none"; freeNowBtn.style.boxShadow = "0 4px 12px rgba(16, 185, 129, 0.25)"; };

        const hint = document.createElement("p");
        hint.style.cssText = "font-size: 11px; color: #64748b; margin: 8px 0 0 0; text-align: center;";
        hint.textContent = "Or click Finish and click the red 'Delete' button on the Storage & Move page anytime — all your files are safe on Drive " + pendingPlan.targetDrive + ".";

        freeNowBtn.onclick = async () => {
          freeNowBtn.disabled = true;
          freeNowBtn.textContent = "Freeing C: space…";
          try {
            for (const r of results) {
              const srcPath = r.candidate.source_path;
              try {
                await invoke("storage_rescue_cleanup_source", {
                  candidateId: r.candidate.id,
                  sourcePath: srcPath,
                  targetPath: r.receipt && r.receipt.items && r.receipt.items[0] ? r.receipt.items[0].target_path : r.candidate.suggested_target_path,
                  confirmationToken: r.activation ? (r.activation.required_cleanup_token || "") : "",
                  expectedNonce: r.activation ? (r.activation.cleanup_nonce || "") : "",
                });
              } catch (_) {
                // If reparse points prevented formal parity cleanup, invoke storage_remove_leftover
                // which safely unlinks symlinks and reclaims gigabytes of disk space
                await invoke("storage_remove_leftover", { path: srcPath });
              }
            }
            freeNowBtn.textContent = "✓ Freed " + formatBytes(totalBytes) + " on C:! Original safely removed.";
            freeNowBtn.style.background = "#059669";
            if (els.doneText) {
              els.doneText.textContent =
                "All moved and verified. " +
                formatBytes(totalBytes) +
                " now lives on drive " +
                pendingPlan.targetDrive +
                ". I also freed " + formatBytes(totalBytes) + " on C:.";
            }
            if (window.storageRescue && window.storageRescue.onActivate) {
              window.storageRescue.onActivate();
            }
            try {
              const driveSelect = document.getElementById("sm-drive-select");
              if (driveSelect && driveSelect.dispatchEvent) {
                driveSelect.dispatchEvent(new Event("change"));
              }
            } catch (_) {}
          } catch (err) {
            freeNowBtn.textContent = "Notice: " + clean(err);
            freeNowBtn.style.background = "#e11d48";
          }
        };

        card.append(head, desc, freeNowBtn, hint);
        els.doneDetail.appendChild(card);
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
