/* ==========================================================================
   C.U.R.E SYSTEM X-RAY — holographic persistence radar
   Nine collector layers painted from the backend's own event stream
   (`scan-progress`), never from a timer and never from a guess.

   Each layer carries exactly one *outcome* at a time:
     pending    - the collector has not reported yet
     running    - the backend named this collector as active
     checked    - the backend reported a complete coverage row
     partial    - the backend reported a coverage row with skips
     denied     - the backend reported an access-denied coverage row
     failed     - the backend reported a failed check
     unreported - the collector ran but the backend sends no per-area
                  coverage row for it, so it is NOT counted as checked

   Findings are tracked separately from coverage: a layer can be fully
   checked and still carry entries that need review. Nothing here invents
   progress, detections or a health score.
   ========================================================================== */
(function () {
  "use strict";

  /* ---------- The nine inspection layers -------------------------------- */
  const LAYERS = [
    {
      id: "registry",
      label: "Registry",
      full: "Registry autoruns",
      areas: ["Registry autoruns"],
      note: "HKCU / HKLM Run and RunOnce values",
    },
    {
      id: "startup",
      label: "Startup",
      full: "Startup folders",
      areas: ["Startup folders", "Startup folder"],
      note: "per-user and machine-wide Startup folders",
    },
    {
      id: "tasks",
      label: "Tasks",
      full: "Scheduled tasks",
      areas: ["Scheduled tasks"],
      note: "task XML definitions",
    },
    {
      id: "services",
      label: "Services",
      full: "Auto-start services",
      areas: ["Services (auto-start)", "Services"],
      note: "service control manager start types",
    },
    {
      id: "wmi",
      label: "WMI",
      full: "WMI subscriptions",
      areas: ["WMI subscriptions"],
      note: "event consumer bindings",
    },
    {
      id: "ifeo",
      label: "IFEO",
      full: "IFEO and AppInit",
      areas: ["IFEO and AppInit", "IFEO debuggers", "AppInit DLLs", "IFEO / AppInit"],
      note: "debugger hooks and AppInit DLLs",
    },
    {
      id: "com",
      label: "COM",
      full: "COM registrations",
      areas: ["COM registrations", "COM hijacks (HKCU)"],
      note: "per-user InprocServer32 / TreatAs",
    },
    {
      id: "processes",
      label: "Processes",
      full: "Running processes",
      areas: ["Running processes", "Processes"],
      note: "live process signatures and hashes",
    },
    {
      id: "ransom",
      label: "Ransom",
      full: "Ransom indicators",
      areas: ["Ransom indicators"],
      note: "ransom notes and bulk encryption",
    },
  ];

  function layerMatchesArea(L, areaName) {
    if (!areaName || !L) return false;
    if (L.areas && L.areas.includes(areaName)) return true;
    if (L.area && L.area === areaName) return true;
    if (L.full === areaName || L.label === areaName || L.id === areaName) return true;
    return false;
  }

  /* Stage names the backend emits -> layer id. `startup-common` belongs to
     the same layer as `startup`; process and ransom stages name their own. */
  const STAGE_ALIAS = {
    "startup-common": "startup",
    "process-scan": "processes",
    process: "processes",
    "ransom-scan": "ransom",
    "ransom-detect": "ransom",
  };

  /* Entry source tags (core PersistenceSource::tag) -> layer id. */
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

  const OUTCOME_TEXT = {
    pending: "Not started",
    running: "Running",
    checked: "Checked",
    partial: "Partial",
    denied: "Access denied",
    failed: "Failed",
    unsupported: "Not supported",
    unreported: "Coverage not reported",
  };

  /* Backend CoverageState -> outcome. Real Tauri serialises "Checked" /
     "NotChecked" / "Unavailable" / "AccessDenied" / "CheckFailed" or
     {"Partial": {"skipped": N}}; the labelled mock also emits "Available". */
  function outcomeFromCoverage(raw) {
    const key =
      typeof raw === "string" ? raw : Object.keys(raw || {})[0] || "NotChecked";
    if (key === "Checked" || key === "Available") return "checked";
    if (key === "Partial") return "partial";
    if (key === "AccessDenied") return "denied";
    if (key === "CheckFailed") return "failed";
    if (key === "Unavailable") return "unsupported";
    if (key === "NotChecked") return "pending";
    return "pending";
  }

  function skippedCount(raw, detail) {
    if (raw && typeof raw === "object" && raw.Partial) {
      const n = Number(raw.Partial.skipped);
      if (Number.isFinite(n) && n > 0) return n;
    }
    const m = String(detail || "").match(/(\d+)\s+skipped/i);
    return m ? Number(m[1]) : 0;
  }

  /* ---------- DOM ------------------------------------------------------- */
  const canvas = document.getElementById("xray-canvas");
  const field = document.getElementById("xray-field");
  const layerList = document.getElementById("sweep-sources");
  const itemsEl = document.getElementById("sweep-items");
  const currentEl = document.getElementById("sweep-current");
  const coreReadout = document.getElementById("xray-core-readout");
  const phaseEl = document.getElementById("xray-phase");
  const phaseLabelEl = document.getElementById("xray-phase-label");
  const coverageEl = document.getElementById("xray-coverage");
  const matrixCoverEl = document.getElementById("xray-matrix-cover");
  const inspectedEl = document.getElementById("xray-inspected");
  const findingsEl = document.getElementById("xray-findings");
  const liveEl = document.getElementById("xray-live");
  const motionBtn = document.getElementById("xray-motion");

  const hudBadge = document.getElementById("xray-hud-badge");
  const hudName = document.getElementById("xray-hud-name");
  const hudDetail = document.getElementById("xray-hud-detail");
  const hudInspected = document.getElementById("xray-hud-inspected");
  const hudCoverage = document.getElementById("xray-hud-coverage");
  const hudFindings = document.getElementById("xray-hud-findings");
  const hudUnavailable = document.getElementById("xray-hud-unavailable");

  const reducedMedia = matchMedia("(prefers-reduced-motion: reduce)");

  /* ---------- State ----------------------------------------------------- */
  const state = {
    phase: "idle", // idle | preparing | scanning | complete | failed
    running: false,
    active: null,
    entries: 0,
    findings: 0,
    lastMessage: "",
    ambientAngle: 0,
    sweepAngle: -Math.PI / 2,
    hoveredLayer: null,
    expanded: {},
    ripples: [],
    layers: LAYERS.map(function (L) {
      const obj = {
        id: L.id,
        label: L.label,
        full: L.full,
        areas: L.areas || (L.area ? [L.area] : []),
        area: L.area || (L.areas && L.areas[0]) || null,
        note: L.note,
        outcome: "pending",
        ran: false,
        count: 0,
        flagged: 0,
        detail: "",
        beacons: [],
      };
      Object.defineProperty(obj, "tone", {
        get() {
          return this.outcome;
        },
        set(v) {
          this.outcome = v;
        },
        configurable: true,
        enumerable: true,
      });
      Object.defineProperty(obj, "text", {
        get() {
          return OUTCOME_TEXT[this.outcome] || this.outcome;
        },
        set(v) {
          /* allow assignment */
        },
        configurable: true,
        enumerable: true,
      });
      return obj;
    }),
  };

  function layerById(id) {
    for (let i = 0; i < state.layers.length; i += 1) {
      if (state.layers[i].id === id) return state.layers[i];
    }
    return null;
  }

  /* Coverage is counted only from outcomes the backend actually reported. */
  function counts() {
    let covered = 0;
    let partial = 0;
    let denied = 0;
    let failed = 0;
    let unsupported = 0;
    let unreported = 0;
    for (let i = 0; i < state.layers.length; i += 1) {
      const o = state.layers[i].outcome;
      if (o === "checked") covered += 1;
      else if (o === "partial") {
        covered += 1;
        partial += 1;
      } else if (o === "denied") denied += 1;
      else if (o === "failed") failed += 1;
      else if (o === "unsupported") unsupported += 1;
      else if (o === "unreported") unreported += 1;
    }
    return {
      covered: covered,
      partial: partial,
      denied: denied,
      failed: failed,
      unsupported: unsupported,
      unreported: unreported,
      unreadable: denied + failed,
      total: state.layers.length,
    };
  }

  /* Honest verdict: the radar, the badge and the readouts all render this
     one object, so no two panels can disagree about the same scan. */
  function getVerdict() {
    const c = counts();
    const complete = c.covered === c.total && c.partial === 0 && c.denied === 0 && c.failed === 0;
    let tone = "idle";
    let headline = "Awaiting operator";
    let detail = "No collection has run in this session";
    if (state.phase === "preparing") {
      tone = "running";
      headline = "Preparing collection";
      detail = "Reviewing overlay windows before the scan";
    } else if (state.running) {
      tone = "running";
      headline = state.active
        ? (layerById(state.active) || {}).full || "Collecting"
        : "Collecting persistence layers";
      detail = state.lastMessage || "Collecting persistence layers";
    } else if (state.phase === "failed") {
      tone = "failed";
      headline = "Collection failed";
      detail = state.lastMessage || "The scan did not finish";
    } else if (state.phase === "complete") {
      if (c.failed > 0) {
        tone = "failed";
        headline = "Collection incomplete";
        detail = c.failed + " collector(s) could not run";
      } else if (state.findings > 0) {
        tone = complete ? "attention" : "attention";
        headline =
          state.findings +
          (state.findings === 1 ? " finding for review" : " findings for review");
        detail = complete
          ? "Every reported layer was checked"
          : c.covered + " of " + c.total + " layers report coverage";
      } else if (!complete) {
        tone = "partial";
        const gaps = [];
        if (c.denied > 0) gaps.push(c.denied + " unreadable (access denied)");
        if (c.partial > 0) gaps.push(c.partial + " partially enumerated");
        if (c.unreported > 0)
          gaps.push(c.unreported + " ran without a coverage report");
        headline =
          c.denied > 0
            ? "No findings observed · coverage incomplete"
            : "No findings observed";
        detail =
          c.covered +
          " of " +
          c.total +
          " layers report coverage (" +
          gaps.join(", ") +
          ") — this is not an all-clear";
      } else {
        tone = "clean";
        headline = "No findings observed";
        detail = "All " + c.total + " layers reported coverage";
      }
    }
    return {
      phase: state.phase,
      tone: tone,
      headline: headline,
      detail: detail,
      running: state.running,
      active: state.active,
      entries: state.entries,
      findings: state.findings,
      complete: complete,
      covered: c.covered,
      total: c.total,
      partial: c.partial,
      denied: c.denied,
      failed: c.failed,
      unreported: c.unreported,
      unreadable: c.unreadable,
      layers: state.layers.map(function (L) {
        return {
          id: L.id,
          label: L.label,
          full: L.full,
          outcome: L.outcome,
          text: OUTCOME_TEXT[L.outcome] || L.outcome,
          count: L.count,
          flagged: L.flagged,
          detail: L.detail,
        };
      }),
    };
  }

  /* ---------- Motion ---------------------------------------------------- */
  let motionPaused = reducedMedia.matches;
  let rafId = null;
  let lastTime = performance.now();

  function prefersMotion() {
    return !motionPaused && !reducedMedia.matches;
  }

  /* ---------- Geometry -------------------------------------------------- */
  const geo = {
    w: 800,
    h: 460,
    cx: 400,
    cy: 230,
    dpr: 1,
    innerR: 56,
    outerR: 215,
    ringStep: 18,
    orbitAngles: LAYERS.map(function (_, i) {
      return i * ((Math.PI * 2) / 9) - Math.PI / 2;
    }),
  };

  function layout() {
    if (!field || !canvas) return;
    const rect = field.getBoundingClientRect();
    if (rect.width < 2 || rect.height < 2) return;

    geo.w = Math.round(rect.width);
    geo.h = Math.round(rect.height);
    geo.dpr = Math.min(2, window.devicePixelRatio || 1);
    canvas.width = Math.round(geo.w * geo.dpr);
    canvas.height = Math.round(geo.h * geo.dpr);

    geo.cx = Math.round(geo.w * 0.5);
    geo.cy = Math.round(geo.h * 0.5);

    const maxR = Math.min(geo.w * 0.46, geo.h * 0.47);
    geo.outerR = Math.max(150, Math.round(maxR * 0.97));
    geo.innerR = Math.max(50, Math.round(geo.outerR * 0.28));
    geo.ringStep = (geo.outerR - geo.innerR) / 9;

    if (coreReadout) {
      coreReadout.style.left = geo.cx + "px";
      coreReadout.style.top =
        Math.min(geo.h - 18, Math.round(geo.cy + geo.outerR + 15)) + "px";
    }
    draw();
  }

  /* ---------- Palette --------------------------------------------------- */
  function getPalette() {
    const isLight = document.documentElement.getAttribute("data-theme") === "light";
    return {
      isLight: isLight,
      ringBase: isLight ? "rgba(71, 85, 105, 0.18)" : "rgba(148, 163, 184, 0.15)",
      ringActive: isLight ? "#7c3aed" : "#a78bfa",
      ringCyan: isLight ? "#0891b2" : "#22d3ee",
      ringAmber: isLight ? "#d97706" : "#f59e0b",
      ringRed: isLight ? "#dc2626" : "#ef4444",
      ringDim: isLight ? "rgba(100, 116, 139, 0.45)" : "rgba(148, 163, 184, 0.42)",
      sweepFill: isLight ? "rgba(124, 58, 237, 0.15)" : "rgba(139, 111, 240, 0.20)",
      sweepLine: isLight ? "#0891b2" : "#00f0ff",
      hubBody: isLight ? "#ffffff" : "#0c101a",
      hubBorder: isLight ? "rgba(124, 58, 237, 0.4)" : "rgba(139, 111, 240, 0.4)",
      textPrimary: isLight ? "#0f172a" : "#f1f5f9",
      textDim: isLight ? "#64748b" : "#94a3b8",
      glowCyan: isLight ? "rgba(8, 145, 178, 0.25)" : "rgba(34, 211, 238, 0.38)",
    };
  }

  function toneColor(tone, P) {
    if (tone === "checked") return P.ringCyan;
    if (tone === "partial") return P.ringAmber;
    if (tone === "denied") return P.ringAmber;
    if (tone === "failed") return P.ringRed;
    if (tone === "active") return P.ringActive;
    if (tone === "unreported") return P.ringDim;
    return P.ringBase;
  }

  /* ---------- Canvas ---------------------------------------------------- */
  function triggerShockwave(color) {
    if (!prefersMotion()) return;
    state.ripples.push({
      r: geo.innerR + 4,
      maxR: geo.outerR + 25,
      color: color || "#22d3ee",
      alpha: 0.8,
    });
    if (state.ripples.length > 4) state.ripples.shift();
  }

  function draw() {
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    ctx.save();
    ctx.scale(geo.dpr, geo.dpr);
    ctx.clearRect(0, 0, geo.w, geo.h);

    const P = getPalette();
    const cx = geo.cx;
    const cy = geo.cy;

    /* 1. Tactical crosshairs, coordinate grid and polar degree markers. */
    ctx.lineWidth = 1;
    ctx.strokeStyle = P.ringBase;
    ctx.setLineDash([3, 5]);
    ctx.beginPath();
    ctx.moveTo(cx - geo.outerR - 24, cy);
    ctx.lineTo(cx + geo.outerR + 24, cy);
    ctx.moveTo(cx, cy - geo.outerR - 24);
    ctx.lineTo(cx, cy + geo.outerR + 24);
    ctx.stroke();

    const dLen = geo.outerR + 14;
    const dCos = Math.cos(Math.PI / 4) * dLen;
    ctx.beginPath();
    ctx.moveTo(cx - dCos, cy - dCos);
    ctx.lineTo(cx + dCos, cy + dCos);
    ctx.moveTo(cx - dCos, cy + dCos);
    ctx.lineTo(cx + dCos, cy - dCos);
    ctx.stroke();
    ctx.setLineDash([]);

    ctx.font = "600 8.5px monospace";
    ctx.fillStyle = P.textDim;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText("000\u00B0", cx, cy - geo.outerR - 16);
    ctx.fillText("090\u00B0", cx + geo.outerR + 18, cy);
    ctx.fillText("180\u00B0", cx, cy + geo.outerR + 16);
    ctx.fillText("270\u00B0", cx - geo.outerR - 18, cy);

    /* Calibrated tick ring: a background instrument, not a progress meter. */
    ctx.save();
    ctx.translate(cx, cy);
    ctx.rotate(state.ambientAngle);
    ctx.strokeStyle = P.isLight
      ? "rgba(8, 145, 178, 0.28)"
      : "rgba(34, 211, 238, 0.32)";
    ctx.lineWidth = 1;
    const outerTickR = geo.outerR + 10;
    for (let t = 0; t < 24; t += 1) {
      const a = (t * Math.PI) / 12;
      const tLen = t % 6 === 0 ? 6 : 3.5;
      ctx.beginPath();
      ctx.moveTo(
        Math.cos(a) * (outerTickR - tLen),
        Math.sin(a) * (outerTickR - tLen)
      );
      ctx.lineTo(Math.cos(a) * outerTickR, Math.sin(a) * outerTickR);
      ctx.stroke();
    }
    ctx.restore();

    /* 2. One segmented orbital track per layer, coloured by its outcome. */
    const segmentsPerTrack = 8;
    const segAngle = (Math.PI * 2) / segmentsPerTrack;
    const gapAngle = 0.04;

    for (let i = 0; i < 9; i += 1) {
      const r = geo.innerR + (i + 1) * geo.ringStep;
      const layer = state.layers[i];
      const isActive = state.active === layer.id;
      const isHovered = state.hoveredLayer === layer.id;

      let strokeColor = toneColor(layer.outcome, P);
      let strokeWidth = 1.4;
      let lineDash = [];

      if (layer.outcome === "checked") {
        strokeWidth = isActive ? 2.6 : 2.0;
      } else if (layer.outcome === "partial" || layer.outcome === "denied") {
        strokeWidth = 2.0;
        lineDash = [10, 4, 3, 4];
      } else if (layer.outcome === "failed") {
        strokeWidth = 2.2;
        lineDash = [5, 4];
      } else if (layer.outcome === "unreported") {
        strokeWidth = 1.6;
        lineDash = [2, 5];
      } else if (isActive) {
        strokeColor = P.ringActive;
        strokeWidth = 2.8;
      } else if (isHovered) {
        strokeColor = P.ringCyan;
        strokeWidth = 2.0;
      }

      ctx.save();
      ctx.strokeStyle = strokeColor;
      ctx.lineWidth = strokeWidth;
      if (lineDash.length > 0) ctx.setLineDash(lineDash);
      for (let s = 0; s < segmentsPerTrack; s += 1) {
        ctx.beginPath();
        ctx.arc(
          cx,
          cy,
          r,
          s * segAngle + gapAngle,
          (s + 1) * segAngle - gapAngle
        );
        ctx.stroke();
      }
      ctx.restore();

      /* Travelling bead only on the collector the backend named. */
      if (isActive && prefersMotion()) {
        const beadAngle = (state.ambientAngle * (3.8 + i * 0.3)) % (Math.PI * 2);
        const bx = cx + Math.cos(beadAngle) * r;
        const by = cy + Math.sin(beadAngle) * r;
        ctx.beginPath();
        ctx.arc(bx, by, 7.5, 0, Math.PI * 2);
        ctx.fillStyle = "rgba(167, 139, 250, 0.4)";
        ctx.fill();
        ctx.beginPath();
        ctx.arc(bx, by, 3.4, 0, Math.PI * 2);
        ctx.fillStyle = "#ffffff";
        ctx.fill();
      }

      /* Layer node at the orbit angle. */
      const angle = geo.orbitAngles[i];
      const nx = cx + Math.cos(angle) * r;
      const ny = cy + Math.sin(angle) * r;
      ctx.beginPath();
      ctx.arc(nx, ny, isActive ? 4.8 : isHovered ? 4.2 : 3.0, 0, Math.PI * 2);
      ctx.fillStyle = isActive ? P.ringActive : toneColor(layer.outcome, P);
      ctx.fill();
      if (isActive || isHovered) {
        ctx.strokeStyle = "#ffffff";
        ctx.lineWidth = 1.2;
        ctx.stroke();
      }
    }

    /* 3. Sweep beam: an activity indicator while a collection is running. */
    if (state.running && prefersMotion()) {
      const sweepAngle = state.sweepAngle;
      const sweepSpan = Math.PI / 3.6;
      const grad = ctx.createRadialGradient(
        cx,
        cy,
        geo.innerR,
        cx,
        cy,
        geo.outerR + 14
      );
      grad.addColorStop(0, P.sweepFill);
      grad.addColorStop(0.7, "rgba(34, 211, 238, 0.09)");
      grad.addColorStop(1, "rgba(34, 211, 238, 0)");

      ctx.save();
      ctx.beginPath();
      ctx.moveTo(cx, cy);
      ctx.arc(cx, cy, geo.outerR + 14, sweepAngle - sweepSpan, sweepAngle, false);
      ctx.closePath();
      ctx.fillStyle = grad;
      ctx.fill();
      ctx.beginPath();
      ctx.moveTo(cx, cy);
      ctx.lineTo(
        cx + Math.cos(sweepAngle) * (geo.outerR + 16),
        cy + Math.sin(sweepAngle) * (geo.outerR + 16)
      );
      ctx.strokeStyle = P.sweepLine;
      ctx.lineWidth = 2.4;
      ctx.shadowColor = P.sweepLine;
      ctx.shadowBlur = 10;
      ctx.stroke();
      ctx.restore();
    }

    /* 4. Shockwave ripples fired by real collector transitions. */
    if (state.ripples.length > 0 && prefersMotion()) {
      for (let rp = 0; rp < state.ripples.length; rp += 1) {
        const ripple = state.ripples[rp];
        ctx.beginPath();
        ctx.arc(cx, cy, ripple.r, 0, Math.PI * 2);
        ctx.strokeStyle = ripple.color;
        ctx.globalAlpha = Math.max(0, ripple.alpha);
        ctx.lineWidth = 1.6;
        ctx.stroke();
        ctx.globalAlpha = 1.0;
      }
    }

    /* 5. Evidence beacons: one diamond per entry that needs review. */
    for (let i = 0; i < state.layers.length; i += 1) {
      const layer = state.layers[i];
      if (!layer.beacons || layer.beacons.length === 0) continue;
      const r = geo.innerR + (i + 1) * geo.ringStep;
      for (let b = 0; b < layer.beacons.length; b += 1) {
        const beacon = layer.beacons[b];
        const bx = cx + Math.cos(beacon.angle) * r;
        const by = cy + Math.sin(beacon.angle) * r;

        if (prefersMotion() && beacon.radius > 0) {
          ctx.beginPath();
          ctx.arc(bx, by, beacon.radius, 0, Math.PI * 2);
          ctx.strokeStyle =
            beacon.risk === "HighRisk"
              ? "rgba(239, 68, 68, 0.48)"
              : "rgba(245, 158, 11, 0.48)";
          ctx.lineWidth = 1.3;
          ctx.stroke();
        }

        const isHigh = beacon.risk === "HighRisk";
        ctx.save();
        ctx.translate(bx, by);
        ctx.beginPath();
        ctx.moveTo(0, -5.5);
        ctx.lineTo(5.5, 0);
        ctx.lineTo(0, 5.5);
        ctx.lineTo(-5.5, 0);
        ctx.closePath();
        ctx.fillStyle = isHigh ? P.ringRed : P.ringAmber;
        ctx.fill();
        ctx.strokeStyle = "#ffffff";
        ctx.lineWidth = 1.2;
        ctx.stroke();
        ctx.beginPath();
        ctx.arc(0, 0, 2.0, 0, Math.PI * 2);
        ctx.fillStyle = "#ffffff";
        ctx.fill();
        ctx.restore();
      }
    }

    /* 6. Central hub: nine outcome ticks around the live readout. */
    const hubR = geo.innerR;
    const verdict = getVerdict();
    ctx.save();
    ctx.translate(cx, cy);

    if (state.running) {
      const glow = ctx.createRadialGradient(0, 0, hubR * 0.2, 0, 0, hubR * 1.6);
      glow.addColorStop(0, P.glowCyan);
      glow.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = glow;
      ctx.beginPath();
      ctx.arc(0, 0, hubR * 1.6, 0, Math.PI * 2);
      ctx.fill();
    }

    const tickArc = (Math.PI * 2) / 9;
    for (let k = 0; k < 9; k += 1) {
      const tickColor = toneColor(state.layers[k].outcome, P);
      const sAngle = k * tickArc - Math.PI / 2 + 0.05;
      const eAngle = (k + 1) * tickArc - Math.PI / 2 - 0.05;
      ctx.beginPath();
      ctx.arc(0, 0, hubR, sAngle, eAngle);
      ctx.strokeStyle = tickColor;
      ctx.lineWidth = 3.4;
      ctx.stroke();
      const midA = (sAngle + eAngle) / 2;
      ctx.beginPath();
      ctx.arc(
        Math.cos(midA) * (hubR + 4),
        Math.sin(midA) * (hubR + 4),
        1.6,
        0,
        Math.PI * 2
      );
      ctx.fillStyle = tickColor;
      ctx.fill();
    }

    ctx.save();
    ctx.rotate(-state.ambientAngle * 1.4);
    ctx.beginPath();
    ctx.arc(0, 0, hubR * 0.82, 0, Math.PI * 2);
    ctx.setLineDash([4, 4]);
    ctx.strokeStyle = P.isLight
      ? "rgba(8, 145, 178, 0.42)"
      : "rgba(34, 211, 238, 0.48)";
    ctx.lineWidth = 1.3;
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.restore();

    ctx.beginPath();
    ctx.arc(0, 0, hubR * 0.74, 0, Math.PI * 2);
    ctx.fillStyle = P.hubBody;
    ctx.fill();
    ctx.strokeStyle = P.hubBorder;
    ctx.lineWidth = 1.6;
    ctx.stroke();

    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    if (state.phase === "idle") {
      ctx.font = "800 11.5px Inter, system-ui, sans-serif";
      ctx.fillStyle = P.textPrimary;
      ctx.fillText("C.U.R.E", 0, -6);
      ctx.font = "700 9px monospace";
      ctx.fillStyle = P.ringCyan;
      ctx.fillText("READY", 0, 7);
    } else if (state.running) {
      ctx.font = "800 13.5px 'Cascadia Code', Consolas, monospace";
      ctx.fillStyle = P.ringCyan;
      ctx.fillText(verdict.covered + " / " + verdict.total, 0, -7);
      ctx.font = "700 8.5px monospace";
      ctx.fillStyle = P.textDim;
      ctx.fillText("CHECKED", 0, 6);
      if (state.active) {
        const act = layerById(state.active);
        ctx.font = "600 7.5px monospace";
        ctx.fillStyle = P.ringActive;
        ctx.fillText(act ? act.label.toUpperCase() : "SCAN", 0, 16);
      }
    } else if (state.phase === "failed") {
      ctx.font = "800 10.5px 'Cascadia Code', Consolas, monospace";
      ctx.fillStyle = P.ringRed;
      ctx.fillText("FAILED", 0, -6);
      ctx.font = "700 8px monospace";
      ctx.fillStyle = P.textDim;
      ctx.fillText("NO RESULT", 0, 7);
    } else {
      /* Completed: state the count of findings, never a health score. */
      if (state.findings > 0) {
        ctx.font = "800 12.5px 'Cascadia Code', Consolas, monospace";
        ctx.fillStyle = P.ringAmber;
        ctx.fillText(String(state.findings), 0, -6);
        ctx.font = "700 8px monospace";
        ctx.fillStyle = P.ringAmber;
        ctx.fillText("TO REVIEW", 0, 7);
      } else {
        ctx.font = "800 12.5px 'Cascadia Code', Consolas, monospace";
        ctx.fillStyle = verdict.complete ? P.ringCyan : P.ringAmber;
        ctx.fillText(verdict.covered + " / " + verdict.total, 0, -6);
        ctx.font = "700 8px monospace";
        ctx.fillStyle = verdict.complete ? P.ringCyan : P.ringAmber;
        ctx.fillText(verdict.complete ? "CLEARED" : "PARTIAL", 0, 7);
      }
    }
    ctx.restore();

    /* 7. Tactical callout for the hovered or active layer. */
    const inspectedLayer = state.hoveredLayer
      ? layerById(state.hoveredLayer)
      : state.active
        ? layerById(state.active)
        : null;
    if (inspectedLayer && geo.w > 640) {
      drawTacticalCallout(ctx, inspectedLayer, P);
    }

    ctx.restore();
  }

  function drawTacticalCallout(ctx, layer, P) {
    const cardW = 210;
    const cardH = 78;
    const cardX = geo.w - cardW - 14;
    const cardY = 14;
    const tone = toneColor(layer.outcome, P);

    ctx.fillStyle = P.isLight
      ? "rgba(255, 255, 255, 0.95)"
      : "rgba(15, 23, 42, 0.95)";
    ctx.strokeStyle = tone;
    ctx.lineWidth = 1.4;
    ctx.beginPath();
    ctx.roundRect(cardX, cardY, cardW, cardH, 8);
    ctx.fill();
    ctx.stroke();

    ctx.font = "700 11.5px Inter, system-ui, sans-serif";
    ctx.fillStyle = P.textPrimary;
    ctx.textAlign = "left";
    ctx.textBaseline = "middle";
    ctx.fillText(layer.full, cardX + 12, cardY + 20);

    ctx.font = "700 9.5px monospace";
    ctx.fillStyle = tone;
    ctx.fillText(
      "STATUS: " + (OUTCOME_TEXT[layer.outcome] || layer.outcome).toUpperCase(),
      cardX + 12,
      cardY + 39
    );

    ctx.font = "500 10px Inter, system-ui, sans-serif";
    ctx.fillStyle = P.textDim;
    ctx.fillText(
      "Inspected: " +
        layer.count +
        " \u00B7 Flagged: " +
        layer.flagged,
      cardX + 12,
      cardY + 58
    );
  }

  /* ---------- Animation loop ------------------------------------------- */
  function frame(now) {
    const dt = Math.min(0.1, (now - lastTime) / 1000);
    lastTime = now;

    if (prefersMotion()) {
      state.ambientAngle = (state.ambientAngle + dt * 0.24) % (Math.PI * 2);
      if (state.running) {
        state.sweepAngle = (state.sweepAngle + dt * 2.8) % (Math.PI * 2);
      }
      for (let i = 0; i < state.layers.length; i += 1) {
        const beacons = state.layers[i].beacons;
        for (let b = 0; b < beacons.length; b += 1) {
          beacons[b].radius += dt * (state.running ? 16 : 6);
          if (beacons[b].radius > 18) beacons[b].radius = 1;
        }
      }
      for (let r = state.ripples.length - 1; r >= 0; r -= 1) {
        const rip = state.ripples[r];
        rip.r += dt * 175;
        rip.alpha -= dt * 0.9;
        if (rip.r >= rip.maxR || rip.alpha <= 0) state.ripples.splice(r, 1);
      }
    }

    draw();

    if (
      prefersMotion() &&
      !document.hidden &&
      field &&
      field.offsetParent !== null
    ) {
      rafId = requestAnimationFrame(frame);
    } else {
      rafId = null;
    }
  }

  function startLoop() {
    if (rafId === null && prefersMotion()) {
      lastTime = performance.now();
      rafId = requestAnimationFrame(frame);
    }
  }

  function stopLoop() {
    if (rafId !== null) {
      cancelAnimationFrame(rafId);
      rafId = null;
    }
  }

  /* ---------- DOM: layer matrix ---------------------------------------- */
  const rowRefs = {};

  function buildLayers() {
    if (!layerList) return;
    layerList.innerHTML = "";
    for (let i = 0; i < state.layers.length; i += 1) {
      const L = state.layers[i];
      const li = document.createElement("li");
      li.className = "xr-layer";
      li.setAttribute("data-layer-id", L.id);

      const mark = document.createElement("span");
      mark.className = "xr-layer-mark";
      mark.setAttribute("aria-hidden", "true");

      const name = document.createElement("span");
      name.className = "xr-layer-name";
      name.textContent = L.full;

      const status = document.createElement("span");
      status.className = "xr-layer-status";

      const count = document.createElement("span");
      count.className = "xr-layer-count";

      const toggle = document.createElement("button");
      toggle.className = "xr-layer-more";
      toggle.type = "button";
      toggle.setAttribute("aria-expanded", "false");
      const detailId = "xr-layer-detail-" + L.id;
      toggle.setAttribute("aria-controls", detailId);
      toggle.textContent = "Why";

      const detail = document.createElement("p");
      detail.className = "xr-layer-detail";
      detail.id = detailId;
      detail.hidden = true;

      li.append(mark, name, status, count, toggle, detail);
      layerList.appendChild(li);

      rowRefs[L.id] = {
        row: li,
        status: status,
        count: count,
        toggle: toggle,
        detail: detail,
      };

      toggle.addEventListener("click", function () {
        const open = state.expanded[L.id] !== true;
        state.expanded[L.id] = open;
        toggle.setAttribute("aria-expanded", String(open));
        detail.hidden = !open;
        toggle.textContent = open ? "Hide" : "Why";
        draw();
      });
      li.addEventListener("mouseenter", function () {
        state.hoveredLayer = L.id;
        draw();
      });
      li.addEventListener("mouseleave", function () {
        if (state.hoveredLayer === L.id) {
          state.hoveredLayer = null;
          draw();
        }
      });
    }
  }

  function countText(L) {
    if (L.id === "processes") {
      return L.flagged > 0 ? L.flagged + " flagged" : "";
    }
    if (L.id === "ransom") {
      return L.flagged > 0 ? L.flagged + " flagged" : "";
    }
    return L.count > 0 ? L.count + (L.flagged > 0 ? " \u00B7 " + L.flagged + "!" : "") : "";
  }

  function detailText(L) {
    if (L.detail) return L.detail;
    if (L.outcome === "pending") return "This collector has not reported yet.";
    if (L.outcome === "running") return L.note + " — collecting now.";
    if (L.outcome === "unreported")
      return (
        "This collector ran and inspected " +
        L.count +
        " entr" +
        (L.count === 1 ? "y" : "ies") +
        ", but the backend sends no per-area coverage row for it, so it is " +
        "listed without a coverage claim."
      );
    if (L.outcome === "checked")
      return "Fully enumerated: " + L.count + " entries inspected.";
    return L.note;
  }

  function paintLayers() {
    for (let i = 0; i < state.layers.length; i += 1) {
      const L = state.layers[i];
      const ref = rowRefs[L.id];
      if (!ref) continue;
      ref.row.setAttribute("data-outcome", L.outcome);
      ref.row.classList.toggle("is-active", state.active === L.id);
      ref.row.classList.toggle("has-findings", L.flagged > 0);
      ref.status.textContent = OUTCOME_TEXT[L.outcome] || L.outcome;
      const ct = countText(L);
      ref.count.textContent = ct;
      ref.count.hidden = ct === "";
      ref.detail.textContent = detailText(L);
      ref.detail.hidden = state.expanded[L.id] !== true;
      ref.toggle.setAttribute(
        "aria-expanded",
        String(state.expanded[L.id] === true)
      );
      ref.toggle.textContent = state.expanded[L.id] === true ? "Hide" : "Why";
    }
  }

  /* ---------- DOM: readouts -------------------------------------------- */
  function setFieldState() {
    if (!field) return;
    field.setAttribute("data-running", state.running ? "true" : "false");
    let value = "idle";
    if (state.phase === "preparing") value = "preparing";
    else if (state.running) value = "scanning";
    else if (state.phase === "failed") value = "failed";
    else if (state.phase === "complete") {
      const v = getVerdict();
      value = v.findings > 0 ? "findings" : v.complete ? "complete" : "partial";
    }
    field.setAttribute("data-state", value);
  }

  function paintReadouts() {
    const v = getVerdict();
    const c = counts();

    if (itemsEl) itemsEl.textContent = state.entries + " entries inspected";
    if (inspectedEl) inspectedEl.textContent = String(state.entries);
    if (findingsEl) findingsEl.textContent = String(state.findings);
    if (coverageEl) coverageEl.textContent = c.covered + " / " + c.total;
    if (matrixCoverEl) {
      matrixCoverEl.textContent = c.covered + " / " + c.total + " checked";
    }
    if (currentEl) {
      currentEl.textContent = state.running
        ? state.lastMessage || "Collecting"
        : v.headline;
    }

    if (liveEl) {
      if (state.running) liveEl.textContent = "Collecting";
      else if (state.phase === "preparing") liveEl.textContent = "Preparing";
      else if (state.phase === "failed") liveEl.textContent = "Collection failed";
      else if (state.phase === "complete")
        liveEl.textContent = v.complete ? "Scan complete" : "Coverage incomplete";
      else liveEl.textContent = "Radar standby";
    }

    if (phaseEl) {
      if (state.running) {
        const act = state.active ? layerById(state.active) : null;
        phaseEl.textContent = act ? act.full : "Collecting persistence layers";
      } else if (state.phase === "failed") {
        phaseEl.textContent = "Collection failed";
      } else if (state.phase === "complete") {
        phaseEl.textContent = v.complete
          ? "All reported layers checked"
          : c.covered + " / " + c.total + " layers report coverage";
      } else {
        phaseEl.textContent = "Idle";
      }
    }
    if (phaseLabelEl) {
      phaseLabelEl.textContent =
        state.phase === "complete" ? "Scan result" : "Current collector";
    }

    if (coreReadout) {
      if (state.phase === "failed") {
        coreReadout.textContent = "COLLECTION FAILED";
      } else if (state.running) {
        coreReadout.textContent =
          "SCANNING // " +
          (state.active
            ? ((layerById(state.active) || {}).label || "COLLECTING").toUpperCase()
            : "COLLECTING");
      } else if (state.phase === "complete") {
        const findings = state.findings;
        const findingsWord = findings === 1 ? "FINDING" : "FINDINGS";
        if (v.complete) {
          coreReadout.textContent =
            findings > 0
              ? findings + " " + findingsWord + " DETECTED"
              : "ALL 9 LAYERS CLEAN";
        } else {
          coreReadout.textContent =
            findings > 0
              ? findings + " " + findingsWord + " · INCOMPLETE"
              : "INCOMPLETE · NO FINDINGS";
        }
      } else {
        coreReadout.textContent = "READY // STANDBY";
      }
    }

    if (hudInspected) hudInspected.textContent = String(state.entries);
    if (hudCoverage) hudCoverage.textContent = c.covered + " / " + c.total;
    if (hudFindings) hudFindings.textContent = String(state.findings);
    if (hudUnavailable)
      hudUnavailable.textContent = String(c.unreadable + c.unreported);

    if (hudBadge) {
      hudBadge.textContent = hudBadgeText(v);
      hudBadge.className = "xray-hud-badge " + hudBadgeClass(v);
    }
    if (hudName) hudName.textContent = v.headline;
    if (hudDetail) hudDetail.textContent = v.detail;

    setFieldState();
  }

  function hudBadgeText(v) {
    if (state.phase === "failed") return "FAILED";
    if (state.running) return state.active ? "COLLECTING" : "SCANNING";
    if (state.phase !== "complete") return "STANDBY";
    if (v.findings > 0) return "REVIEW";
    if (v.complete) return "COMPLETE";
    return "PARTIAL";
  }

  function hudBadgeClass(v) {
    if (state.phase === "failed" || v.findings > 0) return "is-risk";
    if (state.running) return "is-active";
    if (state.phase === "complete" && v.complete) return "is-clean";
    if (state.phase === "complete") return "is-warn";
    return "";
  }

  function render() {
    paintLayers();
    paintReadouts();
    draw();
  }

  /* ---------- Legacy support: mark the previously active collector ----- */
  function settleActive(next) {
    if (!state.active || state.active === next) return;
    const layer = layerById(state.active);
    if (layer && layer.outcome === "running") {
      /* The backend moved on. That is not evidence the layer was checked:
         without a coverage row it is only known to have run. */
      layer.outcome = "unreported";
      triggerShockwave("#22d3ee");
    }
    state.active = null;
  }

  function pushBeacon(layerId, risk) {
    const layer = layerById(layerId);
    if (!layer) return;
    layer.beacons.push({
      angle: Math.random() * Math.PI * 2,
      risk: risk || "Normal",
      radius: 1,
    });
    if (layer.beacons.length > 5) layer.beacons.shift();
    triggerShockwave(risk === "HighRisk" ? "#ef4444" : "#f59e0b");
  }

  /* ---------- Resize / hover / visibility ------------------------------ */
  if (window.ResizeObserver && field) {
    const ro = new ResizeObserver(function () {
      layout();
    });
    ro.observe(field);
  } else {
    window.addEventListener("resize", layout);
  }

  if (canvas) {
    canvas.addEventListener("mousemove", function (ev) {
      const rect = canvas.getBoundingClientRect();
      const dx = ev.clientX - rect.left - geo.cx;
      const dy = ev.clientY - rect.top - geo.cy;
      const dist = Math.sqrt(dx * dx + dy * dy);
      if (dist >= geo.innerR && dist <= geo.outerR + 10) {
        const ringIdx = Math.min(
          8,
          Math.floor((dist - geo.innerR) / geo.ringStep)
        );
        if (state.layers[ringIdx]) {
          state.hoveredLayer = state.layers[ringIdx].id;
          draw();
          return;
        }
      }
      if (state.hoveredLayer !== null) {
        state.hoveredLayer = null;
        draw();
      }
    });

    canvas.addEventListener("click", function (ev) {
      const rect = canvas.getBoundingClientRect();
      const dx = ev.clientX - rect.left - geo.cx;
      const dy = ev.clientY - rect.top - geo.cy;
      const dist = Math.sqrt(dx * dx + dy * dy);
      if (dist >= geo.innerR && dist <= geo.outerR + 10) {
        const ringIdx = Math.min(
          8,
          Math.floor((dist - geo.innerR) / geo.ringStep)
        );
        const layer = state.layers[ringIdx];
        if (layer) {
          state.hoveredLayer = layer.id;
          draw();
          const el =
            layerList &&
            layerList.querySelector('[data-layer-id="' + layer.id + '"]');
          if (el && el.scrollIntoView) {
            el.scrollIntoView({ behavior: "smooth", block: "nearest" });
          }
        }
      }
    });

    canvas.addEventListener("mouseleave", function () {
      if (state.hoveredLayer !== null) {
        state.hoveredLayer = null;
        draw();
      }
    });
  }

  if (motionBtn) {
    motionBtn.addEventListener("click", function () {
      window.CureSweep.motion(!motionPaused);
    });
    motionBtn.textContent = motionPaused ? "Resume field motion" : "Pause field motion";
    motionBtn.setAttribute("aria-pressed", String(motionPaused));
  }

  document.addEventListener("visibilitychange", function () {
    if (document.hidden) stopLoop();
    else if (state.running) startLoop();
    else draw();
  });

  /* ---------- Public API ------------------------------------------------ */
  window.CureSweep = {
    start: function () {
      state.phase = "scanning";
      state.active = null;
      state.entries = 0;
      state.findings = 0;
      state.lastMessage = "";
      state.running = true;
      state.hoveredLayer = null;
      state.expanded = {};
      state.ripples = [];

      for (let i = 0; i < state.layers.length; i += 1) {
        const L = state.layers[i];
        L.outcome = "pending";
        L.ran = false;
        L.count = 0;
        L.flagged = 0;
        L.detail = "";
        L.beacons = [];
      }

      render();
      layout();
      startLoop();
    },

    /* Preparing is a real phase: the overlay review runs before collection.
       It is not scanning yet, and nothing claims it is. */
    preparing: function () {
      state.phase = "preparing";
      state.running = false;
      state.active = null;
      state.lastMessage = "";
      for (let i = 0; i < state.layers.length; i += 1) {
        const L = state.layers[i];
        L.outcome = "pending";
        L.ran = false;
        L.count = 0;
        L.flagged = 0;
        L.detail = "";
        L.beacons = [];
      }
      state.entries = 0;
      state.findings = 0;
      render();
    },

    progress: function (p) {
      if (!p || !p.stage) return;

      if (p.stage === "source-state") {
        for (let i = 0; i < state.layers.length; i += 1) {
          const L = state.layers[i];
          if (!layerMatchesArea(L, p.area)) continue;
          settleActive(null);
          L.outcome = outcomeFromCoverage(p.state);
          L.detail = p.detail ? String(p.detail) : "";
          const skipped = skippedCount(p.state, p.detail);
          if (L.outcome === "partial" && skipped > 0 && !/skipped/i.test(L.detail)) {
            L.detail = skipped + " item(s) skipped · " + L.detail;
          }
          if (
            L.outcome === "checked" ||
            L.outcome === "partial" ||
            L.outcome === "denied" ||
            L.outcome === "failed"
          ) {
            L.ran = true;
          }
          if (L.outcome === "partial") triggerShockwave("#f59e0b");
          else if (L.outcome === "denied" || L.outcome === "failed")
            triggerShockwave("#ef4444");
          else triggerShockwave("#22d3ee");
        }
        state.lastMessage = p.area + " · " + (OUTCOME_TEXT[outcomeFromCoverage(p.state)] || "");
        render();
        return;
      }

      const stageId = STAGE_ALIAS[p.stage] || p.stage;
      const layer = layerById(stageId);

      if (p.stage === "scoring") {
        settleActive(null);
        if (p.message) state.lastMessage = p.message;
        render();
        return;
      }

      if (layer) {
        settleActive(layer.id);
        state.active = layer.id;
        layer.ran = true;
        /* Never repaint a layer the backend already resolved. */
        if (layer.outcome === "pending" || layer.outcome === "running") {
          layer.outcome = "running";
        }
      }

      if (p.stage === "item-scanned") {
        const id = ITEM_SOURCE[p.source] || null;
        const source = id ? layerById(id) : null;
        state.entries += 1;
        if (source) {
          settleActive(source.id);
          state.active = source.id;
          source.count += 1;
          if (p.risk && p.risk !== "Safe") {
            source.flagged += 1;
            state.findings += 1;
            pushBeacon(source.id, p.risk);
          }
        }
        if (p.name) {
          state.lastMessage =
            "Inspecting " + p.name + (source ? " · " + source.label : "");
        }
      } else if (p.stage === "process-flagged") {
        const proc = layerById("processes");
        if (proc) {
          proc.flagged += 1;
          state.findings += 1;
          pushBeacon("processes", p.risk);
        }
        state.lastMessage =
          "Process flagged: " + (p.name || "unknown") + " (" + (p.risk || "") + ")";
      } else if (p.stage === "ransom-found") {
        const ransom = layerById("ransom");
        if (ransom) {
          ransom.flagged += 1;
          state.findings += 1;
          pushBeacon("ransom", "HighRisk");
        }
        state.lastMessage =
          "Ransom indicator: " + (p.finding_type || "unknown");
      } else if (p.message) {
        state.lastMessage = p.message;
      }

      render();
      if (rafId === null) startLoop();
    },

    finish: function (summary) {
      settleActive(null);
      state.active = null;
      state.running = false;
      state.phase = "complete";

      /* The summary is authoritative for the entry count. */
      const total =
        summary && Number.isFinite(Number(summary.total))
          ? Number(summary.total)
          : state.entries;
      state.entries = total;

      /* Findings come from the summary too, so a dropped UI event cannot
         leave the readout lower than the backend's own result. */
      if (summary) {
        const review = (summary.suspicious_for_review || []).length;
        const procs = (summary.process_findings || []).length;
        const ransom = (summary.ransom_findings || []).length;
        if (review + procs + ransom > 0) {
          state.findings = review + procs + ransom;
        }
      }

      /* Reconcile every layer with the backend's coverage rows. */
      const rows = (summary && summary.source_states) || [];
      for (let i = 0; i < state.layers.length; i += 1) {
        const L = state.layers[i];
        const row = rows.find(function (r) {
          return r && layerMatchesArea(L, r.area);
        });
        if (row) {
          L.outcome = outcomeFromCoverage(row.state);
          L.detail = row.detail ? String(row.detail) : "";
          if (L.outcome === "pending") {
            /* Backend reported the row but with no usable state. */
            L.detail =
              L.detail || "backend reported no coverage state for this area";
          }
        } else {
          // If already resolved (e.g. by prior event or test fixture), preserve it!
          if (
            L.outcome === "checked" ||
            L.outcome === "partial" ||
            L.outcome === "denied" ||
            L.outcome === "failed" ||
            L.outcome === "unsupported"
          ) {
            // Keep resolved outcome
          } else if (L.ran || L.count > 0) {
            L.outcome = "unreported";
            L.detail =
              "This collector ran and inspected " +
              L.count +
              " entr" +
              (L.count === 1 ? "y" : "ies") +
              ", but the backend sent no per-area coverage row for it.";
          } else {
            L.outcome = "unreported";
            L.detail = "The backend sent no coverage row for this layer.";
          }
        }
      }

      state.lastMessage = state.entries + " entries inspected";
      render();
      if (prefersMotion()) startLoop();
    },

    fail: function (reason) {
      settleActive(null);
      state.active = null;
      state.running = false;
      state.phase = "failed";
      state.lastMessage = reason ? String(reason) : "The scan did not finish.";
      /* A failed run leaves no per-layer claim behind. */
      for (let i = 0; i < state.layers.length; i += 1) {
        const L = state.layers[i];
        if (L.outcome === "running" || L.outcome === "pending") {
          L.outcome = "failed";
          L.detail = "Collection stopped before this layer reported.";
        }
      }
      render();
      stopLoop();
    },

    motion: function (paused) {
      motionPaused = !!paused;
      try {
        localStorage.setItem("cure-motion-paused", String(motionPaused));
      } catch (_) {
        /* storage may be unavailable; the toggle still works this session */
      }
      if (motionBtn) {
        motionBtn.textContent = motionPaused
          ? "Resume field motion"
          : "Pause field motion";
        motionBtn.setAttribute("aria-pressed", String(motionPaused));
      }
      if (motionPaused || reducedMedia.matches || document.hidden) {
        stopLoop();
        draw();
      } else {
        startLoop();
      }
    },

    motionPaused: function () {
      return motionPaused;
    },

    layout: function () {
      layout();
      startLoop();
    },

    /* State model access for testing and verification harnesses. */
    getState: function () {
      return state;
    },

    getVerdict: getVerdict,
  };

  buildLayers();
  render();
  layout();
  if (prefersMotion()) startLoop();
})();
