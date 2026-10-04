/* ==========================================================================
   SEGMENTED STORAGE CORE — Disk Cleanup visualization.
   One arc per real cleanup category reported by scan_cleanup. Arc length is
   proportional to the engine's own byte totals; nothing is sized for effect.
   Before measurement the arcs stay unmeasured and say so.
   ========================================================================== */
(function () {
  "use strict";

  /* The engine's category keys. Labels shown pre-measurement describe the
     sources that will be checked, never a measured amount. */
  const SLOTS = [
    { key: "temp", label: "Temporary files" },
    { key: "browser_cache", label: "Browser caches" },
    { key: "recycle_bin", label: "Recycle Bin" },
    { key: "windows_old", label: "Windows.old" },
  ];

  const DOWNLOAD_SLOT = {
    key: "old_installers",
    label: "Old installers in Downloads",
  };

  const canvas = document.getElementById("storage-canvas");
  const wrap = document.getElementById("storage-core");
  const listEl = document.getElementById("storage-legend");
  const headlineEl = document.getElementById("storage-reclaimable");
  const subEl = document.getElementById("storage-caption");
  const measureEl = document.getElementById("storage-measure");
  const freedEl = document.getElementById("storage-freed-wrap");

  const css = getComputedStyle(document.documentElement);
  const token = (name, fallback) =>
    (css.getPropertyValue(name) || "").trim() || fallback;
  const C = {
    key: token("--cure-400", "#a78bfa"),
    keyLit: token("--cure-300", "#c4b1ff"),
    ok: token("--ok", "#63c79a"),
    warn: token("--warning", "#e0a458"),
    text: token("--text", "#e9ecf1"),
    text2: token("--text-2", "#a9b1bd"),
    text3: token("--text-3", "#767f8c"),
    hair: "rgba(255,255,255,0.08)",
    hairSoft: "rgba(255,255,255,0.04)",
  };

  const view = {
    measured: false,
    running: false,
    /* before/after are both real measurements returned by the engine. */
    before: null,
    after: null,
    baseline: 0,
    selected: new Set(),
    result: null,
    spin: 0,
  };

  const geo = { w: 0, h: 0, cx: 0, cy: 0, r: 0, dpr: 1 };
  let rafId = null;
  let lastFrame = 0;
  let motionOff = false;

  function readStore(key) {
    try {
      return localStorage.getItem(key);
    } catch (_) {
      return null;
    }
  }
  function store(key, value) {
    try {
      localStorage.setItem(key, value);
    } catch (_) {
      /* optional */
    }
  }

  const reducedMedia = matchMedia("(prefers-reduced-motion: reduce)");
  motionOff = readStore("cure-motion-paused") === "true";

  function fmtBytes(n) {
    if (n === null || n === undefined) return "—";
    if (n >= 1073741824) return (n / 1073741824).toFixed(1) + " GB";
    if (n >= 1048576) return (n / 1048576).toFixed(1) + " MB";
    if (n >= 1024) return Math.round(n / 1024) + " KB";
    return n + " B";
  }

  /* ---------- data projection ------------------------------------------- */

  function project(summary) {
    const rows = [];
    if (!summary) {
      for (let i = 0; i < SLOTS.length; i += 1) {
        rows.push({
          key: SLOTS[i].key,
          label: SLOTS[i].label,
          bytes: null,
          items: null,
          selected: false,
        });
      }
      rows.push({
        key: DOWNLOAD_SLOT.key,
        label: DOWNLOAD_SLOT.label,
        bytes: null,
        items: null,
        selected: false,
      });
      return rows;
    }
    for (let i = 0; i < SLOTS.length; i += 1) {
      const cat =
        (summary.categories || []).find(function (c) {
          return c.key === SLOTS[i].key;
        }) || null;
      rows.push({
        key: SLOTS[i].key,
        label: cat ? cat.label : SLOTS[i].label,
        bytes: cat ? cat.total_bytes : 0,
        items: cat ? cat.item_count : 0,
        selected: !!(cat && cat.item_count > 0 && view.selected.has(cat.key)),
      });
    }
    const dlBytes = (summary.downloads || []).reduce(function (s, d) {
      return s + d.size_bytes;
    }, 0);
    rows.push({
      key: DOWNLOAD_SLOT.key,
      label: DOWNLOAD_SLOT.label,
      bytes: dlBytes,
      items: (summary.downloads || []).length,
      selected: view.checkedDownloads
        ? view.checkedDownloads.size > 0
        : false,
    });
    return rows;
  }

  let current = project(null);

  function totalBytes(rows) {
    let sum = 0;
    for (let i = 0; i < rows.length; i += 1) {
      if (rows[i].bytes) sum += rows[i].bytes;
    }
    return sum;
  }

  function totalItems(rows) {
    let sum = 0;
    for (let i = 0; i < rows.length; i += 1) {
      if (rows[i].items) sum += rows[i].items;
    }
    return sum;
  }

  /* ---------- painting ---------------------------------------------------- */

  function roundRect(ctx, x, y, w, h, r) {
    const radius = Math.min(r, w / 2, h / 2);
    ctx.beginPath();
    ctx.moveTo(x + radius, y);
    ctx.arcTo(x + w, y, x + w, y + h, radius);
    ctx.arcTo(x + w, y + h, x, y + h, radius);
    ctx.arcTo(x, y + h, x, y, radius);
    ctx.arcTo(x, y, x + w, y, radius);
    ctx.closePath();
  }

  function layout() {
    if (!canvas || !wrap) return;
    const rect = wrap.getBoundingClientRect();
    /* Hidden views report a zero box; nothing is drawn rather than a
       degenerate ring. */
    if (rect.width < 60 || rect.height < 60) return;
    geo.w = Math.round(rect.width);
    geo.h = Math.round(rect.height);
    geo.dpr = Math.min(2, window.devicePixelRatio || 1);
    canvas.width = Math.round(geo.w * geo.dpr);
    canvas.height = Math.round(geo.h * geo.dpr);
    geo.cx = geo.w / 2;
    geo.cy = geo.h / 2;
    geo.r = Math.max(38, Math.min(geo.w, geo.h) / 2 - 24);
    draw();
  }

  function draw() {
    if (!canvas || !geo.r) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(geo.dpr, 0, 0, geo.dpr, 0, 0);
    ctx.clearRect(0, 0, geo.w, geo.h);

    const rows = current;
    const measured = view.measured;
    const total = measured ? totalBytes(rows) : 0;
    const freed = view.result ? view.result.bytes_freed : 0;

    /* ring well */
    ctx.strokeStyle = "rgba(255,255,255,0.05)";
    ctx.lineWidth = Math.min(22, geo.r * 0.42);
    ctx.beginPath();
    ctx.arc(geo.cx, geo.cy, geo.r, 0, Math.PI * 2);
    ctx.stroke();

    const band = Math.min(22, geo.r * 0.42);
    const gap = 0.05;
    const start = -Math.PI / 2 + view.spin;
    let cursor = start;

    for (let i = 0; i < rows.length; i += 1) {
      const row = rows[i];
      const span = measured && total > 0 && row.bytes > 0 ? (row.bytes / total) * (Math.PI * 2 - gap * rows.length) : 0;
      if (span > 0) {
        ctx.lineWidth = row.selected ? band : Math.max(5, band * 0.6);
        ctx.lineCap = "butt";
        ctx.strokeStyle = row.selected
          ? "rgba(167,139,250,0.92)"
          : "rgba(167,139,250,0.26)";
        ctx.beginPath();
        ctx.arc(geo.cx, geo.cy, geo.r, cursor, cursor + span);
        ctx.stroke();
        if (row.selected) {
          ctx.strokeStyle = "rgba(214,203,255,0.9)";
          ctx.lineWidth = 2;
          ctx.beginPath();
          ctx.arc(geo.cx, geo.cy, geo.r + 12, cursor, cursor + span);
          ctx.stroke();
        }
      } else if (measured) {
        /* a measured-but-empty category still gets a visible marker */
        ctx.strokeStyle = "rgba(255,255,255,0.09)";
        ctx.lineWidth = 6;
        ctx.beginPath();
        ctx.arc(geo.cx, geo.cy, geo.r, cursor, cursor + 0.03);
        ctx.stroke();
      }
      cursor += span + gap;
    }

    /* reclaimed arc: the engine's real bytes_freed against the real total
       measured at the moment the operator confirmed the selection */
    if (freed > 0 && view.baseline > 0) {
      const span = Math.max(
        0.05,
        Math.min(Math.PI * 2 - 0.2, (freed / view.baseline) * Math.PI * 1.4),
      );
      ctx.strokeStyle = "rgba(99,199,154,0.95)";
      ctx.lineWidth = 5;
      ctx.lineCap = "round";
      ctx.beginPath();
      ctx.arc(geo.cx, geo.cy, geo.r + band * 0.5 + 8, start, start + span);
      ctx.stroke();
    }

    /* hub */
    ctx.fillStyle = "rgba(255,255,255,0.022)";
    ctx.beginPath();
    ctx.arc(geo.cx, geo.cy, Math.max(2, geo.r - band - 24), 0, Math.PI * 2);
    ctx.fill();

    ctx.textAlign = "center";
    ctx.textBaseline = "alphabetic";
    if ("letterSpacing" in ctx) ctx.letterSpacing = "0.16em";
    ctx.fillStyle = C.text3;
    ctx.font = '650 10px "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif';
    ctx.fillText(measured ? "RECLAIMABLE" : "NOT MEASURED", geo.cx, geo.cy - 6);
    if ("letterSpacing" in ctx) ctx.letterSpacing = "0px";

    ctx.fillStyle = measured ? C.text : C.text3;
    ctx.font = '620 25px "Cascadia Code", Consolas, monospace';
    ctx.fillText(
      measured ? fmtBytes(total) : "—",
      geo.cx,
      geo.cy + 20,
    );

    if (measured) {
      ctx.fillStyle = C.text3;
      ctx.font = '500 10px "Cascadia Code", Consolas, monospace';
      ctx.fillText(
        rows.filter(function (r) {
          return r.selected;
        }).length + " of " + rows.length + " selected",
        geo.cx,
        geo.cy + 38,
      );
    }
  }

  function frame(now) {
    rafId = null;
    const dt = Math.min(64, now - (lastFrame || now));
    lastFrame = now;
    /* The ring only turns while an operation is genuinely running. */
    if (view.running) view.spin = (view.spin + dt * 0.00042) % (Math.PI * 2);
    draw();
    if (view.running && !motionOff) rafId = requestAnimationFrame(frame);
  }

  function startLoop() {
    if (rafId !== null || motionOff) {
      draw();
      return;
    }
    lastFrame = performance.now();
    rafId = requestAnimationFrame(frame);
  }

  function stopLoop() {
    if (rafId !== null) cancelAnimationFrame(rafId);
    rafId = null;
  }

  /* ---------- legend ------------------------------------------------------ */

  function paintLegend() {
    if (!listEl) return;
    listEl.replaceChildren();
    const peak = (function () {
      let max = 0;
      for (let i = 0; i < current.length; i += 1) {
        if (current[i].bytes > max) max = current[i].bytes;
      }
      return Math.max(1, max);
    })();
    for (let i = 0; i < current.length; i += 1) {
      const row = current[i];
      const li = document.createElement("li");
      li.className = "st-row" + (row.selected ? " on" : "");
      const mark = document.createElement("span");
      mark.className = "st-mark";
      mark.setAttribute("aria-hidden", "true");
      const name = document.createElement("span");
      name.className = "st-name";
      name.textContent = row.label;
      /* A proportional bar so a wide ledger shows each category's real share
         of the measured total rather than empty space. */
      const bar = document.createElement("span");
      bar.className = "st-bar";
      bar.setAttribute("aria-hidden", "true");
      const fill = document.createElement("i");
      fill.style.width =
        (view.measured && row.bytes ? Math.max(2, (row.bytes / peak) * 100) : 0) + "%";
      bar.appendChild(fill);
      const value = document.createElement("span");
      value.className = "st-value";
      if (!view.measured) {
        value.textContent = "Not measured";
        li.classList.add("unmeasured");
      } else {
        const items =
          row.items === null
            ? ""
            : " · " + row.items + (row.items === 1 ? " item" : " items");
        value.textContent = fmtBytes(row.bytes) + items;
      }
      li.append(mark, name, bar, value);
      listEl.appendChild(li);
    }
    if (measureEl) measureEl.textContent = view.measured ? "MEASURED" : "NOT MEASURED";
    if (headlineEl) {
      headlineEl.textContent = view.measured
        ? fmtBytes(totalBytes(current))
        : "—";
    }
    if (subEl) {
      subEl.textContent = view.measured
        ? totalItems(current) +
          (totalItems(current) === 1 ? " item" : " items") +
          (view.result
            ? " remain · measured again by the engine"
            : " reported by the engine")
        : "Run a disk measurement — nothing is scanned or deleted until you start.";
    }
    if (freedEl) {
      freedEl.classList.toggle("hidden", !view.result);
      if (view.result) {
        freedEl.textContent =
          "Reclaimed " +
          fmtBytes(view.result.bytes_freed) +
          " · " +
          view.result.deleted +
          " of " +
          view.result.attempted +
          " items deleted" +
          (view.result.failed
            ? " · " + view.result.failed + " locked or failed"
            : "");
        freedEl.classList.toggle("is-warn", !!view.result.failed);
      }
    }
  }

  /* ---------- public API -------------------------------------------------- */

  window.CureStorage = {
    setDownloads: function (set) {
      view.checkedDownloads = set;
    },
    idle: function () {
      view.measured = false;
      view.running = false;
      view.before = null;
      view.after = null;
      view.baseline = 0;
      view.result = null;
      view.spin = 0;
      current = project(null);
      paintLegend();
      layout();
      stopLoop();
    },
    measuring: function () {
      view.measured = false;
      view.running = true;
      view.result = null;
      view.before = null;
      view.after = null;
      view.baseline = 0;
      view.spin = 0;
      current = project(null);
      paintLegend();
      startLoop();
    },
    measured: function (summary, selected, checkedDownloads) {
      view.measured = true;
      view.running = false;
      view.before = summary;
      view.baseline = totalBytes(project(summary));
      view.selected = selected instanceof Set ? selected : new Set(selected || []);
      view.checkedDownloads = checkedDownloads || new Set();
      current = project(summary);
      paintLegend();
      draw();
      stopLoop();
    },
    selectionChanged: function (selected, checkedDownloads) {
      view.selected = selected instanceof Set ? selected : new Set(selected || []);
      if (checkedDownloads) view.checkedDownloads = checkedDownloads;
      if (view.measured && view.before) current = project(view.before);
      paintLegend();
      draw();
    },
    running: function () {
      view.running = true;
      startLoop();
    },
    /* `after` is the engine's own rescan — never an estimate. */
    result: function (result, after, checkedDownloads) {
      view.result = result;
      view.after = after || null;
      view.running = false;
      view.checkedDownloads = checkedDownloads || new Set();
      if (after) {
        /* the ring now shows what is left; the freed bytes stay on the receipt */
        view.measured = true;
        current = project(after);
      }
      paintLegend();
      draw();
      stopLoop();
    },
    motion: function (paused) {
      motionOff = !!paused;
      store("cure-motion-paused", String(motionOff));
      if (motionOff) stopLoop();
      else if (view.running) startLoop();
    },
    redraw: layout,
  };

  if (wrap && "ResizeObserver" in window) {
    let queued = false;
    new ResizeObserver(function () {
      if (queued) return;
      queued = true;
      requestAnimationFrame(function () {
        queued = false;
        layout();
      });
    }).observe(wrap);
  } else {
    window.addEventListener("resize", layout);
  }

  document.addEventListener("visibilitychange", function () {
    if (document.hidden) stopLoop();
    else if (view.running) startLoop();
  });

  reducedMedia.addEventListener("change", function () {
    if (reducedMedia.matches) stopLoop();
    else if (view.running) startLoop();
  });

  paintLegend();
  layout();
})();