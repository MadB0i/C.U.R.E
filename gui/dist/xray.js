/* ==========================================================================
   SYSTEM X-RAY — signature scan visualization.
   Replaces the circular source radar. A sectioned machine chassis with a
   travelling inspection beam, live collector traces and resolved evidence
   nodes. Every pixel of state here comes from a real scan-progress event or
   a real scan summary. Nothing animates that no collector is driving, and no
   count, risk or coverage value is invented.
   Offline, canvas 2D, no external assets.
   ========================================================================== */
(function () {
  "use strict";

  /* ---------- the nine persistence layers -------------------------------- */
  /* `area` matches the backend coverage row label exactly. */
  /* `label` is the compact plate name; `full` is the precise collector name
     used in the accessible ledger beneath the field. */
  const LAYERS = [
    { id: "registry", label: "Registry", full: "Registry autoruns", area: "Registry autoruns" },
    { id: "startup", label: "Startup", full: "Startup folders", area: null },
    { id: "tasks", label: "Tasks", full: "Scheduled tasks", area: "Scheduled tasks" },
    { id: "services", label: "Services", full: "Auto-start services", area: "Services (auto-start)" },
    { id: "wmi", label: "WMI", full: "WMI subscriptions", area: "WMI subscriptions" },
    { id: "ifeo", label: "IFEO / AppInit", full: "IFEO debuggers and AppInit DLLs", area: null },
    { id: "com", label: "COM", full: "COM registrations", area: null },
    { id: "processes", label: "Processes", full: "Running processes", area: null },
    { id: "ransom", label: "Ransom", full: "Ransom indicators", area: null },
  ];

  const STAGE_ALIAS = {
    "startup-common": "startup",
    "process-scan": "processes",
    process: "processes",
    "ransom-scan": "ransom",
    "ransom-detect": "ransom",
  };

  const ITEM_SOURCE = {
    "registry-run": "registry",
    "startup-folder": "startup",
    "scheduled-task": "tasks",
    "windows-service": "services",
    "wmi-subscription": "wmi",
    "ifeo-debugger": "ifeo",
    "appinit-dlls": "ifeo",
    "com-hijack": "com",
  };

  /* Real coverage states, mapped without softening any of them. */
  function coverageTone(raw) {
    const key =
      typeof raw === "string" ? raw : Object.keys(raw || {})[0] || "NotChecked";
    if (key === "Checked" || key === "Available") return "checked";
    if (key === "CheckFailed") return "bad";
    if (key === "AccessDenied") return "warn";
    if (key === "Partial") return "partial";
    return "idle";
  }

  function coverageText(raw) {
    const key =
      typeof raw === "string" ? raw : Object.keys(raw || {})[0] || "NotChecked";
    if (key === "Checked") return "Checked";
    if (key === "Available") return "Available";
    if (key === "AccessDenied") return "Access denied";
    if (key === "CheckFailed") return "Check failed";
    if (key === "Partial") return "Partial";
    if (key === "Unavailable") return "Unavailable";
    if (key === "NotChecked") return "Not checked";
    return "Not checked";
  }

  /* ---------- DOM --------------------------------------------------------- */

  const canvas = document.getElementById("xray-canvas");
  const field = document.getElementById("xray-field");
  const sourceList = document.getElementById("sweep-sources");
  const itemsEl = document.getElementById("sweep-items");
  const currentEl = document.getElementById("sweep-current");
  const coreReadout = document.getElementById("xray-core-readout");
  const phaseEl = document.getElementById("xray-phase");
  const coverageEl = document.getElementById("xray-coverage");
  const inspectedEl = document.getElementById("xray-inspected");
  const findingsEl = document.getElementById("xray-findings");
  const elapsedEl = document.getElementById("scan-elapsed");
  const motionBtn = document.getElementById("xray-motion");

  const reducedMedia = matchMedia("(prefers-reduced-motion: reduce)");

  function store(key, value) {
    try {
      localStorage.setItem(key, value);
    } catch (_) {
      /* optional */
    }
  }
  function readStore(key) {
    try {
      return localStorage.getItem(key);
    } catch (_) {
      return null;
    }
  }

  let motionPaused = readStore("cure-motion-paused") === "true";

  function motionOff() {
    return motionPaused || reducedMedia.matches || document.hidden;
  }

  /* ---------- state ------------------------------------------------------- */

  const state = {
    layers: LAYERS.map(function (layer) {
      return {
        id: layer.id,
        label: layer.label,
        full: layer.full,
        tone: "pending",
        text: "Pending",
        count: 0,
        risk: 0,
        detail: "",
      };
    }),
    active: null,
    items: 0,
    findings: 0,
    running: false,
    trails: [],
    activity: 0,
  };

  function layerById(id) {
    for (let i = 0; i < state.layers.length; i += 1) {
      if (state.layers[i].id === id) return state.layers[i];
    }
    return null;
  }

  function indexOfActive() {
    for (let i = 0; i < state.layers.length; i += 1) {
      if (state.layers[i].id === state.active) return i;
    }
    return -1;
  }

  /* ---------- palette (read from the console tokens) ---------------------- */

  const css = getComputedStyle(document.documentElement);
  const token = (name, fallback) => {
    const value = css.getPropertyValue(name);
    return (value || "").trim() || fallback;
  };

  const C = {
    key: token("--cure-400", "#a78bfa"),
    keyLit: token("--cure-300", "#c4b1ff"),
    keyDeep: token("--cure-600", "#6d55d8"),
    ok: token("--ok", "#63c79a"),
    warn: token("--warning", "#e0a458"),
    bad: token("--danger", "#e4706a"),
    text: token("--text", "#e9ecf1"),
    text2: token("--text-2", "#a9b1bd"),
    text3: token("--text-3", "#767f8c"),
    hair: "rgba(255,255,255,0.075)",
    hairSoft: "rgba(255,255,255,0.04)",
    ink0: token("--ink-000", "#06070a"),
    ink1: token("--ink-100", "#0f1216"),
    ink2: token("--ink-200", "#191e25"),
  };

  function toneColor(tone) {
    if (tone === "active") return C.keyLit;
    if (tone === "checked") return C.ok;
    if (tone === "partial" || tone === "warn") return C.warn;
    if (tone === "bad") return C.bad;
    if (tone === "review" || tone === "risk") return C.warn;
    if (tone === "highrisk") return C.bad;
    return C.text3;
  }

  /* ---------- geometry ---------------------------------------------------- */

  const geo = { w: 0, h: 0, rail: 0, plates: [], die: null, dpr: 1 };
  let beamY = 0;
  let beamTarget = 0;
  let dashPhase = 0;
  let rafId = null;
  let lastFrame = 0;

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
    const rect = field.getBoundingClientRect();
    /* A hidden view reports a zero box. Drawing nothing is correct; drawing
       negative geometry is not. */
    if (rect.width < 2 || rect.height < 2) return;
    geo.w = Math.round(rect.width);
    geo.h = Math.round(rect.height);
    geo.dpr = Math.min(2, window.devicePixelRatio || 1);
    canvas.width = Math.round(geo.w * geo.dpr);
    canvas.height = Math.round(geo.h * geo.dpr);

    /* Bands reserved for the corner tags so they never sit on a layer. */
    geo.padTop = 26;
    geo.padBottom = 30;
    const usableTop = geo.padTop;
    const usableBottom = geo.h - geo.padBottom;
    const usableH = Math.max(120, usableBottom - usableTop);

    geo.rail = Math.round(
      Math.min(252, Math.max(168, Math.min(geo.w * 0.32, geo.w - 132))),
    );

    const gapY = Math.max(4, Math.min(11, usableH * 0.022));
    const plateH = Math.round(
      Math.max(15, Math.min(27, (usableH - 8 * gapY) / 9)),
    );
    const span = 9 * plateH + 8 * gapY;
    const firstTop = usableTop + (usableH - span) / 2;
    geo.plates = state.layers.map(function (layer, i) {
      const y = firstTop + i * (plateH + gapY);
      return { id: layer.id, x: 0, y: y, w: geo.rail, h: plateH, index: i };
    });
    geo.plateH = plateH;

    const areaX = geo.rail + 16;
    const areaW = Math.max(60, geo.w - areaX);
    const dieSize = Math.round(
      Math.max(56, Math.min(usableH * 0.82, areaW * 0.52, 360)),
    );
    const cy = usableTop + usableH / 2;
    geo.die = {
      cx: areaX + areaW / 2,
      cy: cy,
      size: dieSize,
      left: areaX + areaW / 2 - dieSize / 2,
      top: cy - dieSize / 2,
    };
    /* The board wraps the core with a working margin rather than filling the
       remaining width, so the composition reads as a machine, not a void. */
    const boardW = Math.min(areaW - 4, dieSize * 1.82);
    geo.board = {
      x: areaX + (areaW - boardW) / 2,
      y: usableTop - 6,
      w: boardW,
      h: Math.min(usableH + 12, dieSize * 1.5),
    };
    geo.board.y = cy - geo.board.h / 2;
    geo.midY = cy;

    /* The live readout is DOM so it stays selectable and announced; keep it
       aligned just under the die's own label. */
    if (coreReadout) {
      coreReadout.style.left = geo.die.cx + "px";
      coreReadout.style.top = geo.die.cy + geo.die.size * 0.29 + "px";
      coreReadout.style.width = Math.max(110, geo.die.size * 0.84) + "px";
      coreReadout.style.fontSize = Math.max(8, Math.round(geo.die.size * 0.045)) + "px";
    }

    beamTarget = plateCenterY(state.active);
    if (!state.running) beamY = beamTarget;
    draw();
  }

  function plateCenterY(id) {
    for (let i = 0; i < geo.plates.length; i += 1) {
      if (geo.plates[i].id === id) {
        return geo.plates[i].y + geo.plates[i].h / 2;
      }
    }
    return geo.die ? geo.die.cy : 0;
  }

  function maxIndexFor(id) {
    let max = 0;
    for (let i = 0; i < state.layers.length; i += 1) {
      if (state.layers[i].count > max) max = state.layers[i].count;
    }
    return Math.max(1, max);
  }

  /* Deterministic orthogonal route from a layer plate into the core. No
     randomness: the same collector always enters through the same pin. */
  function routeFor(index, plate) {
    const die = geo.die;
    if (!die) return null;
    const startX = plate.x + plate.w;
    const pinY = die.top + die.size * (0.14 + index * 0.09);
    const elbow = Math.min(
      die.left - 10,
      startX + 16 + ((index % 3) * 10),
    );
    return [
      { x: startX, y: plate.y + plate.h / 2 },
      { x: elbow, y: plate.y + plate.h / 2 },
      { x: elbow, y: pinY },
      { x: die.left, y: pinY },
    ];
  }

  function strokeRoute(ctx, points) {
    ctx.beginPath();
    ctx.moveTo(points[0].x, points[0].y);
    for (let p = 1; p < points.length; p += 1) {
      ctx.lineTo(points[p].x, points[p].y);
    }
    ctx.stroke();
  }

  function ellipsize(ctx, text, maxWidth) {
    if (ctx.measureText(text).width <= maxWidth) return text;
    let cut = text;
    while (cut.length > 3 && ctx.measureText(cut + "…").width > maxWidth) {
      cut = cut.slice(0, -1);
    }
    return cut + "…";
  }

  /* ---------- painting ---------------------------------------------------- */

  function drawChassis(ctx) {
    const die = geo.die;
    const board = geo.board;
    if (!die || !board) return;

    /* the board: the machine being inspected */
    ctx.fillStyle = "rgba(255,255,255,0.016)";
    roundRect(ctx, board.x, board.y, board.w, board.h, 14);
    ctx.fill();
    ctx.strokeStyle = C.hair;
    ctx.lineWidth = 1;
    roundRect(ctx, board.x, board.y, board.w, board.h, 14);
    ctx.stroke();
    ctx.strokeStyle = C.hairSoft;
    const nb = 12;
    const bn = [
      [board.x, board.y, 1, 1],
      [board.x + board.w, board.y, -1, 1],
      [board.x, board.y + board.h, 1, -1],
      [board.x + board.w, board.y + board.h, -1, -1],
    ];
    for (let c = 0; c < bn.length; c += 1) {
      const cx = bn[c][0];
      const cy2 = bn[c][1];
      ctx.beginPath();
      ctx.moveTo(cx + bn[c][2] * nb, cy2);
      ctx.lineTo(cx, cy2);
      ctx.lineTo(cx, cy2 + bn[c][3] * nb);
      ctx.stroke();
    }

    /* inner shells read as the die's casing, exploded a little */
    const shells = [1.24, 1.44];
    for (let s = 0; s < shells.length; s += 1) {
      const size = die.size * shells[s];
      ctx.strokeStyle = s === 0 ? C.hair : C.hairSoft;
      ctx.lineWidth = 1;
      roundRect(
        ctx,
        die.cx - size / 2,
        die.cy - size / 2,
        size,
        size,
        size * 0.17,
      );
      ctx.stroke();
    }

    /* Deterministic etched routing: the board is populated, and the pattern is
       identical on every run. Nothing here is random decoration. */
    const seed = 0x9e3779b9;
    let s0 = seed;
    const rand = function () {
      s0 = (s0 + 0x6d2b79f5) | 0;
      let t = s0;
      t = Math.imul(t ^ (t >>> 15), t | 1);
      t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
    ctx.strokeStyle = "rgba(255,255,255,0.055)";
    ctx.lineWidth = 1;
    for (let n = 0; n < 54; n += 1) {
      const px = board.x + 10 + rand() * (board.w - 20);
      const py = board.y + 10 + rand() * (board.h - 20);
      if (
        px > die.left - 14 &&
        px < die.left + die.size + 14 &&
        py > die.top - 14 &&
        py < die.top + die.size + 14
      ) {
        continue;
      }
      const len = 12 + rand() * 34;
      const horiz = rand() > 0.5;
      ctx.beginPath();
      ctx.moveTo(px, py);
      if (horiz) ctx.lineTo(px + len, py);
      else ctx.lineTo(px, py + len);
      ctx.stroke();
      if (rand() > 0.55) {
        ctx.fillStyle = "rgba(255,255,255,0.13)";
        ctx.fillRect(horiz ? px + len - 2 : px - 1.5, horiz ? py - 1.5 : py + len - 2, 3, 3);
      }
    }
  }

  function drawDie(ctx) {
    const die = geo.die;
    const board = geo.board;
    if (!die || !board) return;
    const { left, top, size } = die;

    /* local illumination: violet only while a collector is genuinely running */
    if (state.activity > 0.01) {
      const glow = ctx.createRadialGradient(
        die.cx,
        die.cy,
        size * 0.12,
        die.cx,
        die.cy,
        size * 1.45,
      );
      glow.addColorStop(
        0,
        "rgba(139,111,240," + (0.16 * state.activity).toFixed(3) + ")",
      );
      glow.addColorStop(1, "rgba(139,111,240,0)");
      ctx.fillStyle = glow;
      ctx.fillRect(left - size, top - size, size * 3, size * 3);
    }

    const body = ctx.createLinearGradient(left, top, left, top + size);
    body.addColorStop(0, "#1a202a");
    body.addColorStop(1, "#0c0f15");
    ctx.fillStyle = body;
    roundRect(ctx, left, top, size, size, size * 0.11);
    ctx.fill();
    ctx.strokeStyle = "rgba(255,255,255,0.13)";
    ctx.lineWidth = 1;
    ctx.stroke();
    ctx.strokeStyle = "rgba(255,255,255,0.05)";
    roundRect(ctx, left + 7, top + 7, size - 14, size - 14, size * 0.08);
    ctx.stroke();

    /* etched C.U.R.E mark */
    const unit = size * 0.1;
    const gx = die.cx - unit * 1.1;
    const gy = die.cy - unit * 1.95;
    ctx.fillStyle = "rgba(255,255,255,0.06)";
    for (let q = 0; q < 4; q += 1) {
      roundRect(
        ctx,
        gx + (q % 2) * (unit * 1.1),
        gy + Math.floor(q / 2) * (unit * 1.1),
        unit * 0.92,
        unit * 0.92,
        4,
      );
      ctx.fill();
    }

    /* border pins */
    ctx.fillStyle = "rgba(255,255,255,0.17)";
    const pin = 7;
    for (let i = 0; i < 12; i += 1) {
      const t = (i + 0.5) / 12;
      ctx.fillRect(left + t * size - pin / 2, top - 6, pin, 4);
      ctx.fillRect(left + t * size - pin / 2, top + size + 2, pin, 4);
      ctx.fillRect(left - 6, top + t * size - pin / 2, 4, pin);
      ctx.fillRect(left + size + 2, top + t * size - pin / 2, 4, pin);
    }

    /* die label */
    ctx.textAlign = "center";
    ctx.textBaseline = "alphabetic";
    ctx.fillStyle = C.text;
    ctx.font =
      '620 ' +
      Math.max(11, Math.round(size * 0.082)).toFixed(0) +
      'px "Segoe UI Variable Display", "Segoe UI", system-ui, sans-serif';
    ctx.fillText("C.U.R.E", die.cx, die.cy + size * 0.1);
    if ("letterSpacing" in ctx) ctx.letterSpacing = "0.2em";
    ctx.fillStyle = C.text3;
    ctx.font =
      '650 ' +
      Math.max(8, Math.round(size * 0.038)).toFixed(0) +
      'px "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif';
    ctx.fillText("SYSTEM CORE", die.cx, die.cy + size * 0.2);
    if ("letterSpacing" in ctx) ctx.letterSpacing = "0px";

    /* Two honest callouts: what the routes are, in the machine's own language */
    if (geo.board && geo.board.w > 220) {
      ctx.textAlign = "left";
      ctx.font =
        '650 ' + Math.max(8, Math.round(size * 0.036)).toFixed(0) +
        'px "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif';
      if ("letterSpacing" in ctx) ctx.letterSpacing = "0.16em";
      ctx.fillStyle = "rgba(255,255,255,0.24)";
      ctx.fillText("PERSISTENCE BUS", board.x + 14, board.y + board.h - 16);
      ctx.textAlign = "right";
      ctx.fillText(
        state.findings > 0 ? "EVIDENCE IN · " + state.findings : "EVIDENCE IN",
        board.x + board.w - 14,
        board.y + 20,
      );
      if ("letterSpacing" in ctx) ctx.letterSpacing = "0px";
    }
  }

  function drawTraces(ctx, now) {
    const die = geo.die;
    if (!die) return;
    for (let i = 0; i < geo.plates.length; i += 1) {
      const plate = geo.plates[i];
      const layer = state.layers[i];
      const points = routeFor(i, plate);
      if (!points) continue;
      const live = layer.id === state.active && state.running;
      ctx.strokeStyle = live ? "rgba(167,139,250,0.62)" : C.hair;
      ctx.lineWidth = live ? 1.3 : 1;
      strokeRoute(ctx, points);
      /* dash flow runs only while this collector is actually running */
      if (live) {
        ctx.save();
        ctx.setLineDash([4, 10]);
        ctx.lineDashOffset = -(now * 0.045) % 14;
        ctx.strokeStyle = "rgba(214,203,255,0.9)";
        ctx.lineWidth = 1.5;
        strokeRoute(ctx, points);
        ctx.restore();
      }
      /* via at the elbow, pin terminus at the die: the bus is a circuit */
      ctx.fillStyle = live ? "rgba(196,177,255,0.9)" : "rgba(255,255,255,0.18)";
      ctx.beginPath();
      ctx.arc(points[1].x, points[1].y, live ? 2 : 1.4, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = live ? C.keyLit : "rgba(255,255,255,0.16)";
      ctx.beginPath();
      ctx.arc(die.left, points[3].y, live ? 2.6 : 1.8, 0, Math.PI * 2);
      ctx.fill();
    }
  }

  /* A resolved entry travels its layer's route into the core. Routes are
     orthogonal, so trails never cross the layer labels. */
  function drawTrails(ctx) {
    const now = performance.now();
    for (let t = 0; t < state.trails.length; t += 1) {
      const trail = state.trails[t];
      const plate = null;
      const points = trail.points;
      if (!points) continue;
      const age = Math.min(1, (now - trail.at) / 480);
      const settled = Math.min(1, (now - trail.at) / 700);
      const base =
        trail.risk === "HighRisk"
          ? "228,112,106"
          : trail.risk === "Suspicious"
            ? "224,164,88"
            : "167,139,250";
      ctx.strokeStyle = "rgba(" + base + "," + (0.5 * settled).toFixed(3) + ")";
      ctx.lineWidth = 1.2;
      ctx.save();
      ctx.beginPath();
      const total = points.length - 1;
      const upto = Math.max(1, Math.ceil(age * total));
      ctx.moveTo(points[0].x, points[0].y);
      for (let p = 1; p <= upto && p < points.length; p += 1) {
        if (p === upto && age < 1) {
          const a = points[p - 1];
          const b = points[p];
          ctx.lineTo(a.x + (b.x - a.x) * (age * total - (p - 1)), a.y + (b.y - a.y) * (age * total - (p - 1)));
        } else {
          ctx.lineTo(points[p].x, points[p].y);
        }
      }
      ctx.stroke();
      if (age < 1) {
        const head = points[Math.min(total, upto - 1)];
        ctx.fillStyle = "rgba(" + base + ",0.85)";
        ctx.beginPath();
        ctx.arc(head.x, head.y, 2.2, 0, Math.PI * 2);
        ctx.fill();
      }
      ctx.restore();
      void plate;
    }
  }

  function drawPlates(ctx) {
    const maxCount = maxIndexFor(null);
    for (let i = 0; i < geo.plates.length; i += 1) {
      const plate = geo.plates[i];
      const layer = state.layers[i];
      const active = layer.id === state.active && state.running;
      const color = toneColor(active ? "active" : layer.tone);

      ctx.fillStyle = active
        ? "rgba(139,111,240,0.14)"
        : "rgba(255,255,255,0.026)";
      roundRect(ctx, plate.x, plate.y, plate.w, plate.h, 4);
      ctx.fill();

      ctx.fillStyle = color;
      ctx.globalAlpha = active ? 1 : 0.8;
      roundRect(ctx, plate.x, plate.y, 2.5, plate.h, 1.2);
      ctx.fill();
      ctx.globalAlpha = 1;

      ctx.strokeStyle = active ? "rgba(167,139,250,0.42)" : C.hairSoft;
      ctx.lineWidth = 1;
      roundRect(ctx, plate.x, plate.y, plate.w, plate.h, 4);
      ctx.stroke();

      const labelSize = Math.max(8.5, Math.min(11, plate.h * 0.44));
      /* the value column shrinks with the plate so names stay readable */
      const valueWidth = Math.max(44, Math.min(96, plate.w * 0.38));
      const mid = plate.y + plate.h / 2;

      ctx.textAlign = "left";
      ctx.textBaseline = "middle";
      if ("letterSpacing" in ctx) ctx.letterSpacing = "0.06em";
      ctx.font =
        '600 ' +
        labelSize.toFixed(1) +
        'px "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif';
      ctx.fillStyle = active ? C.text : C.text2;
      ctx.fillText(
        ellipsize(ctx, layer.label.toUpperCase(), plate.w - valueWidth - 26),
        plate.x + 14,
        mid + 0.5,
      );
      if ("letterSpacing" in ctx) ctx.letterSpacing = "0px";

      /* the real value, right-aligned in a reserved column */
      const monoSize = Math.max(8, labelSize - 1.2);
      ctx.font = monoSize.toFixed(1) + 'px "Cascadia Code", Consolas, monospace';
      ctx.textAlign = "right";
      if (layer.count > 0) {
        const risky = layer.risk > 0;
        const riskColor = risky
          ? toneColor(layer.tone === "highrisk" ? "highrisk" : "review")
          : C.text3;
        const countText = String(layer.count);
        const right = plate.x + plate.w - 13;
        ctx.fillStyle = risky ? riskColor : C.text3;
        ctx.fillText(countText, right, mid + 0.5);
        if (risky) {
          /* a risk marker sits beside the count, never merged into it */
          const cw = ctx.measureText(countText).width;
          const mx = right - cw - 9;
          ctx.fillStyle = riskColor;
          ctx.beginPath();
          ctx.moveTo(mx, mid - 3);
          ctx.lineTo(mx + 3, mid + 2.4);
          ctx.lineTo(mx - 3, mid + 2.4);
          ctx.closePath();
          ctx.fill();
        }
      } else {
        ctx.fillStyle = layer.tone === "pending" ? C.text3 : color;
        ctx.fillText(
          ellipsize(ctx, layer.text, valueWidth - 18),
          plate.x + plate.w - 13,
          mid + 0.5,
        );
      }

      /* resolved density: how much of this layer was actually inspected */
      if (layer.count > 0) {
        const barW = Math.max(3, (layer.count / maxCount) * (plate.w - 26));
        ctx.fillStyle =
          layer.risk > 0 ? toneColor("review") : "rgba(167,139,250,0.5)";
        roundRect(ctx, plate.x + 14, plate.y + plate.h - 3.5, barW, 1.6, 0.8);
        ctx.fill();
      }
    }
  }

  function drawBeam(ctx) {
    /* Light, not paint: a faint travelling sheet with a crisp leading edge,
       contained by the board so it never reads as a selection rectangle. */
    const board = geo.board;
    if (!board) return;
    const left = 0;
    const right = board.x + board.w;
    const height = 58;
    const top = Math.max(0, beamY - height);
    ctx.save();
    ctx.globalCompositeOperation = "lighter";
    const band = ctx.createLinearGradient(0, top, 0, beamY);
    band.addColorStop(0, "rgba(139,111,240,0)");
    band.addColorStop(0.72, "rgba(139,111,240,0.03)");
    band.addColorStop(1, "rgba(167,139,250,0.075)");
    ctx.fillStyle = band;
    ctx.fillRect(left, top, right - left, height);
    ctx.fillStyle = "rgba(206,194,255,0.42)";
    ctx.fillRect(left, beamY - 0.75, right - left, 1.5);
    ctx.fillStyle = "rgba(167,139,250,0.13)";
    ctx.fillRect(left, beamY - 19, right - left, 1);
    ctx.fillStyle = "rgba(167,139,250,0.06)";
    ctx.fillRect(left, beamY - 36, right - left, 1);
    ctx.restore();
  }

  function draw() {
    if (!canvas || !geo.die || geo.die.size <= 0) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(geo.dpr, 0, 0, geo.dpr, 0, 0);
    ctx.clearRect(0, 0, geo.w, geo.h);
    const now = performance.now();
    drawChassis(ctx);
    drawDie(ctx);
    drawTraces(ctx, now);
    drawTrails(ctx);
    drawPlates(ctx);
    if (state.running) drawBeam(ctx);
  }
  function frame(now) {
    rafId = null;
    const dt = Math.min(64, now - (lastFrame || now));
    lastFrame = now;
    dashPhase += dt;
    const target = plateCenterY(state.active);
    beamTarget = target;
    const distance = Math.abs(target - beamY);
    /* settle fast onto a real collector change, then hold — no idle spinning */
    const ease = distance < 0.6 ? 0.5 : 0.14;
    beamY += (target - beamY) * ease;
    if (state.running) state.activity += (1 - state.activity) * 0.08;
    else state.activity += (0 - state.activity) * 0.12;
    if (state.trails.length) state.trails = state.trails.filter(function (t) {
      return now - t.at < 60000;
    });
    draw();
    schedule();
  }

  function schedule() {
    if (rafId !== null) return;
    rafId = requestAnimationFrame(frame);
  }

  function startLoop() {
    if (motionOff()) {
      beamY = beamTarget = plateCenterY(state.active);
      state.activity = state.running ? 1 : 0;
      draw();
      return;
    }
    if (rafId === null) {
      lastFrame = performance.now();
      schedule();
    }
  }

  function stopLoop() {
    if (rafId !== null) cancelAnimationFrame(rafId);
    rafId = null;
  }

  /* ---------- DOM ledger (the accessible, precise layer list) ------------ */

  function paintLedger() {
    if (!sourceList) return;
    sourceList.replaceChildren();
    for (let i = 0; i < state.layers.length; i += 1) {
      const layer = state.layers[i];
      const active = layer.id === state.active && state.running;
      const li = document.createElement("li");
      li.className = active ? "active" : layer.tone;
      li.dataset.source = layer.id;
      const dot = document.createElement("span");
      dot.className = "cov-dot " +
        (active ? "" : layer.tone === "checked" ? "ok" : layer.tone === "partial" || layer.tone === "warn" || layer.tone === "review" || layer.tone === "risk" ? "warn" : layer.tone === "bad" || layer.tone === "highrisk" ? "bad" : "");
      const name = document.createElement("span");
      name.className = "cov-name";
      name.textContent = layer.full;
      const status = document.createElement("span");
      status.className = "cov-detail source-state";
      const parts = [];
      if (active) parts.push("Inspecting");
      else parts.push(layer.text);
      if (layer.count > 0) parts.push(layer.count + " inspected");
      if (layer.risk > 0)
        parts.push(layer.risk + (layer.tone === "highrisk" ? " high risk" : " to review"));
      if (layer.detail) parts.push(layer.detail);
      status.textContent = parts.join(" · ");
      li.append(dot, name, status);
      sourceList.appendChild(li);
    }
    if (itemsEl) {
      itemsEl.textContent =
        state.items + (state.items === 1 ? " entry inspected" : " entries inspected");
    }
    if (inspectedEl) inspectedEl.textContent = String(state.items);
    if (findingsEl) {
      findingsEl.textContent = String(state.findings);
      findingsEl.className = "ins-value" + (state.findings ? " is-warn" : "");
    }
    if (phaseEl) {
      const activeLayer = layerById(state.active);
      phaseEl.textContent = activeLayer
        ? activeLayer.label
        : state.running
          ? "Preparing collection"
          : "Awaiting collection";
    }
    if (coverageEl) {
      const reported = state.layers.filter(function (l) {
        return l.tone !== "pending" && l.tone !== "idle";
      });
      if (!state.running && reported.length === 0) coverageEl.textContent = "—";
      else {
        const checked = state.layers.filter(function (l) {
          return l.tone === "checked";
        }).length;
        const limited = state.layers.length - checked;
        coverageEl.textContent =
          checked + "/" + state.layers.length +
          (limited > 0 && !state.running ? " · " + limited + " limited" : "");
      }
    }
  }

  function completeActive() {
    if (!state.active) return;
    const layer = layerById(state.active);
    if (layer && layer.tone === "active") {
      layer.tone = "checked";
      layer.text = "Collected";
    }
  }

  function pushTrail(id, risk) {
    for (let i = 0; i < geo.plates.length; i += 1) {
      if (geo.plates[i].id !== id) continue;
      const points = routeFor(geo.plates[i].index, geo.plates[i]);
      if (!points) return;
      state.trails.push({
        points: points.map(function (p) { return { x: p.x, y: p.y }; }),
        at: performance.now(),
        risk: risk || null,
      });
      if (state.trails.length > 18) state.trails.shift();
      return;
    }
  }

  /* ---------- public API (unchanged call sites) -------------------------- */

  window.CureSweep = {
    start() {
      /* Rescuing reviews overlay candidates before the real scan starts, so
         start() can be called twice for one session. Only a genuinely new
         session rotates the companion variant. */
      const newSession = !state.running;
      state.active = null;
      state.items = 0;
      state.findings = 0;
      state.trails = [];
      state.running = true;
      state.activity = 0;
      for (let i = 0; i < state.layers.length; i += 1) {
        state.layers[i].tone = "pending";
        state.layers[i].text = "Pending";
        state.layers[i].count = 0;
        state.layers[i].risk = 0;
        state.layers[i].detail = "";
      }
      if (newSession) window.CureCompanion.beginSession("scan");
      if (currentEl) currentEl.textContent = "Awaiting collection";
      if (coreReadout) coreReadout.textContent = "Awaiting collection";
      paintLedger();
      layout();
      startLoop();
    },

    progress(p) {
      if (p.stage === "source-state") {
        for (let i = 0; i < LAYERS.length; i += 1) {
          if (LAYERS[i].area !== p.area) continue;
          const layer = layerById(LAYERS[i].id);
          if (!layer) break;
          layer.tone = coverageTone(p.state);
          layer.text = coverageText(p.state);
          layer.detail = p.detail ? String(p.detail) : "";
        }
        paintLedger();
        draw();
        return;
      }

      const stage = STAGE_ALIAS[p.stage] || p.stage;
      const known = layerById(stage);
      if (known) {
        if (state.active !== stage) completeActive();
        state.active = stage;
        known.tone = "active";
        known.text = "Inspecting";
      }

      if (["scoring", "item-scan", "done"].indexOf(p.stage) !== -1) {
        completeActive();
        state.active = null;
      }

      if (p.stage === "item-scanned") {
        completeActive();
        const id = ITEM_SOURCE[p.source] || null;
        state.active = id;
        const layer = id ? layerById(id) : null;
        if (layer) {
          layer.tone = "active";
          layer.text = "Inspecting entry";
          layer.count += 1;
          if (p.risk && p.risk !== "Safe") {
            layer.risk += 1;
            state.findings += 1;
            if (p.risk === "HighRisk") layer.tone = "highrisk";
            else if (layer.tone !== "highrisk") layer.tone = "review";
            pushTrail(layer.id, p.risk);
          }
        }
        state.items += 1;
      }
      if (p.stage === "process-flagged") {
        state.items += 1;
        const layer = layerById("processes");
        if (layer) {
          layer.count += 1;
          layer.risk += 1;
          layer.tone = "review";
          pushTrail("processes", p.risk);
        }
      }
      if (p.stage === "ransom-found") {
        state.items += 1;
        state.findings += 1;
        const layer = layerById("ransom");
        if (layer) {
          layer.count += 1;
          layer.risk += 1;
          layer.tone = "highrisk";
          pushTrail("ransom", "HighRisk");
        }
      }

      const message =
        p.message ||
        (p.stage === "item-scanned" ? "Inspecting: " + p.name : "Collecting evidence");
      if (currentEl) currentEl.textContent = message;
      if (coreReadout) coreReadout.textContent = message;
      const activeLayer = layerById(state.active);
      window.CureCompanion.scan(
        "scan",
        message,
        Math.max(0, indexOfActive()),
        window.CureCompanion.variant("scan"),
      );
      paintLedger();
      if (rafId === null) startLoop();
    },

    finish(summary) {
      completeActive();
      state.active = null;
      state.running = false;
      const rows = (summary && summary.source_states) || [];
      for (let i = 0; i < LAYERS.length; i += 1) {
        const layer = state.layers[i];
        if (layer.tone === "pending") {
          layer.tone = "idle";
          layer.text = "Coverage unreported";
        }
        if (!LAYERS[i].area) continue;
        const row = rows.find(function (r) {
          return r.area === LAYERS[i].area;
        });
        if (!row) continue;
        layer.tone = coverageTone(row.state);
        layer.text = coverageText(row.state);
        layer.detail = row.detail ? String(row.detail) : "";
      }
      if (currentEl) currentEl.textContent = "Collection ended";
      if (coreReadout) coreReadout.textContent = "Collection ended";
      window.CureCompanion.scan(
        state.findings > 0 ? "review" : "idle",
        "Collection ended",
        0,
        window.CureCompanion.variant("scan"),
      );
      paintLedger();
      beamY = beamTarget = geo.die ? geo.die.cy : beamY;
      draw();
      stopLoop();
    },

    fail() {
      if (state.active) {
        const layer = layerById(state.active);
        if (layer) {
          layer.tone = "bad";
          layer.text = "Interrupted";
        }
      }
      state.active = null;
      state.running = false;
      if (currentEl)
        currentEl.textContent = "Collection interrupted — retry available";
      if (coreReadout) coreReadout.textContent = "Interrupted";
      window.CureCompanion.scan(
        "review",
        "Collection interrupted — retry available",
        0,
        window.CureCompanion.variant("scan"),
      );
      paintLedger();
      stopLoop();
      draw();
    },

    motion: function (paused) {
      motionPaused = !!paused;
      store("cure-motion-paused", String(motionPaused));
      if (motionBtn) {
        motionBtn.textContent = motionPaused ? "Resume field motion" : "Pause field motion";
        motionBtn.setAttribute("aria-pressed", String(motionPaused));
      }
      if (motionOff()) {
        stopLoop();
        beamY = beamTarget = plateCenterY(state.active);
        draw();
      } else {
        startLoop();
      }
    },
    motionPaused: function () {
      return motionPaused;
    },
    redraw: draw,
  };

  /* ---------- wiring ------------------------------------------------------ */

  if (motionBtn) {
    motionBtn.addEventListener("click", function () {
      window.CureSweep.motion(!motionPaused);
    });
    motionBtn.textContent = motionPaused ? "Resume field motion" : "Pause field motion";
    motionBtn.setAttribute("aria-pressed", String(motionPaused));
  }

  if (field && "ResizeObserver" in window) {
    let queued = false;
    new ResizeObserver(function () {
      if (queued) return;
      queued = true;
      requestAnimationFrame(function () {
        queued = false;
        layout();
      });
    }).observe(field);
  } else {
    window.addEventListener("resize", layout);
  }

  document.addEventListener("visibilitychange", function () {
    if (document.hidden) stopLoop();
    else if (state.running) startLoop();
  });

  reducedMedia.addEventListener("change", function () {
    if (motionOff()) {
      stopLoop();
      beamY = beamTarget = plateCenterY(state.active);
      draw();
    } else if (state.running) startLoop();
  });

  if (field) layout();
  paintLedger();
})();