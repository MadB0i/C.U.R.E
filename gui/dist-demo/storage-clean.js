/**
 * C.U.R.E — Page 3: Storage & Move Clean Controller
 * 1-Click Drive Scanner, True Highest Space Consumers, Fast Move/Delete & Leftovers
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
    consumers: [],
    leftovers: [],
  };

  const els = {};

  function cacheEls() {
    els.catComp = document.getElementById("sm-cat-companion");
    els.drivesGrid = document.getElementById("sm-drives-grid");
    els.scanBtn = document.getElementById("sm-scan-now");
    els.scanBtnText = document.getElementById("sm-scan-btn-text");
    els.cancelBtn = document.getElementById("sm-scan-cancel");
    els.progressBar = document.getElementById("sm-progress-bar");
    els.phaseTicker = document.getElementById("sm-phase-ticker");

    // KPI Cards
    els.kpiCapacityVal = document.getElementById("sm-kpi-capacity-val");
    els.kpiCapacitySub = document.getElementById("sm-kpi-capacity-sub");
    els.kpiConsumersVal = document.getElementById("sm-kpi-consumers-val");
    els.kpiConsumersSub = document.getElementById("sm-kpi-consumers-sub");
    els.kpiLeftoversVal = document.getElementById("sm-kpi-leftovers-val");
    els.kpiLeftoversSub = document.getElementById("sm-kpi-leftovers-sub");

    // Highest Consumers
    els.consumersBadge = document.getElementById("sm-consumers-badge");
    els.consumersIdle = document.getElementById("sm-consumers-idle");
    els.consumersList = document.getElementById("sm-consumers-list");

    // Leftovers
    els.dropTestLeftover = document.getElementById("sm-drop-test-leftover");
    els.rescanLeftovers = document.getElementById("sm-rescan-leftovers");
    els.leftoversList = document.getElementById("sm-leftovers-list");
  }

  async function loadDrives() {
    try {
      const roots = await invoke("storage_available_roots", {});
      const seenLetters = new Set();
      const drives = (roots || []).filter(function (r) {
        const letter = (r.path || "").substring(0, 2).toUpperCase();
        if (!letter.endsWith(":") || seenLetters.has(letter)) return false;
        seenLetters.add(letter);
        return r.is_drive || (r.path && r.path.length <= 3 && r.path.includes(":"));
      });
      state.drives = drives.length ? drives : [
        { path: "C:\\", label: "Drive C", is_drive: true, total_bytes: 512 * 1024 * 1024 * 1024, free_bytes: 64 * 1024 * 1024 * 1024 }
      ];
      renderDrives();
      updateCapacityKPI();
    } catch (e) {
      console.warn("Failed to load drives via storage_available_roots:", e);
      state.drives = [
        { path: "C:\\", label: "Drive C", is_drive: true, total_bytes: 512 * 1024 * 1024 * 1024, free_bytes: 64 * 1024 * 1024 * 1024 }
      ];
      renderDrives();
      updateCapacityKPI();
    }
  }

  function renderDrives() {
    if (!els.drivesGrid) return;
    els.drivesGrid.textContent = "";

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
      tag.textContent = isSys ? "System SSD" : (drive.label || "Data Drive");

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
        updateCapacityKPI();
      });

      els.drivesGrid.appendChild(card);
    });
  }

  function updateCapacityKPI() {
    const sel = state.drives.find((d) => d.path === state.selectedDrive) || state.drives[0];
    if (sel && els.kpiCapacityVal && els.kpiCapacitySub) {
      const total = sel.total_bytes || 0;
      const free = sel.free_bytes || 0;
      const used = Math.max(0, total - free);
      els.kpiCapacityVal.textContent = sel.path + " · " + fmtBytes(free) + " Free";
      els.kpiCapacitySub.textContent = fmtBytes(used) + " used of " + fmtBytes(total);
    }
  }

  async function startDriveScan() {
    if (state.isScanning) return;
    state.isScanning = true;

    if (els.scanBtn) els.scanBtn.disabled = true;
    if (els.cancelBtn) els.cancelBtn.classList.remove("hidden");
    if (els.progressBar) els.progressBar.style.width = "15%";
    if (els.phaseTicker) els.phaseTicker.textContent = "Scanning " + state.selectedDrive + " — inspecting large directories & caches…";

    if (window.CureCat && els.catComp) {
      window.CureCat.setState(els.catComp, "scanning");
    }

    // Set ticker animation stages
    let tickerStep = 0;
    const tickerInterval = setInterval(function () {
      if (!state.isScanning) {
        clearInterval(tickerInterval);
        return;
      }
      tickerStep++;
      if (tickerStep === 1) {
        if (els.progressBar) els.progressBar.style.width = "40%";
        if (els.phaseTicker) els.phaseTicker.textContent = "Measuring developer toolchains (.codex, .gradle, .android, .rustup)…";
      } else if (tickerStep === 2) {
        if (els.progressBar) els.progressBar.style.width = "70%";
        if (els.phaseTicker) els.phaseTicker.textContent = "Scanning Downloads, AppData caches and large standalone files…";
      } else if (tickerStep === 3) {
        if (els.progressBar) els.progressBar.style.width = "90%";
        if (els.phaseTicker) els.phaseTicker.textContent = "Checking registry against installed programs for orphaned leftovers…";
      }
    }, 600);

    let scan = null;
    try {
      scan = await invoke("storage_scan", {
        depth: "quick",
        roots: [state.selectedDrive],
        findDuplicates: false,
        findAbandoned: false,
      });
      state.lastScan = scan;
      if (els.progressBar) els.progressBar.style.width = "100%";
      if (els.phaseTicker) els.phaseTicker.textContent = "Scan complete — highest space consumers and leftovers updated!";
      
      renderHighestConsumers(scan);
      await loadLeftovers(scan);
      await loadDrives(); // Refresh drive free space figures
    } catch (err) {
      console.error("Storage scan error:", err);
      if (els.phaseTicker) els.phaseTicker.textContent = "Scan stopped: " + err;
    } finally {
      state.isScanning = false;
      clearInterval(tickerInterval);
      if (els.scanBtn) els.scanBtn.disabled = false;
      if (els.cancelBtn) els.cancelBtn.classList.add("hidden");
      setTimeout(function () {
        if (window.CureCat && els.catComp) {
          window.CureCat.setState(els.catComp, "idle");
        }
      }, 1200);
    }
  }

  function renderHighestConsumers(scan) {
    if (!scan) return;

    const folders = (scan.large_folders || []).filter(function (f) {
      const p = (f.path || "").toLowerCase().replace(/\\/g, "/");
      return !(p === "c:" || p === "c:/" || p === "d:" || p === "d:/" || p.endsWith("/windows") || p.endsWith("/system32"));
    });

    const files = (scan.large_files || scan.files || []).slice(0, 40);

    const items = [];
    folders.forEach(function (f) {
      items.push({
        isFolder: true,
        id: f.id,
        path: f.path,
        name: f.name || f.path.split(/[\\/]/).pop(),
        bytes: f.size_allocated || f.size_logical || 0,
      });
    });

    files.forEach(function (fi) {
      items.push({
        isFolder: false,
        id: fi.path,
        path: fi.path,
        name: fi.name || fi.path.split(/[\\/]/).pop(),
        bytes: fi.size_allocated || fi.size_logical || 0,
      });
    });

    // Sort strictly descending by size
    items.sort(function (a, b) {
      return (b.bytes || 0) - (a.bytes || 0);
    });

    state.consumers = items.slice(0, 45);

    if (els.consumersIdle) els.consumersIdle.classList.add("hidden");
    if (els.consumersList) els.consumersList.classList.remove("hidden");
    if (els.consumersList) els.consumersList.textContent = "";

    if (!state.consumers.length) {
      if (els.consumersBadge) els.consumersBadge.textContent = "0 items";
      if (els.kpiConsumersVal) els.kpiConsumersVal.textContent = "0 GB";
      if (els.kpiConsumersSub) els.kpiConsumersSub.textContent = "0 heavy items";
      const empty = document.createElement("div");
      empty.className = "sm-leftovers-empty";
      empty.textContent = "No large folders or files detected on " + state.selectedDrive;
      els.consumersList.appendChild(empty);
      return;
    }

    // Update KPI
    updateConsumersKPI();

    const maxBytes = state.consumers[0].bytes || 1;

    state.consumers.forEach(function (item) {
      const row = document.createElement("div");
      row.className = "sm-consumer-item";

      // Left: Badge + Name & Path
      const left = document.createElement("div");
      left.className = "sm-consumer-left";

      const badge = document.createElement("span");
      badge.className = "sm-badge-type " + (item.isFolder ? "sm-badge-folder" : "sm-badge-file");
      badge.textContent = item.isFolder ? "FOLDER" : "FILE";

      const meta = document.createElement("div");
      meta.className = "sm-consumer-meta";

      const name = document.createElement("span");
      name.className = "sm-consumer-name";
      name.textContent = item.name;

      const path = document.createElement("span");
      path.className = "sm-consumer-path";
      path.textContent = item.path;
      path.title = item.path;

      meta.append(name, path);
      left.append(badge, meta);

      // Center: Capacity bar + Size
      const center = document.createElement("div");
      center.className = "sm-consumer-center";

      const track = document.createElement("div");
      track.className = "sm-consumer-bar-track";
      const fill = document.createElement("div");
      fill.className = "sm-consumer-bar-fill";
      const pct = Math.min(100, Math.max(4, (item.bytes / maxBytes) * 100));
      fill.style.width = pct.toFixed(1) + "%";
      track.appendChild(fill);

      const size = document.createElement("span");
      size.className = "sm-consumer-size";
      size.textContent = fmtBytes(item.bytes);

      center.append(track, size);

      // Right: 2 Direct Actions: Move to Drive... and Delete
      const actions = document.createElement("div");
      actions.className = "sm-consumer-actions";

      const moveBtn = document.createElement("button");
      moveBtn.className = "sm-btn-move";
      moveBtn.type = "button";
      moveBtn.innerHTML = `
        <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="13 17 18 12 13 7"></polyline>
          <polyline points="6 17 11 12 6 7"></polyline>
        </svg>
        <span>Move to Drive…</span>
      `;
      moveBtn.title = "Move this item to D: or another drive with directory junction";
      moveBtn.addEventListener("click", function () {
        if (window.CureCat && els.catComp) {
          window.CureCat.setState(els.catComp, "moving");
        }
        if (window.CureMoveBot && window.CureMoveBot.start) {
          window.CureMoveBot.start([item.id || item.path]);
        }
      });

      const delBtn = document.createElement("button");
      delBtn.className = "sm-btn-delete";
      delBtn.type = "button";
      delBtn.innerHTML = `
        <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="3 6 5 6 21 6"></polyline>
          <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
        </svg>
        <span>Delete</span>
      `;
      delBtn.title = "Permanently move to Windows Recycle Bin";
      delBtn.addEventListener("click", function () {
        handleDeleteItem(item, row);
      });

      actions.append(moveBtn, delBtn);

      row.append(left, center, actions);
      els.consumersList.appendChild(row);
    });
  }

  function updateConsumersKPI() {
    let totalBytes = 0;
    state.consumers.forEach(function (c) {
      totalBytes += c.bytes || 0;
    });

    if (els.consumersBadge) {
      els.consumersBadge.textContent = state.consumers.length + " heavy items";
    }
    if (els.kpiConsumersVal) {
      els.kpiConsumersVal.textContent = fmtBytes(totalBytes);
    }
    if (els.kpiConsumersSub) {
      els.kpiConsumersSub.textContent = state.consumers.length + " heavy items found";
    }
  }

  async function handleDeleteItem(item, rowEl) {
    const confirmed = window.confirm(
      'Safely remove "' + item.name + '" (' + fmtBytes(item.bytes) + ')?\n\nThis will safely move it to the Windows Recycle Bin so you can restore it if ever needed.'
    );
    if (!confirmed) return;

    if (window.CureCat && els.catComp) {
      window.CureCat.swat(els.catComp);
    }

    try {
      await invoke("storage_remove_leftover", { path: item.path });
      rowEl.style.transition = "all 0.4s ease";
      rowEl.style.opacity = "0";
      rowEl.style.transform = "translateX(50px)";
      setTimeout(function () {
        rowEl.remove();
        state.consumers = state.consumers.filter(function (c) {
          return c.path !== item.path;
        });
        updateConsumersKPI();
        loadDrives(); // Update drive free space
      }, 400);
    } catch (err) {
      alert("Delete notice: " + err);
    }
  }

  async function loadLeftovers(scan) {
    if (!els.leftoversList) return;
    let list = (scan && scan.abandoned) || [];

    if (!list.length) {
      try {
        const res = await invoke("list_uninstalled_leftovers");
        list = res || [];
      } catch (e) {
        console.warn("Could not list uninstalled leftovers:", e);
      }
    }

    state.leftovers = list;
    renderLeftovers();
  }

  function renderLeftovers() {
    if (!els.leftoversList) return;
    els.leftoversList.textContent = "";

    // Update KPI
    let totalLeftoverBytes = 0;
    state.leftovers.forEach(function (lo) {
      totalLeftoverBytes += lo.size_allocated || lo.size_bytes || 0;
    });

    if (els.kpiLeftoversVal) {
      els.kpiLeftoversVal.textContent = state.leftovers.length + " folders";
    }
    if (els.kpiLeftoversSub) {
      els.kpiLeftoversSub.textContent = fmtBytes(totalLeftoverBytes) + " reclaimable";
    }

    if (!state.leftovers.length) {
      const empty = document.createElement("div");
      empty.className = "sm-leftovers-empty";
      empty.textContent = "No uninstalled app leftovers detected on this PC. System is clean!";
      els.leftoversList.appendChild(empty);
      return;
    }

    state.leftovers.forEach(function (lo) {
      const card = document.createElement("div");
      card.className = "sm-leftover-item";

      const left = document.createElement("div");
      left.className = "sm-leftover-left";

      const titleRow = document.createElement("div");
      titleRow.className = "sm-leftover-title-row";

      const name = document.createElement("span");
      name.className = "sm-leftover-name";
      name.textContent = lo.app_name || lo.name || "Uninstalled App Leftover";

      const pill = document.createElement("span");
      pill.className = "sm-badge-ghost-pill";
      pill.textContent = "GHOST LEFTOVER";

      titleRow.append(name, pill);

      const path = document.createElement("span");
      path.className = "sm-leftover-path";
      path.textContent = lo.path;
      path.title = lo.path;

      left.append(titleRow, path);

      const right = document.createElement("div");
      right.className = "sm-leftover-right";

      const size = document.createElement("span");
      size.className = "sm-leftover-size";
      size.textContent = fmtBytes(lo.size_allocated || lo.size_bytes);

      const removeBtn = document.createElement("button");
      removeBtn.className = "sm-btn-remove-leftover";
      removeBtn.type = "button";
      removeBtn.innerHTML = `
        <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="3 6 5 6 21 6"></polyline>
          <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
        </svg>
        <span>Remove Leftover</span>
      `;
      removeBtn.title = "Safely move leftover folder to Recycle Bin";
      removeBtn.addEventListener("click", function () {
        handleRemoveLeftover(lo, card);
      });

      right.append(size, removeBtn);
      card.append(left, right);
      els.leftoversList.appendChild(card);
    });
  }

  async function handleRemoveLeftover(lo, cardEl) {
    const confirmed = window.confirm(
      'Remove leftover folder for "' + (lo.app_name || lo.name) + '"?\n\nPath: ' + lo.path + '\n\nFiles will be safely sent to the Windows Recycle Bin.'
    );
    if (!confirmed) return;

    if (window.CureCat && els.catComp) {
      window.CureCat.swat(els.catComp);
    }

    try {
      await invoke("storage_remove_leftover", { path: lo.path });
      cardEl.style.transition = "all 0.35s ease";
      cardEl.style.opacity = "0";
      cardEl.style.transform = "translateX(40px)";
      setTimeout(function () {
        cardEl.remove();
        state.leftovers = state.leftovers.filter(function (l) {
          return l.path !== lo.path;
        });
        renderLeftovers();
      }, 350);
    } catch (err) {
      alert("Could not remove leftover: " + err);
    }
  }

  async function handleDropTestLeftover() {
    try {
      if (els.dropTestLeftover) els.dropTestLeftover.disabled = true;
      const createdPath = await invoke("drop_test_leftover");
      if (els.phaseTicker) {
        els.phaseTicker.textContent = "Test leftover created: " + createdPath + " (2 MB dummy cache)";
      }
      await loadLeftovers(null);
    } catch (err) {
      alert("Failed to drop test leftover: " + err);
    } finally {
      if (els.dropTestLeftover) els.dropTestLeftover.disabled = false;
    }
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

    if (els.dropTestLeftover) {
      els.dropTestLeftover.addEventListener("click", handleDropTestLeftover);
    }

    if (els.rescanLeftovers) {
      els.rescanLeftovers.addEventListener("click", function () {
        loadLeftovers(null);
      });
    }

    // Initial check for uninstalled leftovers
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
