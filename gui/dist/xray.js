/* ==========================================================================
   HOLOGRAPHIC SECURITY RADAR — Concept A Next-Gen Rescue Scanner
   Concentric 9-layer orbital rings, dynamic phosphor sweep, real-time evidence
   beacons, and interactive telemetry nodes.
   Driven 100% by real Rust backend events (CureSweep).
   Offline, Canvas 2D, zero external assets, reduced-motion compliant.
   ========================================================================== */
(function () {
  "use strict";

  /* ---------- The Nine Persistence Layers --------------------------------- */
  const LAYERS = [
    { id: "registry",  label: "Registry",      full: "Registry autoruns",             area: "Registry autoruns" },
    { id: "startup",   label: "Startup",       full: "Startup folders",               area: null },
    { id: "tasks",     label: "Tasks",         full: "Scheduled tasks",               area: "Scheduled tasks" },
    { id: "services",  label: "Services",      full: "Auto-start services",           area: "Services (auto-start)" },
    { id: "wmi",       label: "WMI",           full: "WMI subscriptions",             area: "WMI subscriptions" },
    { id: "ifeo",      label: "IFEO / AppInit",full: "IFEO debuggers & AppInit DLLs", area: null },
    { id: "com",       label: "COM",           full: "COM registrations",             area: null },
    { id: "processes", label: "Processes",     full: "Running processes",             area: null },
    { id: "ransom",    label: "Ransom",        full: "Ransom indicators",             area: null },
  ];

  const STAGE_ALIAS = {
    "startup-common": "startup",
    "process-scan": "processes",
    "process": "processes",
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

  function coverageTone(raw) {
    const key = typeof raw === "string" ? raw : Object.keys(raw || {})[0] || "NotChecked";
    if (key === "Checked" || key === "Available") return "checked";
    if (key === "CheckFailed") return "bad";
    if (key === "AccessDenied") return "warn";
    if (key === "Partial") return "partial";
    return "idle";
  }

  function coverageText(raw) {
    const key = typeof raw === "string" ? raw : Object.keys(raw || {})[0] || "NotChecked";
    if (key === "Checked") return "Checked";
    if (key === "Available") return "Available";
    if (key === "AccessDenied") return "Access denied";
    if (key === "CheckFailed") return "Check failed";
    if (key === "Partial") return "Partial";
    if (key === "Unavailable") return "Unavailable";
    return "Not checked";
  }

  /* ---------- DOM References ---------------------------------------------- */
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

  /* ---------- State Model ------------------------------------------------- */
  const state = {
    running: false,
    ambientAngle: 0,
    active: null,
    items: 0,
    findings: 0,
    activity: 0,
    sweepAngle: -Math.PI / 2,
    hoveredLayer: null,
    selectedLayer: null,
    layers: LAYERS.map(function (L) {
      return {
        id: L.id,
        label: L.label,
        full: L.full,
        tone: "pending",
        text: "Ready for inspection",
        count: 0,
        risk: 0,
        detail: "",
        beacons: [],
      };
    }),
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

  /* ---------- Motion & Visibility Control --------------------------------- */
  let motionPaused = reducedMedia.matches;
  let rafId = null;
  let lastTime = performance.now();

  function prefersMotion() {
    return !motionPaused && !reducedMedia.matches;
  }

  /* ---------- Geometry & Coordinate Cache --------------------------------- */
  const geo = {
    w: 800,
    h: 460,
    cx: 400,
    cy: 230,
    dpr: 1,
    innerR: 44,
    outerR: 200,
    ringStep: 16,
    orbitAngles: LAYERS.map(function (_, i) {
      return (i * (Math.PI * 2 / 9)) - (Math.PI / 2);
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

    // Radar Center: Positioned slightly left-of-center for balanced tactical layout
    const isWide = geo.w > 720;
    geo.cx = isWide ? Math.round(geo.w * 0.44) : Math.round(geo.w * 0.5);
    geo.cy = Math.round(geo.h * 0.5);

    const maxR = Math.min(geo.w * 0.42, geo.h * 0.44);
    geo.innerR = Math.max(34, Math.round(maxR * 0.22));
    geo.outerR = Math.max(140, Math.round(maxR * 0.94));
    geo.ringStep = (geo.outerR - geo.innerR) / 9;

    if (coreReadout) {
      coreReadout.style.left = geo.cx + "px";
      coreReadout.style.top = Math.min(geo.h - 22, Math.round(geo.cy + geo.outerR + 14)) + "px";
      if (!state.running && state.items === 0) {
        coreReadout.textContent = "STANDBY // READY";
      }
    }
    const liveTag = document.getElementById("xray-live");
    if (liveTag && !state.running && state.items === 0) {
      liveTag.textContent = "Radar Standby";
    }

    draw();
  }

  /* ---------- Palette & Colors -------------------------------------------- */
  function getPalette() {
    const isLight = document.documentElement.getAttribute("data-theme") === "light";
    return {
      isLight: isLight,
      bg: isLight ? "rgba(248, 250, 252, 0.96)" : "rgba(11, 13, 19, 0.96)",
      ringBase: isLight ? "rgba(71, 85, 105, 0.18)" : "rgba(148, 163, 184, 0.12)",
      ringActive: isLight ? "#7c3aed" : "#a78bfa",
      ringCyan: isLight ? "#0891b2" : "#22d3ee",
      ringAmber: isLight ? "#d97732" : "#f59e0b",
      ringRed: isLight ? "#dc2626" : "#ef4444",
      sweepFill: isLight ? "rgba(124, 58, 237, 0.12)" : "rgba(139, 111, 240, 0.15)",
      sweepLine: isLight ? "rgba(8, 145, 178, 0.8)" : "rgba(34, 211, 238, 0.9)",
      hubBody: isLight ? "#e2e8f0" : "#131722",
      hubBorder: isLight ? "rgba(124, 58, 237, 0.4)" : "rgba(167, 139, 250, 0.4)",
      textPrimary: isLight ? "#0f172a" : "#f1f5f9",
      textDim: isLight ? "#64748b" : "#94a3b8",
      glowCyan: isLight ? "rgba(8, 145, 178, 0.25)" : "rgba(34, 211, 238, 0.35)",
    };
  }

  /* ---------- Canvas Rendering ------------------------------------------- */
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

    // 1. Tactical Crosshairs & Polar Radial Guides
    ctx.lineWidth = 1;
    ctx.strokeStyle = P.ringBase;
    ctx.setLineDash([3, 5]);

    // Horizontal & Vertical Axes
    ctx.beginPath();
    ctx.moveTo(cx - geo.outerR - 18, cy);
    ctx.lineTo(cx + geo.outerR + 18, cy);
    ctx.moveTo(cx, cy - geo.outerR - 18);
    ctx.lineTo(cx, cy + geo.outerR + 18);
    ctx.stroke();

    // 45-degree tactical diagonals
    const dLen = geo.outerR + 10;
    const dCos = Math.cos(Math.PI / 4) * dLen;
    ctx.beginPath();
    ctx.moveTo(cx - dCos, cy - dCos); ctx.lineTo(cx + dCos, cy + dCos);
    ctx.moveTo(cx - dCos, cy + dCos); ctx.lineTo(cx + dCos, cy - dCos);
    ctx.stroke();
    ctx.setLineDash([]);

    // Ambient Rotating Compass Reticle (Outer Tick Ring)
    ctx.save();
    ctx.translate(cx, cy);
    ctx.rotate(state.ambientAngle);
    ctx.strokeStyle = P.isLight ? "rgba(8, 145, 178, 0.22)" : "rgba(34, 211, 238, 0.28)";
    ctx.lineWidth = 1;
    const outerTickR = geo.outerR + 14;
    for (let t = 0; t < 12; t += 1) {
      const a = (t * Math.PI) / 6;
      ctx.beginPath();
      ctx.moveTo(Math.cos(a) * (outerTickR - 4), Math.sin(a) * (outerTickR - 4));
      ctx.lineTo(Math.cos(a) * outerTickR, Math.sin(a) * outerTickR);
      ctx.stroke();
    }
    ctx.restore();

    // 2. Concentric 9 Orbital Rings (One for each Persistence Collector)
    for (let i = 0; i < 9; i += 1) {
      const r = geo.innerR + (i + 1) * geo.ringStep;
      const layer = state.layers[i];
      const isActive = state.active === layer.id;
      const isHovered = state.hoveredLayer === layer.id;

      ctx.beginPath();
      ctx.arc(cx, cy, r, 0, Math.PI * 2);

      // Ring Styling based on real collector tone
      if (layer.tone === "highrisk") {
        ctx.strokeStyle = P.ringRed;
        ctx.lineWidth = 2.2;
        ctx.stroke();
      } else if (layer.tone === "review") {
        ctx.strokeStyle = P.ringAmber;
        ctx.lineWidth = 2;
        ctx.stroke();
      } else if (layer.tone === "checked" || layer.tone === "available") {
        ctx.strokeStyle = P.ringCyan;
        ctx.lineWidth = isActive ? 2.4 : 1.4;
        ctx.stroke();
      } else if (layer.tone === "warn" || layer.tone === "partial") {
        ctx.strokeStyle = P.ringAmber;
        ctx.lineWidth = 1.4;
        ctx.setLineDash([6, 4]);
        ctx.stroke();
        ctx.setLineDash([]);
      } else if (layer.tone === "bad") {
        ctx.strokeStyle = P.ringRed;
        ctx.lineWidth = 1.5;
        ctx.setLineDash([4, 4]);
        ctx.stroke();
        ctx.setLineDash([]);
      } else if (isActive) {
        ctx.strokeStyle = P.ringActive;
        ctx.lineWidth = 2.4;
        ctx.stroke();
      } else {
        // Pending / Idle
        ctx.strokeStyle = isHovered ? P.ringCyan : P.ringBase;
        ctx.lineWidth = isHovered ? 1.6 : 1;
        ctx.stroke();
      }

      // Orbital Layer Micro-Node / Tag
      const angle = geo.orbitAngles[i];
      const nx = cx + Math.cos(angle) * r;
      const ny = cy + Math.sin(angle) * r;

      ctx.beginPath();
      ctx.arc(nx, ny, isActive ? 4 : 2.5, 0, Math.PI * 2);
      ctx.fillStyle = isActive ? P.ringActive : (layer.tone === "review" ? P.ringAmber : (layer.tone === "highrisk" ? P.ringRed : P.ringCyan));
      ctx.fill();
    }

    // 3. Dynamic Rotating Phosphor Sweep Cone
    if (state.running && prefersMotion()) {
      const sweepAngle = state.sweepAngle;
      const sweepSpan = Math.PI / 4; // 45-degree phosphor cone

      const grad = ctx.createRadialGradient(cx, cy, geo.innerR, cx, cy, geo.outerR);
      grad.addColorStop(0, P.sweepFill);
      grad.addColorStop(1, "rgba(34, 211, 238, 0)");

      ctx.save();
      ctx.beginPath();
      ctx.moveTo(cx, cy);
      ctx.arc(cx, cy, geo.outerR + 10, sweepAngle - sweepSpan, sweepAngle, false);
      ctx.closePath();
      ctx.fillStyle = grad;
      ctx.fill();

      // Leading sharp scan line
      ctx.beginPath();
      ctx.moveTo(cx, cy);
      ctx.lineTo(cx + Math.cos(sweepAngle) * (geo.outerR + 12), cy + Math.sin(sweepAngle) * (geo.outerR + 12));
      ctx.strokeStyle = P.sweepLine;
      ctx.lineWidth = 1.8;
      ctx.stroke();
      ctx.restore();
    }

    // 4. Real Evidence Beacons on Orbital Rings
    for (let i = 0; i < state.layers.length; i += 1) {
      const layer = state.layers[i];
      if (!layer.beacons || layer.beacons.length === 0) continue;
      const r = geo.innerR + (i + 1) * geo.ringStep;

      for (let b = 0; b < layer.beacons.length; b += 1) {
        const beacon = layer.beacons[b];
        const bx = cx + Math.cos(beacon.angle) * r;
        const by = cy + Math.sin(beacon.angle) * r;

        // Radiating pulse wave
        if (prefersMotion() && beacon.radius > 0) {
          ctx.beginPath();
          ctx.arc(bx, by, beacon.radius, 0, Math.PI * 2);
          ctx.strokeStyle = beacon.risk === "HighRisk" ? "rgba(239,68,68,0.4)" : "rgba(245,158,11,0.4)";
          ctx.lineWidth = 1;
          ctx.stroke();
        }

        // Core Beacon Dot
        ctx.beginPath();
        ctx.arc(bx, by, 4.5, 0, Math.PI * 2);
        ctx.fillStyle = beacon.risk === "HighRisk" ? P.ringRed : P.ringAmber;
        ctx.fill();
        ctx.strokeStyle = "#ffffff";
        ctx.lineWidth = 1.2;
        ctx.stroke();
      }
    }

    // 5. Central Holographic Core Hub (Hexagonal Cybernetic Unit)
    const hubR = geo.innerR;
    ctx.save();
    ctx.translate(cx, cy);

    // Subtle ambient breathing core glow
    if (state.running) {
      const glow = ctx.createRadialGradient(0, 0, hubR * 0.2, 0, 0, hubR * 1.8);
      glow.addColorStop(0, P.glowCyan);
      glow.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = glow;
      ctx.beginPath();
      ctx.arc(0, 0, hubR * 1.8, 0, Math.PI * 2);
      ctx.fill();
    }

    // Hexagonal Plate
    ctx.beginPath();
    for (let h = 0; h < 6; h += 1) {
      const a = (h * Math.PI) / 3;
      const hx = Math.cos(a) * hubR;
      const hy = Math.sin(a) * hubR;
      if (h === 0) ctx.moveTo(hx, hy);
      else ctx.lineTo(hx, hy);
    }
    ctx.closePath();
    ctx.fillStyle = P.hubBody;
    ctx.fill();
    ctx.strokeStyle = P.hubBorder;
    ctx.lineWidth = 1.6;
    ctx.stroke();

    // Rotating dashed inner reticle ring
    ctx.save();
    ctx.rotate(-state.ambientAngle * 1.5);
    ctx.beginPath();
    ctx.arc(0, 0, hubR * 0.72, 0, Math.PI * 2);
    ctx.setLineDash([5, 5]);
    ctx.strokeStyle = P.ringCyan;
    ctx.lineWidth = 1.2;
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.restore();

    // C.U.R.E Center Shield Emblem
    ctx.font = "700 11px Inter, system-ui, sans-serif";
    ctx.fillStyle = P.textPrimary;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText("C.U.R.E", 0, -6);

    ctx.font = "600 8px monospace";
    ctx.fillStyle = P.ringCyan;
    ctx.fillText("RADAR", 0, 7);

    ctx.restore();

    // 6. Interactive Detail Callout Card (if hovering or active)
    const inspectedLayer = state.hoveredLayer ? layerById(state.hoveredLayer) : (state.active ? layerById(state.active) : null);
    if (inspectedLayer && geo.w > 580) {
      drawTacticalCallout(ctx, inspectedLayer, P);
    }

    ctx.restore();
  }

  function drawTacticalCallout(ctx, layer, P) {
    const cardW = 190;
    const cardH = 76;
    const cardX = geo.w - cardW - 14;
    const cardY = 16;

    ctx.fillStyle = P.isLight ? "rgba(255, 255, 255, 0.94)" : "rgba(19, 23, 34, 0.94)";
    ctx.strokeStyle = layer.tone === "review" ? P.ringAmber : (layer.tone === "highrisk" ? P.ringRed : P.ringCyan);
    ctx.lineWidth = 1.2;

    // Rounded rectangle card
    ctx.beginPath();
    ctx.roundRect(cardX, cardY, cardW, cardH, 8);
    ctx.fill();
    ctx.stroke();

    // Card Content
    ctx.font = "700 11px Inter, system-ui, sans-serif";
    ctx.fillStyle = P.textPrimary;
    ctx.textAlign = "left";
    ctx.fillText(layer.full, cardX + 12, cardY + 20);

    ctx.font = "600 10px monospace";
    ctx.fillStyle = layer.tone === "review" ? P.ringAmber : (layer.tone === "highrisk" ? P.ringRed : P.ringCyan);
    ctx.fillText("STATUS: " + layer.text.toUpperCase(), cardX + 12, cardY + 38);

    ctx.font = "500 10px Inter, system-ui, sans-serif";
    ctx.fillStyle = P.textDim;
    ctx.fillText("Inspected: " + layer.count + " · Findings: " + layer.risk, cardX + 12, cardY + 56);
  }

  /* ---------- Animation Loop ---------------------------------------------- */
  function frame(now) {
    const dt = Math.min(0.1, (now - lastTime) / 1000);
    lastTime = now;

    if (prefersMotion()) {
      state.ambientAngle = (state.ambientAngle + dt * 0.22) % (Math.PI * 2);

      if (state.running) {
        // Smooth radar sweep angular velocity (~0.85 rev / sec)
        state.sweepAngle = (state.sweepAngle + dt * 2.8) % (Math.PI * 2);
      }

      // Expand beacon ripple rings
      for (let i = 0; i < state.layers.length; i += 1) {
        const beacons = state.layers[i].beacons;
        for (let b = 0; b < beacons.length; b += 1) {
          beacons[b].radius += dt * (state.running ? 14 : 5);
          if (beacons[b].radius > 16) beacons[b].radius = 1;
        }
      }
    }

    draw();

    // Keep animation loop running when view is visible and motion preferred
    if (prefersMotion() && !document.hidden && field && field.offsetParent !== null) {
      rafId = requestAnimationFrame(frame);
    } else {
      rafId = null;
    }
  }

  function startLoop() {
    if (rafId === null) {
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

  /* ---------- Accessible DOM Ledger Update -------------------------------- */
  function paintLedger() {
    if (!sourceList) return;
    sourceList.innerHTML = "";

    for (let i = 0; i < state.layers.length; i += 1) {
      const L = state.layers[i];
      const li = document.createElement("li");
      li.className = "sweep-source tone-" + L.tone + (state.active === L.id ? " is-active" : "");
      li.setAttribute("data-layer-id", L.id);

      const mark = document.createElement("span");
      mark.className = "source-mark";
      li.appendChild(mark);

      const name = document.createElement("strong");
      name.className = "source-name";
      name.textContent = L.full;
      li.appendChild(name);

      const status = document.createElement("span");
      status.className = "source-status";
      status.textContent = L.text;
      li.appendChild(status);

      if (L.count > 0 || L.risk > 0) {
        const countBadge = document.createElement("span");
        countBadge.className = "source-count" + (L.risk > 0 ? " has-risk" : "");
        countBadge.textContent = L.count + (L.risk > 0 ? " (" + L.risk + " flagged)" : "");
        li.appendChild(countBadge);
      }

      // Click to focus and inspect
      li.addEventListener("click", function () {
        state.hoveredLayer = state.hoveredLayer === L.id ? null : L.id;
        paintLedger();
        draw();
      });

      sourceList.appendChild(li);
    }

    // Global instrument cells
    if (inspectedEl) inspectedEl.textContent = String(state.items);
    if (findingsEl) findingsEl.textContent = String(state.findings);
    if (itemsEl) itemsEl.textContent = state.items + " entries inspected";

    // Coverage indicator: count checked layers
    let checkedCount = 0;
    for (let i = 0; i < state.layers.length; i += 1) {
      const t = state.layers[i].tone;
      if (t === "checked" || t === "available" || t === "review" || t === "highrisk") {
        checkedCount += 1;
      }
    }
    if (coverageEl) {
      coverageEl.textContent = checkedCount + " / " + state.layers.length;
      if (coverageEl.parentElement) {
        coverageEl.parentElement.setAttribute("data-coverage",
          checkedCount === state.layers.length ? "complete" : "incomplete");
      }
    }
  }

  function completeActive() {
    if (!state.active) return;
    const layer = layerById(state.active);
    if (layer && layer.tone === "active") {
      layer.tone = layer.risk > 0 ? "review" : "checked";
      layer.text = layer.risk > 0 ? "Findings available" : "Completed";
    }
  }

  function pushBeacon(layerId, risk) {
    const layer = layerById(layerId);
    if (!layer) return;
    const angle = (Math.random() * Math.PI * 2);
    layer.beacons.push({ angle: angle, risk: risk || "Normal", radius: 1 });
    if (layer.beacons.length > 5) layer.beacons.shift();
  }

  /* ---------- Event Handlers & Resize Observer ---------------------------- */
  if (window.ResizeObserver && field) {
    const ro = new ResizeObserver(function () {
      layout();
    });
    ro.observe(field);
  } else {
    window.addEventListener("resize", layout);
  }

  // Hover detection over Canvas
  if (canvas) {
    canvas.addEventListener("mousemove", function (ev) {
      const rect = canvas.getBoundingClientRect();
      const x = ev.clientX - rect.left;
      const y = ev.clientY - rect.top;
      const dx = x - geo.cx;
      const dy = y - geo.cy;
      const dist = Math.sqrt(dx * dx + dy * dy);

      if (dist >= geo.innerR && dist <= geo.outerR + 10) {
        const ringIdx = Math.min(8, Math.floor((dist - geo.innerR) / geo.ringStep));
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
      const x = ev.clientX - rect.left;
      const y = ev.clientY - rect.top;
      const dx = x - geo.cx;
      const dy = y - geo.cy;
      const dist = Math.sqrt(dx * dx + dy * dy);

      if (dist >= geo.innerR && dist <= geo.outerR + 10) {
        const ringIdx = Math.min(8, Math.floor((dist - geo.innerR) / geo.ringStep));
        if (state.layers[ringIdx]) {
          state.selectedLayer = state.layers[ringIdx].id;
          state.hoveredLayer = state.layers[ringIdx].id;
          paintLedger();
          draw();
          const el = sourceList && sourceList.querySelector('[data-layer-id="' + state.layers[ringIdx].id + '"]');
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

  // Motion button toggle
  if (motionBtn) {
    motionBtn.addEventListener("click", function () {
      window.CureSweep.motion(!motionPaused);
    });
    motionBtn.textContent = motionPaused ? "Resume field motion" : "Pause field motion";
    motionBtn.setAttribute("aria-pressed", String(motionPaused));
  }

  // Visibility Change: sleep when page or tab hidden
  document.addEventListener("visibilitychange", function () {
    if (document.hidden) stopLoop();
    else if (state.running) startLoop();
  });

  /* ---------- Public API (100% Backward Compatible) ----------------------- */
  window.CureSweep = {
    start: function () {
      state.active = null;
      state.items = 0;
      state.findings = 0;
      state.running = true;
      state.hoveredLayer = null;

      for (let i = 0; i < state.layers.length; i += 1) {
        state.layers[i].tone = "pending";
        state.layers[i].text = "Pending";
        state.layers[i].count = 0;
        state.layers[i].risk = 0;
        state.layers[i].detail = "";
        state.layers[i].beacons = [];
      }

      const liveEl = document.getElementById("xray-live");
      if (liveEl) liveEl.textContent = "Active Sweep";
      if (currentEl) currentEl.textContent = "Collecting persistence layers…";
      const phaseLbl = document.getElementById("xray-phase-label");
      if (phaseLbl) phaseLbl.textContent = "Current collector";
      if (coreReadout) coreReadout.textContent = "RADAR // SCANNING";
      if (phaseEl) phaseEl.textContent = "Scanning persistence layers";

      // Notify LUMA companion
      if (window.CureCompanion) window.CureCompanion.scan("scan", "Collection started");

      paintLedger();
      layout();
      startLoop();
    },

    progress: function (p) {
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
        known.text = "Collecting";
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
            pushBeacon(layer.id, p.risk);
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
          pushBeacon("processes", p.risk);
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
          pushBeacon("ransom", "HighRisk");
        }
      }

      const message = p.message || (p.stage === "item-scanned" ? "Inspecting: " + p.name : "Collecting evidence");
      if (currentEl) currentEl.textContent = message;
      if (coreReadout) {
        if (state.active) {
          const actL = layerById(state.active);
          coreReadout.textContent = actL ? "SCANNING // " + actL.label.toUpperCase() : "RADAR // SCANNING";
        } else {
          coreReadout.textContent = "RADAR // SCANNING";
        }
      }
      if (phaseEl && state.active) {
        const activeL = layerById(state.active);
        phaseEl.textContent = activeL ? activeL.full : message;
      }

      paintLedger();
      if (rafId === null) startLoop();
    },

    finish: function (summary) {
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
        const row = rows.find(function (r) { return r.area === LAYERS[i].area; });
        if (row) {
          layer.tone = coverageTone(row.state);
          layer.text = coverageText(row.state);
        }
      }

      // Derive honest coverage summary from both radar layers and backend source_states
      var checkedLayers = 0;
      var totalLayers = state.layers.length;
      var anyFailed = false;
      var anyDenied = false;
      var anyPartial = false;
      for (var ci = 0; ci < totalLayers; ci += 1) {
        var lt = state.layers[ci].tone;
        if (lt === "checked" || lt === "available" || lt === "review" || lt === "highrisk") {
          checkedLayers += 1;
        }
        if (lt === "bad") anyFailed = true;
        if (lt === "warn") anyDenied = true;
        if (lt === "partial") anyPartial = true;
      }

      // Verify backend source_states directly: any non-Available/non-Checked means incomplete
      var backendIncomplete = false;
      if (!rows.length) {
        backendIncomplete = true;
      } else {
        for (var bi = 0; bi < rows.length; bi += 1) {
          var bState = rows[bi].state;
          if (bState !== "Available" && bState !== "Checked") {
            backendIncomplete = true;
            if (bState === "CheckFailed") anyFailed = true;
            if (bState === "AccessDenied") anyDenied = true;
            if (bState === "Partial") anyPartial = true;
          }
        }
      }

      var allCovered = (checkedLayers === totalLayers);
      var coverageComplete = allCovered && !backendIncomplete && !anyFailed && !anyDenied && !anyPartial;

      const liveEl = document.getElementById("xray-live");
      if (liveEl) {
        liveEl.textContent = anyFailed ? "Collection Failed" : (coverageComplete ? "Radar Complete" : "Coverage Incomplete");
      }
      if (currentEl) {
        currentEl.textContent = (anyFailed ? "Scan failed" : "Scan complete") + " · " + (summary ? summary.total : state.items) + " entries" +
          (coverageComplete ? "" : " · " + checkedLayers + "/" + totalLayers + " layers covered");
      }
      const phaseLblDone = document.getElementById("xray-phase-label");
      if (phaseLblDone) phaseLblDone.textContent = "Scan result";
      // HONEST core readout: strictly distinguish the 5 scan outcome states
      if (coreReadout) {
        if (anyFailed) {
          // 1. Failed or cancelled
          coreReadout.textContent = "COLLECTION FAILED";
        } else if (state.findings > 0 && !coverageComplete) {
          // 2. Incomplete with findings
          coreReadout.textContent = state.findings + (state.findings === 1 ? " FINDING · INCOMPLETE" : " FINDINGS · INCOMPLETE");
        } else if (state.findings > 0) {
          // 3. Completed with findings
          coreReadout.textContent = state.findings + (state.findings === 1 ? " FINDING DETECTED" : " FINDINGS DETECTED");
        } else if (!coverageComplete) {
          // 4. Incomplete with no observed findings — NEVER claim clean!
          coreReadout.textContent = "INCOMPLETE · NO FINDINGS";
        } else {
          // 5. Completed with no observed findings (only when every layer verified)
          coreReadout.textContent = "ALL " + totalLayers + " LAYERS CLEAN";
        }
      }
      if (phaseEl) {
        phaseEl.textContent = anyFailed
          ? "Collection failed · review required"
          : (coverageComplete
            ? "Inspection complete · all " + totalLayers + " layers checked"
            : "Coverage incomplete · " + checkedLayers + "/" + totalLayers + " layers checked");
      }

      // Honest LUMA reaction — never claim "inspected cleanly" with incomplete coverage
      if (window.Luma) {
        if (anyFailed) {
          window.Luma.warning("Collection failed — review required");
        } else if (state.findings > 0) {
          window.Luma.curious(state.findings + " finding(s) ready for review");
        } else if (coverageComplete) {
          window.Luma.success("All persistence vectors inspected cleanly");
        } else {
          window.Luma.warning("Coverage incomplete — uninspected areas remain");
        }
      }

      paintLedger();
      draw();
      if (prefersMotion()) startLoop();
    },

    fail: function () {
      completeActive();
      state.active = null;
      state.running = false;
      const liveEl = document.getElementById("xray-live");
      if (liveEl) liveEl.textContent = "Collection Failed";
      const phaseLblFail = document.getElementById("xray-phase-label");
      if (phaseLblFail) phaseLblFail.textContent = "Scan status";
      if (currentEl) currentEl.textContent = "Scan interrupted or failed";
      if (coreReadout) coreReadout.textContent = "COLLECTION FAILED";
      if (phaseEl) phaseEl.textContent = "Check failed";
      if (window.Luma) window.Luma.warning("Scan could not complete");
      paintLedger();
      draw();
      stopLoop();
    },

    motion: function (paused) {
      motionPaused = !!paused;
      try {
        localStorage.setItem("cure-motion-paused", String(motionPaused));
      } catch (_) {}
      if (motionBtn) {
        motionBtn.textContent = motionPaused ? "Resume field motion" : "Pause field motion";
        motionBtn.setAttribute("aria-pressed", String(motionPaused));
      }
      if (motionPaused || reducedMedia.matches || document.hidden) {
        stopLoop();
        draw();
      } else if (state.running) {
        startLoop();
      }
    },

    motionPaused: function () {
      return motionPaused;
    },

    layout: function () {
      layout();
      if (prefersMotion()) startLoop();
    },

    getState: function () {
      return state;
    },
  };

  // Initial layout call
  paintLedger();
  layout();
  if (prefersMotion()) startLoop();
})();
