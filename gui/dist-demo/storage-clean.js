/**
 * Unified 1-Click Storage & Move Controller
 * Powering Block A (Drive Selector), Block B (Highest Space Consumers), and Block C (Uninstalled App Leftovers)
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

  function fmtBytes(bytes) {
    if (!bytes || bytes <= 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return (bytes / Math.pow(k, i)).toFixed(i >= 2 ? 1 : 0) + " " + sizes[i];
  }

  const state = {
    drives: [],
    selectedDrive: "C:\\",
    lastScan: null,
    isScanning: false,
    leftovers: [],
  };

  const els = {};

  function cacheEls() {
    els.drivesGrid = document.getElementById("sm-drives-grid");
    els.scanBtn = document.getElementById("sw-scan");
    els.scanBtnText = document.getElementById("sm-scan-btn-text");
    els.cancelBtn = document.getElementById("sw-cancel");
    els.catComp = document.getElementById("sm-cat-companion");
    els.progressWrap = document.getElementById("sw-progress");
    els.progressBar = document.getElementById("sm-scan-bar-fill");
    els.progressPhase = document.getElementById("sw-progress-phase");
    els.progressDetail = document.getElementById("sw-progress-detail");
    els.progressBytes = document.getElementById("sw-count-bytes");
    els.consumersBody = document.getElementById("sm-consumers-body");
    els.consumersBadge = document.getElementById("sm-consumers-badge");
    els.leftoversList = document.getElementById("sm-leftovers-list");
    els.rescanLeftovers = document.getElementById("sm-rescan-leftovers");
  }

  async function loadDrives() {
    try {
      const roots = await invoke("storage_available_roots");
      state.drives = (roots || []).filter(function (r) {
        return r.is_drive || (r.path && r.path.length <= 3 && r.path.includes(":"));
      });
      renderDrives();
    } catch (e) {
      console.warn("Failed to load drives:", e);
    }
  }

  function renderDrives() {
    if (!els.drivesGrid) return;
    els.drivesGrid.textContent = "";

    if (!state.drives.length) {
      // Fallback C: drive
      state.drives = [
        { path: "C:\\", label: "Drive C", is_drive: true, total_bytes: 512 * 1024 * 1024 * 1024, free_bytes: 45 * 1024 * 1024 * 1024 }
      ];
    }

    state.drives.forEach(function (drive) {
      const card = document.createElement("div");
      card.className = "sm-drive-card" + (drive.path === state.selectedDrive ? " is-selected" : "");

      const top = document.createElement("div");
      top.className = "sm-drive-top";

      const letter = document.createElement("span");
      letter.className = "sm-drive-letter";
      letter.textContent = drive.path;

      const tag = document.createElement("span");
      tag.className = "sm-drive-tag";
      const isSys = drive.path.toUpperCase().startsWith("C:");
      tag.textContent = isSys ? "System SSD" : (drive.label || "Drive");

      top.append(letter, tag);

      const total = drive.total_bytes || 1;
      const free = drive.free_bytes || 0;
      const used = Math.max(0, total - free);
      const pct = Math.min(100, Math.max(0, (used / total) * 100));

      const meter = document.createElement("div");
      meter.className = "sm-drive-meter";
      const fill = document.createElement("div");
      fill.className = "sm-drive-fill" + (pct > 88 ? " is-high" : "");
      fill.style.width = pct.toFixed(1) + "%";
      meter.appendChild(fill);

      const space = document.createElement("div");
      space.className = "sm-drive-space";
      space.textContent = fmtBytes(free) + " free of " + fmtBytes(total);

      card.append(top, meter, space);

      card.addEventListener("click", function () {
        state.selectedDrive = drive.path;
        document.querySelectorAll(".sm-drive-card").forEach(function (c) {
          c.classList.remove("is-selected");
        });
        card.classList.add("is-selected");
        if (els.scanBtnText) {
          els.scanBtnText.textContent = "Scan Selected Drive (" + drive.path + ")";
        }
      });

      els.drivesGrid.appendChild(card);
    });
  }

  async function startDriveScan() {
    if (state.isScanning) return;
    state.isScanning = true;

    if (els.scanBtn) els.scanBtn.disabled = true;
    if (els.cancelBtn) els.cancelBtn.classList.remove("hidden");
    if (els.progressWrap) els.progressWrap.classList.remove("hidden");
    if (els.progressBar) els.progressBar.style.width = "10%";
    if (els.progressPhase) els.progressPhase.textContent = "Scanning " + state.selectedDrive + "…";
    if (els.progressDetail) els.progressDetail.textContent = "Looking for heavy folders and files…";

    if (window.CureCat && els.catComp) {
      window.CureCat.setState(els.catComp, "scanning");
    }

    let scan = null;
    try {
      scan = await invoke("storage_scan", {
        depth: "quick",
        roots: [state.selectedDrive],
        find_duplicates: false,
        find_abandoned: true,
      });
      state.lastScan = scan;
      if (els.progressBar) els.progressBar.style.width = "100%";
      renderHighestConsumers(scan);
      loadLeftovers(scan);
    } catch (err) {
      console.error("Storage scan error:", err);
      if (els.progressPhase) els.progressPhase.textContent = "Scan stopped: " + err;
    } finally {
      state.isScanning = false;
      if (els.scanBtn) els.scanBtn.disabled = false;
      if (els.cancelBtn) els.cancelBtn.classList.add("hidden");
      setTimeout(function () {
        if (els.progressWrap) els.progressWrap.classList.add("hidden");
        if (window.CureCat && els.catComp) {
          window.CureCat.setState(els.catComp, "idle");
        }
      }, 1200);
    }
  }

  function renderHighestConsumers(scan) {
    if (!els.consumersBody || !scan) return;
    els.consumersBody.textContent = "";

    const folders = (scan.large_folders || []).filter(function (f) {
      const p = (f.path || "").toLowerCase();
      // Filter root windows and program files containers
      return !(p.endsWith(":\\windows") || p.endsWith(":\\windows\\") || p === "c:\\" || p === "d:\\");
    });

    const files = (scan.large_files || scan.files || []).slice(0, 30);

    // Combine and sort descending by allocated size
    const items = [];
    folders.forEach(function (f) {
      items.push({
        isFolder: true,
        id: f.id,
        path: f.path,
        name: f.name || f.path,
        bytes: f.size_allocated || f.size_logical || 0,
        tier: f.tier,
      });
    });
    files.forEach(function (fi) {
      items.push({
        isFolder: false,
        id: fi.path,
        path: fi.path,
        name: fi.name || fi.path,
        bytes: fi.size_allocated || fi.size_logical || 0,
        tier: fi.tier,
      });
    });

    items.sort(function (a, b) {
      return (b.bytes || 0) - (a.bytes || 0);
    });

    const displayItems = items.slice(0, 35);

    if (els.consumersBadge) {
      els.consumersBadge.textContent = displayItems.length + " heavy items";
    }

    if (!displayItems.length) {
      const tr = document.createElement("tr");
      const td = document.createElement("td");
      td.colSpan = 3;
      td.className = "sm-empty-cell";
      td.textContent = "No large files or folders found on " + state.selectedDrive;
      tr.appendChild(td);
      els.consumersBody.appendChild(tr);
      return;
    }

    const maxBytes = displayItems[0].bytes || 1;

    displayItems.forEach(function (item) {
      const tr = document.createElement("tr");

      // Column 1: Name & Path
      const tdName = document.createElement("td");
      const wrap = document.createElement("div");
      wrap.className = "sm-folder-item";

      const icon = document.createElement("span");
      icon.className = "sm-folder-icon";
      icon.textContent = item.isFolder ? "📁" : "📄";

      const meta = document.createElement("div");
      meta.className = "sm-folder-meta";

      const name = document.createElement("span");
      name.className = "sm-folder-name";
      name.textContent = item.name;

      const path = document.createElement("span");
      path.className = "sm-folder-path";
      path.textContent = item.path;
      path.title = item.path;

      meta.append(name, path);
      wrap.append(icon, meta);
      tdName.appendChild(wrap);

      // Column 2: Space Bar + Bold Size
      const tdSize = document.createElement("td");
      const sizeCell = document.createElement("div");
      sizeCell.className = "sm-size-cell";

      const sizeVal = document.createElement("span");
      sizeVal.className = "sm-size-val";
      sizeVal.textContent = fmtBytes(item.bytes);

      const track = document.createElement("div");
      track.className = "sm-capacity-bar-track";
      const fill = document.createElement("div");
      fill.className = "sm-capacity-bar-fill";
      const pct = Math.min(100, Math.max(3, (item.bytes / maxBytes) * 100));
      fill.style.width = pct.toFixed(1) + "%";
      track.appendChild(fill);

      sizeCell.append(sizeVal, track);
      tdSize.appendChild(sizeCell);

      // Column 3: ONLY 2 DIRECT BUTTONS: Move to Drive... and Delete
      const tdActions = document.createElement("td");
      tdActions.className = "sm-actions-cell";

      const moveBtn = document.createElement("button");
      moveBtn.className = "sm-action-btn-move";
      moveBtn.textContent = "Move to Drive…";
      moveBtn.title = "Move this item to D: or an external drive";
      moveBtn.addEventListener("click", function () {
        if (window.CureMoveBot && window.CureMoveBot.start) {
          window.CureMoveBot.start([item.id || item.path]);
        }
      });

      const delBtn = document.createElement("button");
      delBtn.className = "sm-action-btn-delete";
      delBtn.textContent = "Delete";
      delBtn.title = "Safely remove to Recycle Bin after confirmation";
      delBtn.addEventListener("click", function () {
        handleDeleteItem(item, tr);
      });

      tdActions.append(moveBtn, delBtn);

      tr.append(tdName, tdSize, tdActions);
      els.consumersBody.appendChild(tr);
    });
  }

  function handleDeleteItem(item, rowEl) {
    const confirmed = window.confirm(
      "Safely remove \"" + item.name + "\" (" + fmtBytes(item.bytes) + ")?\n\nThis will safely move it to the Windows Recycle Bin / quarantine so you can restore it if ever needed."
    );
    if (!confirmed) return;

    if (window.CureCat && els.catComp) {
      window.CureCat.swat(els.catComp);
    }

    invoke("storage_remove_leftover", { path: item.path })
      .then(function () {
        rowEl.style.opacity = "0.4";
        rowEl.style.textDecoration = "line-through";
        const act = rowEl.querySelector(".sm-actions-cell");
        if (act) act.textContent = "Moved to Recycle Bin ✓";
      })
      .catch(function (err) {
        alert("Removal notice: " + err);
      });
  }

  async function loadLeftovers(scan) {
    if (!els.leftoversList) return;
    let list = (scan && scan.abandoned) || [];

    if (!list.length) {
      try {
        const res = await invoke("list_uninstalled_leftovers");
        list = res || [];
      } catch (e) {
        console.warn("Could not list leftovers:", e);
      }
    }

    state.leftovers = list;
    renderLeftovers();
  }

  function renderLeftovers() {
    if (!els.leftoversList) return;
    els.leftoversList.textContent = "";

    if (!state.leftovers.length) {
      const empty = document.createElement("div");
      empty.className = "sm-leftovers-empty";
      empty.textContent = "No uninstalled app leftovers detected on this PC. System is clean!";
      els.leftoversList.appendChild(empty);
      return;
    }

    state.leftovers.forEach(function (lo) {
      const card = document.createElement("div");
      card.className = "sm-leftover-card";

      const left = document.createElement("div");
      left.className = "sm-leftover-left";

      const name = document.createElement("span");
      name.className = "sm-leftover-name";
      name.textContent = lo.name || lo.app_name || "Uninstalled App Leftover";

      const path = document.createElement("span");
      path.className = "sm-leftover-path";
      path.textContent = lo.path;
      path.title = lo.path;

      const size = document.createElement("span");
      size.className = "sm-leftover-size";
      size.textContent = lo.size_formatted || fmtBytes(lo.size_allocated || lo.size_bytes);

      left.append(name, path, size);

      const removeBtn = document.createElement("button");
      removeBtn.className = "sm-leftover-remove-btn";
      removeBtn.textContent = "Remove Leftover";
      removeBtn.addEventListener("click", function () {
        const confirmed = window.confirm(
          "Remove leftover folder for \"" + (lo.name || lo.app_name) + "\"?\n\nPath: " + lo.path + "\n\nFiles will be safely sent to the Windows Recycle Bin."
        );
        if (!confirmed) return;

        if (window.CureCat && els.catComp) {
          window.CureCat.swat(els.catComp);
        }

        invoke("storage_remove_leftover", { path: lo.path })
          .then(function () {
            card.remove();
            if (!els.leftoversList.children.length) {
              renderLeftovers();
            }
          })
          .catch(function (err) {
            alert("Could not remove leftover: " + err);
          });
      });

      card.append(left, removeBtn);
      els.leftoversList.appendChild(card);
    });
  }

  function wire() {
    cacheEls();
    loadDrives();

    if (window.CureCat && els.catComp) {
      window.CureCat.render(els.catComp, "idle");
    }

    if (els.scanBtn) {
      els.scanBtn.addEventListener("click", startDriveScan);
    }

    if (els.rescanLeftovers) {
      els.rescanLeftovers.addEventListener("click", function () {
        loadLeftovers(null);
      });
    }

    // Load initial leftovers
    loadLeftovers(null);
  }

  window.CureStorageClean = {
    init: wire,
    startDriveScan: startDriveScan,
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", wire);
  } else {
    wire();
  }
})();
