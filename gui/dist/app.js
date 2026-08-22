(function () {
  "use strict";

  const REDUCED = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  const TAU = window.__TAURI__;
  if (!TAU || !TAU.core || !TAU.event) {
    document.getElementById("fallback").classList.remove("hidden");
    return;
  }
  document.getElementById("app").classList.remove("hidden");

  const invoke = TAU.core.invoke;
  const listen = TAU.event.listen;

  const statusPill = document.getElementById("status-pill");
  const statusText = document.getElementById("status-text");
  const logList = document.getElementById("log");
  const feedCountEl = document.getElementById("feed-count");
  const scanView = document.getElementById("scan-view");
  const resultsView = document.getElementById("results-view");

  let scanToken = 0;
  window.__curePingCount = 0;
  let itemFeedCount = 0;
  let lastItemNode = null;

  function setPill(state, text) {
    statusPill.className = "pill " + state;
    statusText.textContent = text;
  }

  let glitchTimer = null;
  function glitchPillText() {
    if (REDUCED) return;
    statusText.classList.remove("glitch");
    void statusText.offsetWidth;
    statusText.classList.add("glitch");
    clearTimeout(glitchTimer);
    glitchTimer = setTimeout(() => statusText.classList.remove("glitch"), 650);
  }

  let typeChain = Promise.resolve();
  function appendLog(stage, message) {
    const li = document.createElement("li");
    if (stage === "cleaning") li.classList.add("warn-line");
    const tag = document.createElement("b");
    tag.textContent = "[" + stage + "]";
    const body = document.createElement("span");
    li.append(tag, body);
    logList.appendChild(li);
    while (logList.children.length > 200) logList.removeChild(logList.firstChild);

    if (REDUCED) {
      body.textContent = message;
      logList.scrollTop = logList.scrollHeight;
      return;
    }

    const caret = document.createElement("span");
    caret.className = "caret";
    const prev = typeChain;
    typeChain = prev.then(
      () =>
        new Promise((typed) => {
          li.appendChild(caret);
          let shown = 0;
          const step = Math.max(1, Math.round(message.length / 60));
          const tick = () => {
            shown += step;
            body.textContent = message.slice(0, shown);
            logList.scrollTop = logList.scrollHeight;
            if (shown < message.length) {
              setTimeout(tick, 13);
            } else {
              caret.remove();
              typed();
            }
          };
          tick();
        })
    );
  }

  function appendItemLine(p) {
    const li = document.createElement("li");
    li.className = "item-line fresh risk-" + String(p.risk || "Safe").toLowerCase();

    const arrow = document.createElement("span");
    arrow.className = "arrow";
    arrow.textContent = "→";

    const name = document.createElement("span");
    name.className = "iname";
    name.textContent = String(p.name || "?");

    const src = document.createElement("span");
    src.className = "isrc";
    src.textContent = "[" + String(p.source || "?") + "]";

    const risk = document.createElement("span");
    risk.className = "irisk";
    risk.textContent = String(p.risk || "?");

    const score = document.createElement("span");
    score.className = "iscore";
    score.textContent = "(" + String(p.score) + ")";

    li.append(arrow, name, src, risk, score);
    logList.appendChild(li);
    while (logList.children.length > 200) logList.removeChild(logList.firstChild);
    logList.scrollTop = logList.scrollHeight;
    itemFeedCount += 1;
    if (feedCountEl) feedCountEl.textContent = itemFeedCount + " ITEMS";
  }

  // ---- shared network model ----
  // One node/edge data source; the scan-view radar renders it live while the
  // results-view map re-renders it settled at its own scale.
  const TRAVEL = 360;
  const RGB = {
    Safe: [79, 174, 125],
    Suspicious: [209, 161, 63],
    HighRisk: [225, 89, 79],
  };

  const NetStore = (() => {
    const GOLDEN = 2.399963229728653;
    let nodes = [];
    let nodeSeq = 0;
    return {
      reset() {
        nodes.length = 0;
        nodeSeq = Math.floor(Math.random() * 100);
      },
      add(risk, name) {
        const nd = {
          ang: (nodeSeq++ * GOLDEN) % (Math.PI * 2),
          rf: 0.58 + Math.random() * 0.34,
          born: performance.now() - (REDUCED ? TRAVEL : 0),
          risk,
          name: String(name || "?"),
        };
        nodes.push(nd);
        window.__cureNodeCount = (window.__cureNodeCount || 0) + 1;
        return nd;
      },
      all() {
        return nodes;
      },
    };
  })();

  const radar = (() => {
    const stage = document.getElementById("net-stage");
    const canvas = document.getElementById("radar");
    const ctx = canvas.getContext("2d");
    const DPR = Math.min(window.devicePixelRatio || 1, 2);
    const ACCENT = (a) => "rgba(124, 108, 240, " + a + ")";

    let rafId = null;
    let pings = [];
    let pulses = [];
    let lastPulse = 0;
    let W = 0;
    let H = 0;
    let CX = 0;
    let CY = 0;
    let R = 0;
    let RX = 0;

    const motes = [];
    for (let i = 0; i < 10; i++) {
      motes.push({
        rf: 0.16 + Math.random() * 0.3,
        sp: (0.0004 + Math.random() * 0.0008) * (i % 2 ? 1 : -1),
        ph: Math.random() * Math.PI * 2,
        a: 0.12 + Math.random() * 0.16,
      });
    }

    function size() {
      const rect = stage.getBoundingClientRect();
      W = Math.max(1, Math.round(rect.width * DPR));
      H = Math.max(1, Math.round(rect.height * DPR));
      canvas.width = W;
      canvas.height = H;
      CX = W / 2;
      CY = H / 2;
      R = Math.max(12, (Math.min(W, H) / 2) * 0.9);
      RX = Math.max(R, Math.min(W / 2 - 24 * DPR, R * 1.9));
    }
    if (window.ResizeObserver) {
      new ResizeObserver(() => {
        size();
        if (REDUCED && !rafId) drawStaticFrame();
      }).observe(stage);
    } else {
      window.addEventListener("resize", () => {
        size();
        if (REDUCED && !rafId) drawStaticFrame();
      });
    }

    function ring(x, y, r) {
      ctx.beginPath();
      ctx.arc(x, y, Math.max(r, 0.01), 0, Math.PI * 2);
      ctx.stroke();
    }

    function rgba(c, a) {
      return "rgba(" + c[0] + "," + c[1] + "," + c[2] + "," + a + ")";
    }

    function nodeXY(nd) {
      return {
        x: CX + Math.cos(nd.ang) * nd.rf * RX,
        y: CY + Math.sin(nd.ang) * nd.rf * R,
      };
    }

    function ellipse(x, y, rx, ry) {
      ctx.beginPath();
      ctx.ellipse(x, y, Math.max(rx, 0.01), Math.max(ry, 0.01), 0, 0, Math.PI * 2);
      ctx.stroke();
    }

    function drawBackdrop(now) {
      const M = Math.max(RX, R);
      const vg = ctx.createRadialGradient(CX, CY, R * 0.15, CX, CY, M * 1.35);
      vg.addColorStop(0, "rgba(10,10,11,0)");
      vg.addColorStop(1, "rgba(10,10,11,0.62)");
      ctx.fillStyle = vg;
      ctx.fillRect(0, 0, W, H);

      const glow = ctx.createRadialGradient(CX, CY, 0, CX, CY, M);
      glow.addColorStop(0, "rgba(124,108,240,0.05)");
      glow.addColorStop(0.45, "rgba(124,108,240,0.018)");
      glow.addColorStop(1, "rgba(124,108,240,0)");
      ctx.fillStyle = glow;
      ctx.fillRect(0, 0, W, H);

      ctx.lineWidth = 1 * DPR;
      ctx.strokeStyle = ACCENT(0.08);
      ellipse(CX, CY, RX, R);
      ctx.strokeStyle = ACCENT(0.055);
      ellipse(CX, CY, RX * 0.66, R * 0.66);
      ctx.strokeStyle = ACCENT(0.038);
      ellipse(CX, CY, RX * 0.33, R * 0.33);

      ctx.strokeStyle = ACCENT(0.03);
      ctx.beginPath();
      ctx.moveTo(CX - RX, CY);
      ctx.lineTo(CX + RX, CY);
      ctx.moveTo(CX, CY - R);
      ctx.lineTo(CX, CY + R);
      ctx.stroke();

      for (const m of motes) {
        const a = m.ph + m.sp * now;
        const x = CX + Math.cos(a) * RX * m.rf;
        const y = CY + Math.sin(a) * R * m.rf;
        ctx.fillStyle = ACCENT(m.a.toFixed(2));
        ctx.beginPath();
        ctx.arc(x, y, 1.3 * DPR, 0, Math.PI * 2);
        ctx.fill();
      }
    }

    function drawCore(now) {
      const k = Math.min(1.5, Math.max(0.85, Math.min(RX, R) / 300));
      const breathe = REDUCED ? 0.5 : 0.5 + 0.5 * Math.sin(now / 850);
      const haloR = (16 + 14 * breathe) * k * DPR;
      const halo = ctx.createRadialGradient(CX, CY, 0, CX, CY, haloR);
      halo.addColorStop(0, ACCENT((0.22 + 0.12 * breathe).toFixed(3)));
      halo.addColorStop(1, ACCENT(0));
      ctx.fillStyle = halo;
      ctx.beginPath();
      ctx.arc(CX, CY, haloR, 0, Math.PI * 2);
      ctx.fill();

      ctx.lineWidth = 1.2 * DPR;
      if (!REDUCED) {
        ctx.strokeStyle = ACCENT(0.5);
        ctx.beginPath();
        ctx.arc(CX, CY, 15 * k * DPR, now / 2400, now / 2400 + 1.15);
        ctx.stroke();
        ctx.strokeStyle = ACCENT(0.26);
        ctx.beginPath();
        ctx.arc(CX, CY, 20 * k * DPR, -now / 3600, -now / 3600 + 0.7);
        ctx.stroke();
      } else {
        ctx.strokeStyle = ACCENT(0.36);
        ring(CX, CY, 15 * k * DPR);
      }

      ctx.fillStyle = "#7c6cf0";
      ctx.shadowColor = "rgba(124, 108, 240, 0.5)";
      ctx.shadowBlur = (4 + 3 * breathe) * DPR;
      ctx.beginPath();
      ctx.arc(CX, CY, (3.4 + 1.1 * breathe) * k * DPR, 0, Math.PI * 2);
      ctx.fill();
      ctx.shadowBlur = 0;
    }

    function edgeAlpha(risk) {
      // uniform violet connectors — risk color lives on the node dots
      return 0.22;
    }

    function labelAlphaFor(risk) {
      if (risk === "HighRisk") return 0.7;
      if (risk === "Suspicious") return 0.55;
      return 0.34;
    }

    function dotRadius(risk) {
      if (risk === "HighRisk") return 3.1;
      if (risk === "Suspicious") return 2.6;
      return 2.2;
    }

    function shortName(nd) {
      let s = nd.name || "";
      if (s.includes("\\")) {
        const parts = s.split("\\");
        s = parts[parts.length - 1];
      }
      if (s.length > 22) s = s.slice(0, 21) + "…";
      return s;
    }

    function showLabel(nd) {
      return NetStore.all().length <= 60 || nd.risk !== "Safe";
    }

    function drawNetwork(now) {
      ctx.textBaseline = "middle";
      for (const nd of NetStore.all()) {
        const t = Math.min((now - nd.born) / TRAVEL, 1);
        const ease = 1 - Math.pow(1 - t, 3);
        const p = nodeXY(nd);
        const c = RGB[nd.risk] || RGB.Safe;
        const gapX = CX + Math.cos(nd.ang) * 12 * DPR;
        const gapY = CY + Math.sin(nd.ang) * 12 * DPR;
        const hx = gapX + (p.x - gapX) * ease;
        const hy = gapY + (p.y - gapY) * ease;

        if (t < 1) {
          ctx.strokeStyle = ACCENT((0.3 * (0.35 + 0.65 * t)).toFixed(3));
          ctx.lineWidth = 1.3 * DPR;
          ctx.beginPath();
          ctx.moveTo(gapX, gapY);
          ctx.lineTo(hx, hy);
          ctx.stroke();
          ctx.fillStyle = "rgba(237,237,239,0.9)";
          ctx.shadowColor = rgba(c, 0.9);
          ctx.shadowBlur = 4 * DPR;
          ctx.beginPath();
          ctx.arc(hx, hy, 2.4 * DPR, 0, Math.PI * 2);
          ctx.fill();
          ctx.shadowBlur = 0;
        } else {
          ctx.strokeStyle = ACCENT(edgeAlpha(nd.risk));
          ctx.lineWidth = 1 * DPR;
          ctx.beginPath();
          ctx.moveTo(gapX, gapY);
          ctx.lineTo(p.x, p.y);
          ctx.stroke();
        }

        const na = t >= 1 ? 1 : Math.max(0, (t - 0.6) / 0.4);
        if (na > 0) {
          const sinceArrival = now - (nd.born + TRAVEL);
          const flash = t >= 1 ? Math.max(0, 1 - sinceArrival / 480) : 0;
          ctx.fillStyle = rgba(c, na.toFixed(2));
          ctx.shadowColor = rgba(c, 0.9);
          ctx.shadowBlur = (2 + flash * 5) * DPR;
          ctx.beginPath();
          ctx.arc(p.x, p.y, dotRadius(nd.risk) * (1 + flash * 0.35) * DPR, 0, Math.PI * 2);
          ctx.fill();
          ctx.shadowBlur = 0;
        }

        if (t >= 1 && showLabel(nd)) {
          const la =
            Math.min((now - (nd.born + TRAVEL)) / 420, 1) *
            labelAlphaFor(nd.risk);
          if (la > 0.01) {
            const right = Math.cos(nd.ang) >= 0;
            ctx.font =
              10 * DPR + 'px ui-monospace, "SF Mono", "Cascadia Code", Consolas, monospace';
            ctx.textAlign = right ? "left" : "right";
            ctx.fillStyle = "rgba(139,139,147," + la.toFixed(2) + ")";
            ctx.shadowColor = "rgba(10,10,11,0.9)";
            ctx.shadowBlur = 4 * DPR;
            ctx.fillText(shortName(nd), p.x + (right ? 1 : -1) * 9 * DPR, p.y);
            ctx.shadowBlur = 0;
          }
        }
      }
    }

    function drawPings(now) {
      pings = pings.filter((p) => now - p.born < 1300);
      for (const p of pings) {
        const pos = nodeXY(p);
        const t = (now - p.born) / 1300;
        ctx.lineWidth = 2.2 * DPR;
        ctx.strokeStyle =
          "rgba(225,89,79," + ((1 - t) * 0.55).toFixed(3) + ")";
        ring(pos.x, pos.y, t * 46 * DPR);
        ctx.lineWidth = 1 * DPR;
        ctx.strokeStyle =
          "rgba(225,89,79," + ((1 - t) * 0.26).toFixed(3) + ")";
        ring(pos.x, pos.y, t * 26 * DPR);
      }
    }

    function frame(now) {
      if (!pulses.length || now - lastPulse > 2800) {
        pulses.push({ born: now });
        lastPulse = now;
      }
      pulses = pulses.filter((p) => now - p.born < 2600);
      ctx.clearRect(0, 0, W, H);
      drawBackdrop(now);
      ctx.lineWidth = 1 * DPR;
      for (const p of pulses) {
        const t = (now - p.born) / 2600;
        ctx.strokeStyle = ACCENT((0.1 * (1 - t)).toFixed(3));
        ellipse(CX, CY, 8 * DPR + (RX - 8 * DPR) * t, 8 * DPR + (R - 8 * DPR) * t);
      }
      drawNetwork(now);
      drawPings(now);
      drawCore(now);
      rafId = requestAnimationFrame(frame);
    }

    function drawStaticFrame() {
      const now = performance.now();
      ctx.clearRect(0, 0, W, H);
      drawBackdrop(now);
      drawNetwork(now);
      for (const p of pings) {
        const pos = nodeXY(p);
        ctx.fillStyle = "rgba(225,89,79,0.55)";
        ctx.beginPath();
        ctx.arc(pos.x, pos.y, 3 * DPR, 0, Math.PI * 2);
        ctx.fill();
      }
      drawCore(now);
    }

    return {
      start() {
        size();
        pings = [];
        pulses = [];
        NetStore.reset();
        lastPulse = performance.now();
        if (REDUCED) {
          if (rafId) cancelAnimationFrame(rafId);
          rafId = null;
          drawStaticFrame();
          return;
        }
        if (!rafId) rafId = requestAnimationFrame(frame);
      },
      stop() {
        if (rafId) cancelAnimationFrame(rafId);
        rafId = null;
        ctx.clearRect(0, 0, canvas.width, canvas.height);
      },
      addNode(risk, name) {
        const nd = NetStore.add(RGB[risk] ? risk : "Safe", name);
        if (REDUCED && !rafId) drawStaticFrame();
        return nd;
      },
      ping(nd) {
        window.__curePingCount += 1;
        pings.push({
          ang: nd ? nd.ang : Math.random() * Math.PI * 2,
          rf: nd ? nd.rf : 0.58 + Math.random() * 0.34,
          born: performance.now(),
        });
        if (REDUCED && !rafId) drawStaticFrame();
      },
    };
  })();

  function countUp(el, target) {
    if (REDUCED || target === 0) {
      el.textContent = String(target);
      return;
    }
    const started = performance.now();
    const tick = (now) => {
      const t = Math.min((now - started) / 800, 1);
      const eased = 1 - Math.pow(1 - t, 3);
      el.textContent = String(Math.round(target * eased));
      if (t < 1) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  }

  // ---- results-view scan map ----
  // Renders the same NetStore data as a settled, ambient "dormant but alive"
  // constellation: slow core breathing + an occasional faint edge traveler.
  const netmap = (() => {
    const stageEl = document.getElementById("map-stage");
    const canvas = document.getElementById("map-canvas");
    const countEl = document.getElementById("map-count");
    if (!stageEl || !canvas || !canvas.getContext) {
      return { show() {}, hide() {} };
    }
    const ctx = canvas.getContext("2d");
    const DPR = Math.min(window.devicePixelRatio || 1, 2);
    const ACCENT = (a) => "rgba(124, 108, 240, " + a + ")";
    const TRAVELER_MS = 1500;

    let rafId = null;
    let travelers = [];
    let lastSpawn = 0;
    let W = 0;
    let H = 0;
    let CX = 0;
    let CY = 0;
    let R = 0;
    let RX = 0;

    const motes = [];
    for (let i = 0; i < 7; i++) {
      motes.push({
        rf: 0.2 + Math.random() * 0.28,
        sp: (0.0003 + Math.random() * 0.0005) * (i % 2 ? 1 : -1),
        ph: Math.random() * Math.PI * 2,
        a: 0.08 + Math.random() * 0.1,
      });
    }

    function size() {
      const rect = stageEl.getBoundingClientRect();
      if (rect.width < 8 || rect.height < 8) return false;
      W = Math.max(1, Math.round(rect.width * DPR));
      H = Math.max(1, Math.round(rect.height * DPR));
      canvas.width = W;
      canvas.height = H;
      CX = W / 2;
      CY = H / 2;
      // the results rail is portrait — derive the vertical radius from the
      // available height so the constellation fills the panel instead of
      // floating as a landscape-biased blob mid-card
      R = Math.max(10, (H / 2 - 8 * DPR) * 0.92);
      RX = Math.max(30, Math.min(W / 2 - 16 * DPR, R * 1.5));
      return true;
    }
    function onResize() {
      if (size()) {
        if (REDUCED || !rafId) drawFrame(performance.now());
      }
    }
    if (window.ResizeObserver) {
      new ResizeObserver(onResize).observe(stageEl);
    } else {
      window.addEventListener("resize", onResize);
    }

    function ring(x, y, r) {
      ctx.beginPath();
      ctx.arc(x, y, Math.max(r, 0.01), 0, Math.PI * 2);
      ctx.stroke();
    }
    function ellipse(x, y, rx, ry) {
      ctx.beginPath();
      ctx.ellipse(x, y, Math.max(rx, 0.01), Math.max(ry, 0.01), 0, 0, Math.PI * 2);
      ctx.stroke();
    }
    function rgba(c, a) {
      return "rgba(" + c[0] + "," + c[1] + "," + c[2] + "," + a + ")";
    }
    function nodeXY(nd) {
      return {
        x: CX + Math.cos(nd.ang) * nd.rf * RX,
        y: CY + Math.sin(nd.ang) * nd.rf * R,
      };
    }
    function edgeAlpha(risk) {
      // uniform violet connectors — risk color lives on the node dots
      return 0.22;
    }
    function labelAlphaFor(risk) {
      if (risk === "HighRisk") return 0.66;
      if (risk === "Suspicious") return 0.52;
      return 0.32;
    }
    function dotRadius(risk) {
      if (risk === "HighRisk") return 2.9;
      if (risk === "Suspicious") return 2.4;
      return 2.0;
    }
    function shortName(nd) {
      let s = nd.name || "";
      if (s.includes("\\")) {
        const parts = s.split("\\");
        s = parts[parts.length - 1];
      }
      const max = W < 340 * DPR ? 13 : 18;
      if (s.length > max) s = s.slice(0, max - 1) + "…";
      return s;
    }
    function showLabel(nd) {
      const n = NetStore.all().length;
      if (nd.risk !== "Safe") return true;
      // narrow rail: risky nodes only, or labels turn to mush
      if (W < 340 * DPR) return false;
      return n <= 24;
    }

    function drawBackdrop(now) {
      const M = Math.max(RX, R);
      const vg = ctx.createRadialGradient(CX, CY, R * 0.2, CX, CY, M * 1.25);
      vg.addColorStop(0, "rgba(10,10,11,0)");
      vg.addColorStop(1, "rgba(10,10,11,0.5)");
      ctx.fillStyle = vg;
      ctx.fillRect(0, 0, W, H);

      const glow = ctx.createRadialGradient(CX, CY, 0, CX, CY, M);
      glow.addColorStop(0, "rgba(124,108,240,0.04)");
      glow.addColorStop(0.5, "rgba(124,108,240,0.014)");
      glow.addColorStop(1, "rgba(124,108,240,0)");
      ctx.fillStyle = glow;
      ctx.fillRect(0, 0, W, H);

      ctx.lineWidth = 1 * DPR;
      ctx.strokeStyle = ACCENT(0.075);
      ellipse(CX, CY, RX, R);
      ctx.strokeStyle = ACCENT(0.05);
      ellipse(CX, CY, RX * 0.62, R * 0.62);

      for (const m of motes) {
        const a = m.ph + m.sp * now;
        const x = CX + Math.cos(a) * RX * m.rf;
        const y = CY + Math.sin(a) * R * m.rf;
        ctx.fillStyle = ACCENT(m.a.toFixed(2));
        ctx.beginPath();
        ctx.arc(x, y, 1.1 * DPR, 0, Math.PI * 2);
        ctx.fill();
      }
    }

    function drawCore(now) {
      const k = Math.min(1.2, Math.max(0.6, Math.min(RX, R) / 260));
      const breathe =
        REDUCED ? 0.5 : 0.5 + 0.5 * Math.sin(now / 2100);
      const haloR = (12 + 9 * breathe) * k * DPR;
      const halo = ctx.createRadialGradient(CX, CY, 0, CX, CY, haloR);
      halo.addColorStop(0, ACCENT((0.18 + 0.1 * breathe).toFixed(3)));
      halo.addColorStop(1, ACCENT(0));
      ctx.fillStyle = halo;
      ctx.beginPath();
      ctx.arc(CX, CY, haloR, 0, Math.PI * 2);
      ctx.fill();

      ctx.lineWidth = 1 * DPR;
      ctx.strokeStyle = ACCENT(REDUCED ? 0.32 : 0.32 + 0.12 * breathe);
      ring(CX, CY, 11 * k * DPR);

      ctx.fillStyle = "#7c6cf0";
      ctx.shadowColor = "rgba(124, 108, 240, 0.45)";
      ctx.shadowBlur = (3 + 2 * breathe) * DPR;
      ctx.beginPath();
      ctx.arc(CX, CY, (2.6 + 0.7 * breathe) * k * DPR, 0, Math.PI * 2);
      ctx.fill();
      ctx.shadowBlur = 0;
    }

    function drawGraph(now) {
      const nodes = NetStore.all();
      ctx.textBaseline = "middle";
      for (const nd of nodes) {
        const p = nodeXY(nd);
        const c = RGB[nd.risk] || RGB.Safe;
        const gapX = CX + Math.cos(nd.ang) * 9 * DPR;
        const gapY = CY + Math.sin(nd.ang) * 9 * DPR;

        ctx.strokeStyle = ACCENT(edgeAlpha(nd.risk));
        ctx.lineWidth = 1 * DPR;
        ctx.beginPath();
        ctx.moveTo(gapX, gapY);
        ctx.lineTo(p.x, p.y);
        ctx.stroke();

        ctx.fillStyle = rgba(c, 0.92);
        ctx.shadowColor = rgba(c, 0.6);
        ctx.shadowBlur = 2.5 * DPR;
        ctx.beginPath();
        ctx.arc(p.x, p.y, dotRadius(nd.risk) * DPR, 0, Math.PI * 2);
        ctx.fill();
        ctx.shadowBlur = 0;

        if (showLabel(nd)) {
          ctx.font =
            9 * DPR + 'px ui-monospace, "SF Mono", "Cascadia Code", Consolas, monospace';
          const label = shortName(nd);
          // default: extend away from the core; flip whenever the label
          // would run off-canvas on its chosen side (measured, not guessed)
          let align = Math.cos(nd.ang) >= 0 ? "left" : "right";
          const lw = ctx.measureText(label).width;
          if (align === "left" && p.x + 8 * DPR + lw > W - 2) {
            align = "right";
          } else if (align === "right" && p.x - 8 * DPR - lw < 2) {
            align = "left";
          }
          ctx.textAlign = align;
          ctx.fillStyle = "rgba(139,139,147," + labelAlphaFor(nd.risk).toFixed(2) + ")";
          ctx.shadowColor = "rgba(10,10,11,0.9)";
          ctx.shadowBlur = 4 * DPR;
          ctx.fillText(label, p.x + (align === "left" ? 1 : -1) * 8 * DPR, p.y);
          ctx.shadowBlur = 0;
        }
      }
    }

    function drawTravelers(now) {
      travelers = travelers.filter((tr) => now - tr.born < TRAVELER_MS);
      for (const tr of travelers) {
        const nd = NetStore.all()[tr.idx];
        if (!nd) continue;
        const t = (now - tr.born) / TRAVELER_MS;
        const ease = 1 - Math.pow(1 - t, 2.2);
        const p = nodeXY(nd);
        const c = RGB[nd.risk] || RGB.Safe;
        const gapX = CX + Math.cos(nd.ang) * 9 * DPR;
        const gapY = CY + Math.sin(nd.ang) * 9 * DPR;
        const x = gapX + (p.x - gapX) * ease;
        const y = gapY + (p.y - gapY) * ease;
        const fade = Math.min(1, t * 4) * Math.min(1, (1 - t) * 3.2);
        ctx.fillStyle = rgba(c, (0.55 * fade).toFixed(3));
        ctx.shadowColor = rgba(c, 0.7);
        ctx.shadowBlur = 4 * DPR;
        ctx.beginPath();
        ctx.arc(x, y, 1.7 * DPR, 0, Math.PI * 2);
        ctx.fill();
        ctx.shadowBlur = 0;
      }
    }

    function spawnTraveler(now) {
      const n = NetStore.all().length;
      if (!n || travelers.length >= 2) return;
      travelers.push({ idx: Math.floor(Math.random() * n), born: now });
    }

    function drawFrame(now) {
      ctx.clearRect(0, 0, W, H);
      drawBackdrop(now);
      drawGraph(now);
      if (!REDUCED) {
        if (now - lastSpawn > 3400 + Math.random() * 1600) {
          spawnTraveler(now);
          lastSpawn = now;
        }
        drawTravelers(now);
      } else {
        travelers = [];
      }
      drawCore(now);
    }

    function loop(now) {
      drawFrame(now);
      rafId = requestAnimationFrame(loop);
    }

    return {
      show(summaryTotal) {
        if (countEl) countEl.textContent = summaryTotal + " NODES";
        travelers = [];
        lastSpawn = performance.now() - 2400;
        if (REDUCED) {
          if (rafId) cancelAnimationFrame(rafId);
          rafId = null;
          size();
          drawFrame(performance.now());
          return;
        }
        size();
        if (!rafId) rafId = requestAnimationFrame(loop);
      },
      hide() {
        if (rafId) cancelAnimationFrame(rafId);
        rafId = null;
        travelers = [];
        ctx.clearRect(0, 0, canvas.width, canvas.height);
      },
    };
  })();

  const SOURCE_ICONS = {
    StartupFolder:
      '<svg viewBox="0 0 24 24"><path d="M14 2H7a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V7z"/><path d="M14 2v5h5"/></svg>',
    ScheduledTask:
      '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3.5 2"/></svg>',
    RegistryRun:
      '<svg viewBox="0 0 24 24"><circle cx="7.5" cy="15.5" r="4.5"/><path d="M11 12L21 2"/><path d="M17 6l3 3"/><path d="M14 9l2.5 2.5"/></svg>',
  };

  const SOURCE_LABELS = {
    StartupFolder: "Startup folder",
    ScheduledTask: "Scheduled task",
    RegistryRun: "Registry run",
  };

  const BADGE_CHECK =
    '<svg viewBox="0 0 24 24"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>';
  const BADGE_WARN =
    '<svg viewBox="0 0 24 24"><path d="M12 3L22 20H2z"/><line x1="12" y1="9.5" x2="12" y2="14"/><circle cx="12" cy="16.8" r="0.6"/></svg>';

  function findingsHeadline(n) {
    return n === 1
      ? "1 finding needs a decision"
      : n + " findings need a decision";
  }

  function reasonChipLabel(reason) {
    const lower = reason.toLowerCase();
    if (lower.includes("drop zone")) return ["Suspicious path", "red"];
    if (lower.includes("randomly generated")) return ["Random name", "red"];
    if (lower.includes("powershell")) return ["Hidden PowerShell", "red"];
    if (lower.includes("trusted install")) return ["Trusted location", "teal"];
    if (lower.includes("user profile folder")) return ["Profile exe", "amber"];
    // Detection Engine v2 — note: "invalid signature" contains "valid
    // signature", so it must be matched first.
    if (lower.includes("invalid signature")) return ["Invalid Signature", "red"];
    if (lower.includes("valid signature")) return ["Valid Signature", "teal"];
    if (lower.includes("known malware hash")) return ["Known Malware Hash", "red"];
    if (lower.includes("unsigned binary")) return ["Unsigned Binary", "amber"];
    const stripped = reason.replace(/^[+-]\d+\s*/, "");
    return [stripped.split(/\s+/).slice(0, 3).join(" "), ""];
  }

  function scoreChipClass(score) {
    if (score >= 55) return "crit";
    if (score >= 40) return "high";
    if (score >= 25) return "med";
    return "low";
  }

  function buildCard(entry, cleaned) {
    const card = document.createElement("li");
    card.className = "review-card reveal" + (cleaned ? " cleaned" : "");

    const rawSource = String(entry.entry.source);
    const iconClass =
      rawSource === "ScheduledTask"
        ? "icon-task"
        : rawSource === "RegistryRun"
          ? "icon-registry"
          : "icon-startup";

    const iconWrap = document.createElement("div");
    iconWrap.className = "src-icon " + iconClass;
    iconWrap.innerHTML = SOURCE_ICONS[rawSource] || SOURCE_ICONS.StartupFolder;
    card.appendChild(iconWrap);

    const main = document.createElement("div");
    main.className = "rc-main";

    const topRow = document.createElement("div");
    topRow.className = "rc-top";

    const name = document.createElement("span");
    name.className = "rc-name";
    name.textContent = entry.entry.name;
    name.title = entry.entry.command;

    const scoreEl = document.createElement("span");
    scoreEl.className = "score-chip " + scoreChipClass(entry.score);
    scoreEl.textContent = String(entry.score);
    scoreEl.title = entry.risk + " · risk score " + entry.score;

    topRow.append(name, scoreEl);
    main.appendChild(topRow);

    const chips = document.createElement("div");
    chips.className = "chips";
    const srcChip = document.createElement("span");
    srcChip.className = "chip src";
    srcChip.textContent = SOURCE_LABELS[rawSource] || "Persistence";
    srcChip.title = entry.entry.location;
    chips.appendChild(srcChip);
    const reasons = Array.isArray(entry.reasons) ? entry.reasons : [];
    for (const reason of reasons.slice(0, 4)) {
      const [label, tone] = reasonChipLabel(String(reason));
      const chip = document.createElement("span");
      chip.className = "chip" + (tone ? " " + tone : "");
      chip.textContent = label;
      chip.title = reason;
      chips.appendChild(chip);
    }
    if (chips.children.length > 0) main.appendChild(chips);

    card.appendChild(main);

    if (cleaned) {
      const done = document.createElement("span");
      done.className = "manual-note";
      done.textContent = "Quarantined";
      card.appendChild(done);
    } else if (rawSource === "RegistryRun") {
      const note = document.createElement("span");
      note.className = "manual-note";
      note.textContent = "Manual removal required";
      note.title =
        entry.entry.location +
        " — registry values are not auto-disabled in this version";
      card.appendChild(note);
    } else {
      const btn = document.createElement("button");
      btn.className = "quarantine-btn";
      btn.textContent = "Quarantine";
      btn.addEventListener("click", async () => {
        btn.disabled = true;
        try {
          await invoke("quarantine_entry", {
            id: entry.entry.id,
            name: entry.entry.name,
            command: entry.entry.command,
          });
          btn.textContent = "Quarantined ✓";
          btn.classList.add("row-done");
        } catch (err) {
          btn.disabled = false;
          setPill("error", String(err));
        }
      });
      card.appendChild(btn);
    }
    return card;
  }

  function fillCards(container, entries, cleaned, baseDelayMs) {
    container.innerHTML = "";
    entries.forEach((entry, index) => {
      const card = buildCard(entry, cleaned);
      card.style.setProperty("--d", baseDelayMs + index * 80 + "ms");
      container.appendChild(card);
    });
  }

  function renderResults(summary) {
    const badge = document.getElementById("badge");
    const headline = document.getElementById("headline");
    const reviewBlock = document.getElementById("review-block");
    const cleanedBlock = document.getElementById("cleaned-block");
    const reviewClear = document.getElementById("review-clear");

    const cleanedCount = summary.high_risk_cleaned.length;
    const reviewCount = summary.suspicious_for_review.length;
    const trouble = cleanedCount + reviewCount;

    badge.className = "badge " + (trouble ? "warn" : "clean");
    badge.innerHTML = trouble ? BADGE_WARN : BADGE_CHECK;

    const subline = document.getElementById("subline");
    if (reviewCount > 0) {
      headline.textContent = findingsHeadline(reviewCount);
      subline.textContent =
        summary.total + " entries checked" +
        (cleanedCount
          ? ", " + cleanedCount + " cleaned automatically"
          : "");
    } else if (cleanedCount > 0) {
      headline.textContent = "Threats cleaned automatically";
      subline.textContent =
        summary.total + " entries checked, " + cleanedCount +
        " cleaned, nothing left to review";
    } else {
      headline.textContent = "System is clean";
      subline.textContent =
        summary.total + " entries checked, nothing needs attention";
    }

    countUp(document.getElementById("stat-cleaned"), cleanedCount);
    countUp(document.getElementById("stat-review"), reviewCount);
    countUp(document.getElementById("stat-safe"), summary.safe);

    // zeros shouldn't shout — neutralize accent line + number on empty stats
    document.querySelector(".stat.tint-teal").classList.toggle("stat-zero", cleanedCount === 0);
    document.querySelector(".stat.tint-amber").classList.toggle("stat-zero", reviewCount === 0);
    document.querySelector(".stat.tint-neutral").classList.toggle("stat-zero", summary.safe === 0);

    fillCards(
      document.getElementById("review-cards"),
      summary.suspicious_for_review,
      false,
      290
    );
    fillCards(
      document.getElementById("cleaned-cards"),
      summary.high_risk_cleaned,
      true,
      250
    );

    reviewClear.classList.toggle("hidden", reviewCount > 0);
    reviewBlock.classList.toggle("hidden", false);

    const showCleanedPanel = cleanedCount > 0;
    cleanedBlock.classList.toggle("hidden", !showCleanedPanel);

    if (trouble === 0) {
      setPill("clean", "Scan complete — all clear");
    } else {
      const hasHighRiskInReview = summary.suspicious_for_review.some(
        (s) => s.risk === "HighRisk"
      );
      setPill(
        hasHighRiskInReview ? "danger" : "warn",
        "Scan complete — review needed"
      );
    }

    const revealables = resultsView.querySelectorAll(".reveal");
    if (!REDUCED && revealables.length > 0) {
      void resultsView.offsetWidth;
    }
    netmap.show(summary.total);
  }

  function switchView(fromEl, toEl) {
    return new Promise((resolve) => {
      if (REDUCED) {
        fromEl.classList.add("hidden");
        toEl.classList.remove("hidden");
        resolve();
        return;
      }
      fromEl.classList.add("exiting");
      setTimeout(() => {
        fromEl.classList.add("hidden");
        fromEl.classList.remove("exiting");
        toEl.classList.remove("hidden");
        toEl.classList.add("pre-enter");
        void toEl.offsetWidth;
        toEl.classList.remove("pre-enter");
        setTimeout(resolve, 330);
      }, 290);
    });
  }

  async function runScan() {
    const token = ++scanToken;
    resultsView.classList.add("hidden");
    netmap.hide();
    resultsView
      .querySelectorAll(".reveal")
      .forEach((el) => el.classList.remove("in"));
    scanView.classList.remove("hidden", "exiting", "pre-enter");
    logList.innerHTML = "";
    itemFeedCount = 0;
    lastItemNode = null;
    if (feedCountEl) feedCountEl.textContent = "0 ITEMS";
    setPill("scanning", "sweeping persistence locations…");
    radar.start();
    try {
      const summary = await invoke("run_auto_scan");
      if (token !== scanToken) return;
      appendLog("done", summary.total + " entries processed — scan finished");
      radar.stop();
      await switchView(scanView, resultsView);
      if (token !== scanToken) return;
      renderResults(summary);
    } catch (err) {
      radar.stop();
      setPill("error", String(err));
      appendLog("error", String(err));
    }
  }

  function appendStageLine(stage, message) {
    const li = document.createElement("li");
    if (stage === "cleaning") li.classList.add("warn-line");
    const tag = document.createElement("b");
    tag.textContent = "[" + stage + "]";
    const body = document.createElement("span");
    body.textContent = message;
    li.append(tag, body);
    logList.appendChild(li);
    while (logList.children.length > 200) logList.removeChild(logList.firstChild);
    logList.scrollTop = logList.scrollHeight;
  }

  listen("scan-progress", (event) => {
    const payload = event.payload;
    if (payload.stage === "item-scanned") {
      appendItemLine(payload);
      lastItemNode = radar.addNode(payload.risk, payload.name);
      return;
    }
    setPill("scanning", payload.message);
    if (payload.stage === "cleaning") {
      glitchPillText();
      radar.ping(lastItemNode);
      lastItemNode = null;
      appendStageLine(payload.stage, payload.message);
      return;
    }
    appendLog(payload.stage, payload.message);
  });

  document.getElementById("rescan-btn").addEventListener("click", runScan);

  const footMsg = document.getElementById("footbar-msg");
  let footTimer = null;
  function footFeedback(text, isError) {
    footMsg.textContent = text;
    footMsg.classList.toggle("error", !!isError);
    footMsg.classList.add("show");
    clearTimeout(footTimer);
    footTimer = setTimeout(() => footMsg.classList.remove("show"), 2800);
  }

  function cleanErrText(err, fallback) {
    const s = String(err == null ? "" : err).replace(/^Error:\s*/i, "").trim();
    return s || fallback;
  }

  document
    .getElementById("btn-quarantine-folder")
    .addEventListener("click", async () => {
      try {
        await invoke("open_quarantine_folder");
        footFeedback("Quarantine folder opened", false);
      } catch (err) {
        footFeedback(
          cleanErrText(err, "Could not open quarantine folder"),
          true
        );
      }
    });
  document
    .getElementById("btn-view-log")
    .addEventListener("click", async () => {
      try {
        await invoke("view_log");
        footFeedback("Scan log opened", false);
      } catch (err) {
        footFeedback(cleanErrText(err, "Could not open scan log"), true);
      }
    });
  document.getElementById("btn-exit").addEventListener("click", async () => {
    try {
      await invoke("exit_app");
    } catch (err) {
      footFeedback(cleanErrText(err, "Could not exit app"), true);
    }
  });

  runScan();
})();
