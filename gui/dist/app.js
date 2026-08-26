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
  const cleanupView = document.getElementById("cleanup-view");
  const landingView = document.getElementById("landing-view");

  let scanToken = 0;
  window.__curePingCount = 0;
  let itemFeedCount = 0;
  let lastItemNode = null;

  function escHtml(s) {
    return String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  }

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

  function appendProcessLine(p) {
    const li = document.createElement("li");
    li.className = "item-line fresh risk-" + String(p.risk || "Safe").toLowerCase();

    const arrow = document.createElement("span");
    arrow.className = "arrow";
    arrow.textContent = "\u26A0";

    const name = document.createElement("span");
    name.className = "iname";
    name.textContent = String(p.name || "?") + " (pid " + p.pid + ")";

    const src = document.createElement("span");
    src.className = "isrc";
    src.textContent = "[process]";

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

  function appendRansomLine(p) {
    const li = document.createElement("li");
    li.className = "item-line fresh risk-highrisk";
    const arrow = document.createElement("span");
    arrow.className = "arrow";
    arrow.textContent = "\u26A0";
    const name = document.createElement("span");
    name.className = "iname";
    name.textContent = p.finding_type === "ransom-note" ? "Ransom note" : "Bulk encryption";
    const detail = document.createElement("span");
    detail.className = "isrc";
    detail.textContent = p.detail || "";
    li.append(arrow, name, detail);
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
          resolved: true,
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
    let mascot = null;
    let visit = null;
    let visitQueue = [];
    let visitsDone = 0;
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
        const pending = nd.resolved === false;
        const c = pending ? [222, 220, 240] : (RGB[nd.risk] || RGB.Safe);
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
          const flashAt = nd.resolvedAt != null ? nd.resolvedAt : nd.born + TRAVEL;
          const flash =
            t >= 1 && !pending ? Math.max(0, 1 - (now - flashAt) / 480) : 0;
          ctx.fillStyle = rgba(c, na.toFixed(2));
          ctx.shadowColor = rgba(c, 0.9);
          ctx.shadowBlur = (2 + flash * 5) * DPR;
          ctx.beginPath();
          ctx.arc(p.x, p.y, (pending ? 2.4 : dotRadius(nd.risk)) * (1 + flash * 0.35) * DPR, 0, Math.PI * 2);
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

    // ---- mascot: violet orb that darts core -> node and knocks threats out
    const MASCOT_TRAVEL_MS = 340;
    const MASCOT_IMPACT_MS = 220;
    // Rakshak's per-node patrol: travel to each new node, pause a beat to
    // "check" it (node resolves to its risk color), then move on. Speeds
    // adapt to backlog so a normal scan shows a real visit per node.
    const VISIT_TRAVEL_MS = 420;
    const VISIT_TRAVEL_MIN_MS = 150;
    const CHECK_MS = 340;
    const CHECK_MIN_MS = 0;
    const VISIT_QUEUE_CAP = 10;
    const MAX_VISITS = 48;

    // ---- Rakshak's patrol visits: travel to each pending node, check it,
    // resolve its color, escalate to the fight gesture on threats.

    function drawOrbAt(x, y, r, sx, sy) {
      ctx.save();
      ctx.translate(x, y);
      ctx.scale(sx, sy);
      const g = ctx.createRadialGradient(-r * 0.3, -r * 0.3, r * 0.1, 0, 0, r);
      g.addColorStop(0, "#d9d4ff");
      g.addColorStop(0.5, "#7c6cf0");
      g.addColorStop(1, "#453aa6");
      ctx.shadowColor = "rgba(124,108,240,0.65)";
      ctx.shadowBlur = 10 * DPR;
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.arc(0, 0, r, 0, Math.PI * 2);
      ctx.fill();
      ctx.shadowBlur = 0;
      ctx.restore();
    }

    function startNextVisit(now) {
      if (visit || !visitQueue.length) return;
      const entry = visitQueue.shift();
      visit = { nd: entry.nd, fight: entry.fight, start: now, phase: "travel", trail: [] };
      window.__cureVisitActive = true;
    }

    function drawVisit(now) {
      if (!visit) return;
      const p = nodeXY(visit.nd);

      if (visit.phase === "travel") {
        // hustle when nodes are backing up — calm when the scan is light
        const load = Math.min(visitQueue.length, 6);
        const dur = Math.max(VISIT_TRAVEL_MIN_MS, VISIT_TRAVEL_MS - load * 55);
        const t = Math.min((now - visit.start) / dur, 1);
        const e = 1 - Math.pow(1 - t, 2.2);
        const x = CX + (p.x - CX) * e;
        const y = CY + (p.y - CY) * e;

        visit.trail.push({ x, y });
        if (visit.trail.length > 7) visit.trail.shift();
        for (let i = 0; i < visit.trail.length; i++) {
          const tr = visit.trail[i];
          ctx.fillStyle = ACCENT((((i + 1) / visit.trail.length) * 0.2).toFixed(3));
          ctx.beginPath();
          ctx.arc(tr.x, tr.y, Math.max(0.6, 2.1 - i * 0.18) * DPR, 0, Math.PI * 2);
          ctx.fill();
        }

        const stretch = 1 + 0.22 * Math.sin(t * Math.PI);
        const ang = Math.atan2(p.y - CY, p.x - CX);
        ctx.save();
        ctx.translate(x, y);
        ctx.rotate(ang);
        drawOrbAt(0, 0, 6.2 * DPR, stretch, 1 / stretch);
        ctx.restore();

        if (t >= 1) {
          visit.phase = "check";
          visit.start = now;
        }
        return;
      }

      // check phase: a quick pause + glow pulse while the node resolves
      const load = Math.min(visitQueue.length, 6);
      const dur = Math.max(CHECK_MIN_MS, CHECK_MS - load * 55);
      const ct = (now - visit.start) / dur;
      const pulse = Math.sin(Math.min(ct, 1) * Math.PI);
      drawOrbAt(p.x, p.y, (6.2 + 0.6 * pulse) * DPR, 1, 1);
      if (dur > 0 && ct < 1) {
        ctx.lineWidth = 1.6 * DPR;
        ctx.strokeStyle = ACCENT((0.5 * (1 - ct)).toFixed(3));
        ring(p.x, p.y, (6 + ct * 17) * DPR);
      }
      if (ct >= 1) {
        visit.nd.resolved = true;
        visit.nd.resolvedAt = now;
        visitsDone += 1;
        window.__cureResolvedCount = (window.__cureResolvedCount || 0) + 1;
        if (visit.fight) {
          // fight gesture plays in place: pre-complete the legacy travel so
          // only the existing impact rings/squash animate at the node
          mascot = { nd: visit.nd, start: now - MASCOT_TRAVEL_MS, trail: [] };
          window.__cureMascotActive = true;
        }
        visit = null;
        window.__cureVisitActive = visitQueue.length > 0;
      }
    }

    function drawMascot(now) {
      if (!mascot) return;
      const p = nodeXY(mascot.nd);
      const t = Math.min((now - mascot.start) / MASCOT_TRAVEL_MS, 1);
      const e = 1 - Math.pow(1 - t, 2.4);
      const x = CX + (p.x - CX) * e;
      const y = CY + (p.y - CY) * e;

      mascot.trail.push({ x, y });
      if (mascot.trail.length > 7) mascot.trail.shift();
      for (let i = 0; i < mascot.trail.length; i++) {
        const tr = mascot.trail[i];
        ctx.fillStyle = ACCENT((((i + 1) / mascot.trail.length) * 0.2).toFixed(3));
        ctx.beginPath();
        ctx.arc(tr.x, tr.y, Math.max(0.6, 2.1 - i * 0.18) * DPR, 0, Math.PI * 2);
        ctx.fill();
      }

      const stretch = 1 + 0.26 * Math.sin(t * Math.PI);
      let sx = stretch;
      let sy = 1 / stretch;
      let r = 6.2 * DPR;

      if (t >= 1) {
        const it = Math.min((now - mascot.start - MASCOT_TRAVEL_MS) / MASCOT_IMPACT_MS, 1);
        const pulse = Math.sin(it * Math.PI);
        sx = 1 + 0.42 * pulse;
        sy = 1 - 0.34 * pulse;
        ctx.lineWidth = 2 * DPR;
        ctx.strokeStyle = ACCENT(((1 - it) * 0.75).toFixed(3));
        ring(p.x, p.y, (4 + it * 26) * DPR);
        ctx.lineWidth = 1 * DPR;
        ctx.strokeStyle = "rgba(237,237,239," + ((1 - it) * 0.5).toFixed(3) + ")";
        ring(p.x, p.y, (2 + it * 14) * DPR);
        r *= 1 + 0.22 * (1 - it);
        if (it >= 1) {
          mascot = null;
          window.__cureMascotActive = false;
          return;
        }
      }

      const ang = Math.atan2(p.y - CY, p.x - CX);
      ctx.save();
      ctx.translate(x, y);
      ctx.rotate(ang);
      ctx.scale(sx, sy);
      const g = ctx.createRadialGradient(-r * 0.3, -r * 0.3, r * 0.1, 0, 0, r);
      g.addColorStop(0, "#d9d4ff");
      g.addColorStop(0.5, "#7c6cf0");
      g.addColorStop(1, "#453aa6");
      ctx.shadowColor = "rgba(124,108,240,0.65)";
      ctx.shadowBlur = 10 * DPR;
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.arc(0, 0, r, 0, Math.PI * 2);
      ctx.fill();
      ctx.shadowBlur = 0;
      ctx.restore();
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
      drawMascot(now);
      startNextVisit(now);
      drawVisit(now);
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
        mascot = null;
        visit = null;
        visitQueue = [];
        visitsDone = 0;
        window.__cureMascotActive = false;
        window.__cureVisitActive = false;
        window.__cureResolvedCount = 0;
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
        // Rakshak personally visits new nodes while there's budget; once the
        // queue saturates (huge scans) nodes resolve instantly via the
        // existing pulse-only birth animation.
        if (!REDUCED && visitsDone < MAX_VISITS && visitQueue.length < VISIT_QUEUE_CAP) {
          nd.resolved = false;
          visitQueue.push({ nd, fight: false });
        } else {
          window.__cureResolvedCount = (window.__cureResolvedCount || 0) + 1;
        }
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
      dispatchMascot(nd) {
        window.__cureMascotCount = (window.__cureMascotCount || 0) + 1;
        if (REDUCED || !nd) return;
        // HighRisk: escalate this node's visit into the fight gesture. If the
        // visit is queued, jump it to the front; if Rakshak is already mid-
        // visit to it, flag the escalation; otherwise fall back to the
        // legacy core->node dart (existing animation, unchanged).
        const qi = visitQueue.findIndex(function(e) { return e.nd === nd; });
        if (qi !== -1) {
          const entry = visitQueue.splice(qi, 1)[0];
          entry.fight = true;
          visitQueue.unshift(entry);
          return;
        }
        if (visit && visit.nd === nd) {
          visit.fight = true;
          return;
        }
        mascot = { nd, start: performance.now(), trail: [] };
        window.__cureMascotActive = true;
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
    // Rakshak's results-view life: a one-time "secured" wash sweeps outward
    // from the core while he holds a guard pose, then he relaxes into a slow
    // figure-eight patrol around the core.
    const WASH_MS = 850;
    const GUARD_HOLD_MS = 2600;
    const GUARD_RELAX_MS = 1400;

    let rafId = null;
    let travelers = [];
    let lastSpawn = 0;
    let flourish = null;
    let guardUntil = 0;
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
      RX = Math.max(30, Math.min(W / 2 - 12 * DPR, R * 2.2));
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
      // spread nodes toward the panel edges: rf [0.58..0.92] -> [0.6..0.975]
      const rm = 0.6 + (nd.rf - 0.58) * 1.1;
      return {
        x: CX + Math.cos(nd.ang) * rm * RX,
        y: CY + Math.sin(nd.ang) * rm * R,
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

    function drawWash(now) {
      if (!flourish) return;
      if (now < flourish.start) return; // wait out the panel reveal fade
      const t = (now - flourish.start) / WASH_MS;
      if (t >= 1) {
        flourish = null;
        return;
      }
      const e = 1 - Math.pow(1 - t, 2.5);
      // leading edge sweeping through the constellation
      ctx.lineWidth = 2.4 * DPR;
      ctx.strokeStyle = ACCENT((0.55 * (1 - t)).toFixed(3));
      ellipse(CX, CY, Math.max(1, e * RX), Math.max(1, e * R));
      // trailing soft fill behind the edge
      const grd = ctx.createRadialGradient(
        CX, CY, Math.max(0, e * Math.min(RX, R) - 44 * DPR),
        CX, CY, Math.max(2, e * Math.max(RX, R) * 1.02)
      );
      grd.addColorStop(0, "rgba(124,108,240,0)");
      grd.addColorStop(0.82, "rgba(124,108,240," + (0.08 * (1 - t)).toFixed(3) + ")");
      grd.addColorStop(1, "rgba(124,108,240,0)");
      ctx.fillStyle = grd;
      ctx.fillRect(0, 0, W, H);
      // inner echo ring
      const e2 = Math.max(0, e - 0.16);
      ctx.lineWidth = 1 * DPR;
      ctx.strokeStyle = ACCENT((0.28 * (1 - t)).toFixed(3));
      ellipse(CX, CY, Math.max(1, e2 * RX), Math.max(1, e2 * R));
    }

    function drawRakshak(now) {
      const k = Math.min(1.2, Math.max(0.6, Math.min(RX, R) / 260));
      const guarding = now < guardUntil;
      let x = CX;
      let y = CY;

      if (!guarding) {
        // figure-eight patrol drift around the core, slow enough to read as
        // "watching over"; eases out of the guard pose instead of jumping
        const pt = now / 1000;
        const px = CX + Math.sin(pt * 0.42) * RX * 0.3;
        const py = CY + Math.sin(pt * 0.84 + 1.2) * R * 0.2;
        const relax = Math.min(Math.max((now - guardUntil) / GUARD_RELAX_MS, 0), 1);
        const ease = 1 - Math.pow(1 - relax, 2);
        x = CX + (px - CX) * ease;
        y = CY + (py - CY) * ease;
      }

      const r = ((6.5 + 1.5 * k) * (guarding ? 1.08 : 1)) * DPR;
      // guard rings ease in with the flourish, fade as patrol resumes
      let ra;
      if (guarding) {
        ra = REDUCED ? 1 : Math.min((now - (guardUntil - GUARD_HOLD_MS)) / 600, 1);
      } else {
        ra = Math.max(0, 1 - (now - guardUntil) / 900);
      }
      if (ra > 0.01) {
        const breathe = REDUCED ? 0 : 0.5 + 0.5 * Math.sin(now / 1300);
        ctx.lineWidth = 1.4 * DPR;
        ctx.strokeStyle = ACCENT((0.42 * ra * (0.8 + 0.2 * breathe)).toFixed(3));
        ring(x, y, r * 1.9);
        ctx.lineWidth = 1 * DPR;
        ctx.strokeStyle = ACCENT((0.18 * ra * (0.8 + 0.2 * breathe)).toFixed(3));
        ring(x, y, r * 2.7);
      }

      const g = ctx.createRadialGradient(x - r * 0.3, y - r * 0.3, r * 0.1, x, y, r);
      g.addColorStop(0, "#d9d4ff");
      g.addColorStop(0.5, "#7c6cf0");
      g.addColorStop(1, "#453aa6");
      ctx.shadowColor = "rgba(124,108,240,0.75)";
      ctx.shadowBlur = 15 * DPR;
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.fill();
      ctx.shadowBlur = 0;
    }

    function drawFrame(now) {
      ctx.clearRect(0, 0, W, H);
      drawBackdrop(now);
      drawGraph(now);
      drawWash(now);
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
      drawRakshak(now);
    }

    function loop(now) {
      drawFrame(now);
      rafId = requestAnimationFrame(loop);
    }

    return {
      show(summaryTotal) {
        if (countEl) countEl.textContent = summaryTotal + " nodes";
        travelers = [];
        lastSpawn = performance.now() - 2400;
        // any nodes Rakshak didn't reach (huge scans) settle into place here
        for (const nd of NetStore.all()) nd.resolved = true;
        // hold the wash until the map panel's entrance reveal has finished,
        // so the flourish plays on the settled network, not under the fade
        const t0 = performance.now();
        const washStart = t0 + 750;
        guardUntil = washStart + GUARD_HOLD_MS;
        flourish = REDUCED ? null : { start: washStart };
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
        flourish = null;
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
    const procCount = (summary.process_findings || []).length;
    const ransomCount = (summary.ransom_findings || []).length;
    const trouble = cleanedCount + reviewCount + procCount + ransomCount;

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

    // Process findings panel
    const processBlock = document.getElementById("process-block");
    const ransomBlock = document.getElementById("ransom-block");
    if (processBlock) {
      const procFindings = summary.process_findings || [];
      sweepState.findings = procFindings;
      sweepState.checked.clear();
      sweepState.armed = false;
      clearTimeout(sweepState.armTimer);
      const procCards = document.getElementById("process-cards");
      if (procCards) {
        procCards.innerHTML = "";
        procFindings.forEach(function(p) {
          const card = document.createElement("li");
          card.className = "review-card proc-entry";
          card.dataset.name = p.name;
          card.dataset.pid = String(p.pid);
          card.dataset.exe = p.exe_path || "";
          const box = document.createElement("input");
          box.type = "checkbox";
          box.addEventListener("change", function() {
            if (box.checked) sweepState.checked.add(p.pid);
            else sweepState.checked.delete(p.pid);
            disarmKillButton();
            card.classList.toggle("proc-selected", box.checked);
          });
          const main = document.createElement("div");
          main.className = "rc-main";
          const topRow = document.createElement("div");
          topRow.className = "rc-top";
          const nameEl = document.createElement("span");
          nameEl.className = "rc-name";
          nameEl.textContent = p.name;
          nameEl.title = p.exe_path || "";
          const scoreEl = document.createElement("span");
          scoreEl.className = "score-chip " + scoreChipClass(p.score);
          scoreEl.textContent = String(p.score);
          scoreEl.title = p.risk + " · risk score " + p.score;
          topRow.append(nameEl, scoreEl);
          main.appendChild(topRow);
          const chips = document.createElement("div");
          chips.className = "chips";
          var pidChip = document.createElement("span");
          pidChip.className = "chip";
          pidChip.textContent = "pid " + p.pid;
          chips.appendChild(pidChip);
          if (p.exe_path) {
            var exeChip = document.createElement("span");
            exeChip.className = "chip";
            exeChip.textContent = p.exe_path.split(/[/\\]/).pop();
            exeChip.title = p.exe_path;
            chips.appendChild(exeChip);
          }
          var reasons = Array.isArray(p.reasons) ? p.reasons : [];
          for (var ri = 0; ri < Math.min(reasons.length, 4); ri++) {
            var lr = reasonChipLabel(String(reasons[ri]));
            var rc = document.createElement("span");
            rc.className = "chip" + (lr[1] ? " " + lr[1] : "");
            rc.textContent = lr[0];
            rc.title = reasons[ri];
            chips.appendChild(rc);
          }
          if (chips.children.length > 0) main.appendChild(chips);
          card.append(box, main);
          procCards.appendChild(card);
        });
      }
      updateKillButton();
      var killStatus = document.getElementById("kill-procs-status");
      if (killStatus) { killStatus.textContent = ""; killStatus.classList.add("hidden"); }
      processBlock.classList.toggle("hidden", procFindings.length === 0);
    }

    // Ransom findings panel
    if (ransomBlock) {
      const ransomFindings = summary.ransom_findings || [];
      const ransomCards = document.getElementById("ransom-cards");
      const ransomLink = document.getElementById("ransom-link");
      if (ransomCards) {
        ransomCards.innerHTML = "";
        let hasFamily = false;
        ransomFindings.forEach(function(r) {
          const card = document.createElement("div");
          card.className = "entry-card";
          let html =
            '<div class="card-header"><span class="card-name">' +
            escHtml(r.finding_type === "ransom-note" ? "Ransom note" : "Bulk encryption detected") +
            '</span></div><div class="card-detail">' + escHtml(r.detail) + "</div>";
          if (r.suspected_family) {
            html += '<div class="card-detail" style="color:#c9a0ff">Suspected family: ' +
              escHtml(r.suspected_family) + "</div>";
            hasFamily = true;
          }
          card.innerHTML = html;
          ransomCards.appendChild(card);
        });
        if (ransomLink) {
          ransomLink.classList.toggle("hidden", !hasFamily);
        }
      }
      ransomBlock.classList.toggle("hidden", ransomFindings.length === 0);
    }

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

    // Rakshak's status line under the scan-map header
    const rkStatus = document.getElementById("rakshak-status");
    if (rkStatus) {
      rkStatus.innerHTML = '<span class="rk-name">Rakshak</span> secured ' +
        summary.total + " node" + (summary.total === 1 ? "" : "s");
    }

    const revealables = resultsView.querySelectorAll(".reveal");
    if (!REDUCED && revealables.length > 0) {
      void resultsView.offsetWidth;
    }
    netmap.show(summary.total);
    syncCanaryStatus();
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

  async function runScan(preLines = []) {
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
    for (const line of preLines) {
      appendLog("overlay", line);
    }
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
    if (payload.stage === "process-flagged") {
      appendProcessLine(payload);
      radar.addNode(payload.risk, payload.name);
      return;
    }
    if (payload.stage === "ransom-found") {
      appendRansomLine(payload);
      return;
    }
    setPill("scanning", payload.message);
    if (payload.stage === "cleaning") {
      glitchPillText();
      radar.dispatchMascot(lastItemNode);
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

  // ---- canary guard (ransomware decoy monitor) ----------------------------

  const canaryToggle = document.getElementById("canary-toggle");
  const canaryToggleText = canaryToggle ? canaryToggle.querySelector(".canary-toggle-text") : null;
  let canaryActive = false;

  async function syncCanaryStatus() {
    try {
      const st = await invoke("canary_status");
      canaryActive = !!st.active;
      updateCanaryUI();
    } catch (_) { /* ignore */ }
  }

  function updateCanaryUI() {
    if (canaryToggle) {
      canaryToggle.setAttribute("aria-pressed", String(canaryActive));
      canaryToggle.classList.toggle("on", canaryActive);
    }
    if (canaryToggleText) canaryToggleText.textContent = canaryActive ? "ON" : "OFF";
  }

  if (canaryToggle) {
    canaryToggle.addEventListener("click", async () => {
      try {
        if (canaryActive) {
          await invoke("stop_canary_guard");
          canaryActive = false;
          footFeedback("Canary guard deactivated", false);
        } else {
          await invoke("start_canary_guard");
          canaryActive = true;
          footFeedback("Canary guard active — decoys planted", false);
        }
      } catch (err) {
        footFeedback(cleanErrText(err, "Could not toggle canary guard"), true);
      }
      updateCanaryUI();
    });
  }

  // Listen for canary-alert events from backend
  const canaryOverlay = document.getElementById("canary-alert-overlay");
  const canaryDetail = document.getElementById("canary-alert-detail");
  const canaryDismissBtn = document.getElementById("canary-dismiss-btn");

  if (canaryDismissBtn) {
    canaryDismissBtn.addEventListener("click", () => {
      if (canaryOverlay) canaryOverlay.classList.add("hidden");
    });
  }

  TAU.event.listen("canary-alert", (ev) => {
    const payload = typeof ev.payload === "string" ? JSON.parse(ev.payload) : ev.payload;
    if (canaryDetail) {
      const kind = payload.kind || "unknown";
      const folder = payload.folder || "";
      const file = payload.file || "";
      const action = payload.action || "";
      canaryDetail.textContent =
        kind.replace(/-/g, " ").toUpperCase() + " — " +
        (folder ? folder + " " : "") + file +
        (action ? " (" + action + ")" : "");
    }
    if (canaryOverlay) canaryOverlay.classList.remove("hidden");
  });

  // Auto-sync canary status when results view appears
  const origRenderResults = typeof renderResults === "function" ? renderResults : null;

  // ---- disk cleanup (separate flow / own view) ----------------------------

  function fmtBytes(n) {
    if (n >= 1073741824) return (n / 1073741824).toFixed(1) + " GB";
    if (n >= 1048576) return (n / 1048576).toFixed(1) + " MB";
    if (n >= 1024) return Math.round(n / 1024) + " KB";
    return n + " B";
  }

  const cleanupEls = {
    openBtn: document.getElementById("open-cleanup"),
    backBtn: document.getElementById("cleanup-back"),
    statusLine: document.getElementById("cleanup-status-line"),
    statusText: document.getElementById("cleanup-status-text"),
    subline: document.getElementById("cleanup-subline"),
    idle: document.getElementById("cleanup-idle"),
    scanBtn: document.getElementById("cleanup-scan-btn"),
    loading: document.getElementById("cleanup-loading"),
    body: document.getElementById("cleanup-body"),
    total: document.getElementById("cleanup-total"),
    grid: document.getElementById("cleanup-grid"),
    downloads: document.getElementById("cleanup-downloads"),
    dlList: document.getElementById("cleanup-dl-list"),
    btn: document.getElementById("cleanup-btn"),
    status: document.getElementById("cleanup-status"),
    failures: document.getElementById("cleanup-failures"),
    stage: document.getElementById("toss-stage"),
    liveCounter: document.getElementById("cleanup-live-counter"),
  };

  const cleanupState = {
    summary: null,
    selectedCats: new Set(),
    checkedDownloads: new Set(),
    armed: false,
    armTimer: null,
    running: false,
    open: false,
    savedPill: null,
  };

  const sweepState = {
    findings: [],
    checked: new Set(),
    armed: false,
    armTimer: null,
    running: false,
  };

  function disarmKillButton() {
    sweepState.armed = false;
    clearTimeout(sweepState.armTimer);
    const statusEl = document.getElementById("kill-procs-status");
    if (statusEl) { statusEl.textContent = ""; statusEl.classList.add("hidden"); }
    updateKillButton();
  }

  function updateKillButton() {
    const btn = document.getElementById("kill-procs-btn");
    if (!btn) return;
    const enabled = sweepState.checked.size > 0 && !sweepState.running;
    btn.disabled = !enabled;
    btn.classList.toggle("arm-danger", sweepState.armed);
    btn.textContent = sweepState.armed
      ? "Click again to kill " + sweepState.checked.size + " process(es)"
      : "Kill selected (" + sweepState.checked.size + ")";
  }

  function setCleanupPill(state, text) {
    cleanupEls.statusLine.className = "pill " + state;
    cleanupEls.statusText.textContent = text;
  }

  // ---- mascot toss animation (SVG, cleanup view) ---------------------------

  const toss = {
    raf: null,
    start: 0,
    expected: 0,
    active: false,
    settling: false,
    svg: null,
    orbG: null,
    orb: null,
    glyphs: null,
    lid: null,
    trailG: null,
    trail: [],
    flash: null,
  };
  const TOSS_CYCLE_MS = 520;
  const GLYPH_BASE = [
    [6, 30],
    [27, 30],
    [48, 30],
  ];
  const ORB_REST = [24, 16];
  const TRASH_MOUTH = [124, 22];

  function tossInit() {
    if (toss.svg) return;
    toss.svg = cleanupEls.stage.querySelector("svg");
    toss.orbG = document.getElementById("mascot-g");
    toss.orb = document.getElementById("mascot-orb");
    toss.glyphs = Array.from(document.querySelectorAll("#file-glyphs .file-glyph"));
    toss.lid = document.getElementById("trash-lid");
    toss.trailG = document.getElementById("trail-g");
    toss.orb.setAttribute("cx", "0");
    toss.orb.setAttribute("cy", "0");
    toss.orbG.setAttribute("transform", "translate(" + ORB_REST[0] + " " + ORB_REST[1] + ")");
    const NS = "http://www.w3.org/2000/svg";
    for (let i = 0; i < 3; i++) {
      const c = document.createElementNS(NS, "circle");
      c.setAttribute("r", "3");
      c.setAttribute("fill", "rgba(124,108,240,0.3)");
      c.setAttribute("opacity", "0");
      toss.trailG.appendChild(c);
    }
    toss.flash = document.createElementNS(NS, "circle");
    toss.flash.setAttribute("r", "0");
    toss.flash.setAttribute("fill", "none");
    toss.flash.setAttribute("stroke", "rgba(124,108,240,0.8)");
    toss.flash.setAttribute("stroke-width", "1.6");
    toss.flash.setAttribute("opacity", "0");
    toss.svg.appendChild(toss.flash);
  }

  function tossSetOrb(x, y, sx, sy) {
    toss.orbG.setAttribute(
      "transform",
      "translate(" + x + " " + y + ") scale(" + sx + " " + sy + ")"
    );
  }

  function tossGlyphTransform(i, x, y, s, opacity) {
    const g = toss.glyphs[i];
    g.setAttribute("transform", "translate(" + x + " " + y + ") scale(" + s + ")");
    g.setAttribute("opacity", String(opacity));
  }

  function easeInOut(t) {
    return t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2;
  }

  function tossFrame(now) {
    if (!toss.active) return;
    const elapsed = now - toss.start;
    const cycleT = (elapsed % TOSS_CYCLE_MS) / TOSS_CYCLE_MS;
    const gi = Math.floor(elapsed / TOSS_CYCLE_MS) % toss.glyphs.length;
    const [gx, gy] = GLYPH_BASE[gi];

    let ox = ORB_REST[0];
    let oy = ORB_REST[1];
    let sx = 1;
    let sy = 1;

    if (cycleT < 0.32) {
      const t = easeInOut(cycleT / 0.32);
      ox = ORB_REST[0] + (gx + 6 - ORB_REST[0]) * t;
      oy = ORB_REST[1] + (gy + 7 - ORB_REST[1]) * t;
      const stretch = 1 + 0.22 * Math.sin(t * Math.PI);
      sx = stretch;
      sy = 1 / stretch;
      tossGlyphTransform(gi, gx, gy, 1, 1);
    } else if (cycleT < 0.4) {
      const t = (cycleT - 0.32) / 0.08;
      ox = gx + 6;
      oy = gy + 7;
      sx = 1 + 0.35 * Math.sin(t * Math.PI);
      sy = 1 - 0.28 * Math.sin(t * Math.PI);
      tossGlyphTransform(gi, gx, gy, 1 - 0.2 * t, 1 - 0.6 * t);
    } else if (cycleT < 0.78) {
      const t = easeInOut((cycleT - 0.4) / 0.38);
      const tx = TRASH_MOUTH[0];
      const ty = TRASH_MOUTH[1];
      const cxp = (gx + 6 + tx) / 2;
      const cyp = Math.min(gy, ty) - 26;
      const gx2 = (1 - t) * (gx + 6) + 2 * (1 - t) * t * cxp + t * t * tx;
      const gy2 = (1 - t) * (gy + 7) + 2 * (1 - t) * t * cyp + t * t * ty;
      const ot = Math.max(0, t - 0.06);
      ox = (1 - ot) * (gx + 6) + 2 * (1 - ot) * ot * cxp + ot * ot * tx;
      oy = (1 - ot) * (gy + 7) + 2 * (1 - ot) * ot * (cyp + 6) + ot * ot * (ty + 3);
      const stretch = 1 + 0.2 * Math.sin(t * Math.PI);
      sx = stretch;
      sy = 1 / stretch;
      tossGlyphTransform(gi, gx2, gy2, 1 - 0.35 * t, 1 - 0.6 * t);
    } else {
      const t = (cycleT - 0.78) / 0.22;
      ox = TRASH_MOUTH[0] + (ORB_REST[0] - TRASH_MOUTH[0]) * easeInOut(t);
      oy = TRASH_MOUTH[1] + (ORB_REST[1] - TRASH_MOUTH[1]) * easeInOut(t);
      const pop = Math.sin(Math.min(t * 2.2, 1) * Math.PI);
      toss.lid.setAttribute("transform", "rotate(" + -34 * pop + " -9 -9) translate(0 " + -3 * pop + ")");
      toss.flash.setAttribute("r", String(2 + pop * 9));
      toss.flash.setAttribute("opacity", String((1 - t) * 0.8));
      toss.flash.setAttribute("cx", String(TRASH_MOUTH[0]));
      toss.flash.setAttribute("cy", String(TRASH_MOUTH[1]));
      tossGlyphTransform(gi, gx, gy, 0.4, 0);
    }

    tossSetOrb(ox, oy, sx, sy);

    const trailEls = toss.trailG.children;
    for (let i = trailEls.length - 1; i >= 0; i--) {
      const src = trailEls[i];
      const behind = trailEls.length - i;
      src.setAttribute("cx", String(ox - behind * 4));
      src.setAttribute("cy", String(oy + behind * 1.2));
      src.setAttribute("opacity", String(0.3 - behind * 0.08));
    }

    const ramp = Math.min(elapsed / 1400, 1);
    cleanupEls.liveCounter.textContent =
      "+" + fmtBytes(Math.round(toss.expected * ramp));

    toss.raf = requestAnimationFrame(tossFrame);
  }

  function startToss(expectedBytes) {
    window.__cureTossSeen = true;
    tossInit();
    cleanupEls.stage.classList.remove("hidden");
    if (REDUCED) {
      tossSetOrb(ORB_REST[0], ORB_REST[1], 1, 1);
      return;
    }
    toss.active = true;
    toss.start = performance.now();
    toss.expected = Math.max(expectedBytes, 1);
    window.__cureTossActive = true;
    cleanupEls.liveCounter.classList.remove("hidden");
    toss.lid.setAttribute("transform", "");
    toss.raf = requestAnimationFrame(tossFrame);
  }

  function stopToss(freedBytes) {
    if (REDUCED || !toss.active) {
      if (REDUCED) tossSetOrb(ORB_REST[0], ORB_REST[1], 1, 1);
      return;
    }
    toss.active = false;
    cancelAnimationFrame(toss.raf);
    window.__cureTossActive = false;
    toss.lid.setAttribute("transform", "");
    toss.flash.setAttribute("opacity", "0");
    for (let i = 0; i < toss.glyphs.length; i++) {
      const [gx, gy] = GLYPH_BASE[i];
      tossGlyphTransform(i, gx, gy, 1, 1);
    }
    tossSetOrb(ORB_REST[0], ORB_REST[1], 1, 1);
    const trailEls = toss.trailG.children;
    for (const tr of trailEls) tr.setAttribute("opacity", "0");
    cleanupEls.liveCounter.classList.add("hidden");
  }

  function resetToss() {
    if (toss.active) {
      toss.active = false;
      cancelAnimationFrame(toss.raf);
    }
    if (toss.svg) {
      tossSetOrb(ORB_REST[0], ORB_REST[1], 1, 1);
      for (let i = 0; i < toss.glyphs.length; i++) {
        const [gx, gy] = GLYPH_BASE[i];
        tossGlyphTransform(i, gx, gy, 1, 1);
      }
    }
    cleanupEls.liveCounter.classList.add("hidden");
  }

  // ---- cleanup state / rendering -------------------------------------------

  function disarmCleanupButton() {
    cleanupState.armed = false;
    clearTimeout(cleanupState.armTimer);
    cleanupEls.status.textContent = "";
    cleanupEls.status.classList.add("hidden");
    updateCleanupButton();
  }

  function cleanupSelectionBytes() {
    let bytes = 0;
    for (const cat of cleanupState.summary.categories) {
      if (cat.item_count > 0 && cleanupState.selectedCats.has(cat.key)) {
        bytes += cat.total_bytes;
      }
    }
    for (const dl of cleanupState.summary.downloads) {
      if (cleanupState.checkedDownloads.has(dl.path)) bytes += dl.size_bytes;
    }
    return bytes;
  }

  function updateCleanupButton() {
    const s = cleanupState.summary;
    const anyCat =
      s &&
      s.categories.some(
        (c) => c.item_count > 0 && cleanupState.selectedCats.has(c.key)
      );
    const anyDl = cleanupState.checkedDownloads.size > 0;
    const enabled = (anyCat || anyDl) && !cleanupState.running;
    cleanupEls.btn.disabled = !enabled;
    cleanupEls.btn.classList.toggle("arm-danger", cleanupState.armed);
    if (!cleanupState.running) {
      cleanupEls.btn.classList.remove("btn-active");
    }
    cleanupEls.btn.textContent = cleanupState.armed
      ? "Really free " + fmtBytes(cleanupSelectionBytes()) + "?"
      : "Clean up";
  }

  function renderCleanup(summary, keepResult = false) {
    cleanupState.summary = summary;
    cleanupState.selectedCats = new Set();
    cleanupState.checkedDownloads = new Set();
    cleanupState.armed = false;
    clearTimeout(cleanupState.armTimer);
    cleanupState.running = false;

    cleanupEls.loading.classList.add("hidden");
    cleanupEls.body.classList.remove("hidden");
    if (!keepResult) {
      cleanupEls.status.textContent = "";
      cleanupEls.status.classList.add("hidden");
      cleanupEls.failures.classList.add("hidden");
      cleanupEls.failures.innerHTML = "";
    }

    const itemCount = summary.categories.reduce((n, c) => n + c.item_count, 0);
    cleanupEls.total.innerHTML =
      "≈ <b>" + fmtBytes(summary.total_bytes) + "</b> reclaimable across " +
      (itemCount + summary.downloads.length) + " items";
    cleanupEls.subline.textContent =
      itemCount + summary.downloads.length + " cleanable items found on this machine";

    cleanupEls.grid.innerHTML = "";
    for (const cat of summary.categories) {
      const card = document.createElement("button");
      card.type = "button";
      card.className = "cleanup-cat";
      card.dataset.key = cat.key;
      card.disabled = cat.item_count === 0;
      const on = cat.item_count > 0;
      if (on) cleanupState.selectedCats.add(cat.key);
      card.classList.toggle("on", on);
      card.classList.toggle("off", !on);
      card.title = on
        ? "Click to skip this category"
        : cat.item_count === 0
          ? "Nothing found in this category"
          : "Currently skipped — click to include";
      const name = document.createElement("span");
      name.className = "cc-name";
      name.textContent = cat.label;
      const size = document.createElement("span");
      size.className = "cc-size";
      size.textContent = fmtBytes(cat.total_bytes);
      const count = document.createElement("span");
      count.className = "cc-count";
      count.textContent =
        cat.item_count + (cat.item_count === 1 ? " item" : " items");
      card.append(name, size, count);
      card.addEventListener("click", () => {
        if (card.disabled) return;
        const nowOn = !cleanupState.selectedCats.has(cat.key);
        if (nowOn) cleanupState.selectedCats.add(cat.key);
        else cleanupState.selectedCats.delete(cat.key);
        card.classList.toggle("on", nowOn);
        card.classList.toggle("off", !nowOn);
        card.title = nowOn ? "Click to skip this category" : "Currently skipped — click to include";
        disarmCleanupButton();
      });
      cleanupEls.grid.appendChild(card);
    }

    cleanupEls.downloads.classList.toggle(
      "hidden",
      summary.downloads.length === 0
    );
    cleanupEls.dlList.innerHTML = "";
    for (const dl of summary.downloads) {
      const li = document.createElement("li");
      li.className = "dl-item";
      const box = document.createElement("input");
      box.type = "checkbox";
      box.dataset.path = dl.path;
      box.addEventListener("change", () => {
        if (box.checked) cleanupState.checkedDownloads.add(dl.path);
        else cleanupState.checkedDownloads.delete(dl.path);
        disarmCleanupButton();
      });
      const name = document.createElement("span");
      name.className = "dl-name";
      name.textContent = dl.name;
      name.title = dl.path;
      const meta = document.createElement("span");
      meta.className = "dl-meta";
      meta.textContent = fmtBytes(dl.size_bytes) + " · " + dl.age_days + "d old";
      li.append(box, name, meta);
      cleanupEls.dlList.appendChild(li);
    }

    updateCleanupButton();
  }

  async function startCleanupScan(keepResult = false) {
    cleanupEls.idle.classList.add("hidden");
    cleanupEls.body.classList.add("hidden");
    cleanupEls.loading.classList.remove("hidden");
    cleanupEls.loading.textContent = "measuring reclaimable space…";
    if (!keepResult) {
      setCleanupPill("scanning", "measuring reclaimable space…");
    }
    try {
      const summary = await invoke("scan_cleanup");
      renderCleanup(summary, keepResult);
      if (!keepResult) {
        setCleanupPill(
          "clean",
          "Disk scan complete — " + fmtBytes(summary.total_bytes) + " reclaimable"
        );
      }
    } catch (err) {
      cleanupEls.loading.textContent =
        "disk cleanup unavailable: " + cleanErrText(err, String(err));
      if (!keepResult) {
        setCleanupPill("error", "Disk cleanup unavailable");
      }
    }
  }

  function showCleanupIdle() {
    resetToss();
    cleanupEls.stage.classList.add("hidden");
    cleanupEls.body.classList.add("hidden");
    cleanupEls.loading.classList.add("hidden");
    cleanupEls.idle.classList.remove("hidden");
    setCleanupPill("idle", "Disk cleanup — ready when you are");
  }

  async function openCleanup() {
    if (cleanupState.open) return;
    cleanupState.open = true;
    cleanupState.savedPill = {
      cls: statusPill.className,
      text: statusText.textContent,
    };
    showCleanupIdle();
    await switchView(resultsView, cleanupView);
  }

  function closeCleanup() {
    if (!cleanupState.open) return;
    cleanupState.open = false;
    disarmCleanupButton();
    resetToss();
    cleanupEls.stage.classList.add("hidden");
    if (cleanupState.savedPill) {
      statusPill.className = cleanupState.savedPill.cls;
      statusText.textContent = cleanupState.savedPill.text;
    }
    switchView(cleanupView, resultsView);
  }

  cleanupEls.openBtn.addEventListener("click", openCleanup);
  cleanupEls.backBtn.addEventListener("click", closeCleanup);
  cleanupEls.scanBtn.addEventListener("click", () => startCleanupScan(false));

  cleanupEls.btn.addEventListener("click", async () => {
    if (cleanupState.running || cleanupEls.btn.disabled) return;
    if (!cleanupState.armed) {
      cleanupState.armed = true;
      updateCleanupButton();
      clearTimeout(cleanupState.armTimer);
      cleanupState.armTimer = setTimeout(disarmCleanupButton, 4000);
      return;
    }
    cleanupState.running = true;
    clearTimeout(cleanupState.armTimer);
    cleanupState.armed = false;
    cleanupEls.btn.disabled = true;
    cleanupEls.btn.classList.remove("arm-danger");
    cleanupEls.btn.classList.add("btn-active");
    cleanupEls.btn.textContent = "Cleaning…";
    const expected = cleanupSelectionBytes();
    startToss(expected);
    try {
      const result = await invoke("run_cleanup", {
        categories: Array.from(cleanupState.selectedCats),
        downloadPaths: Array.from(cleanupState.checkedDownloads),
      });
      stopToss(result.bytes_freed);
      setCleanupPill(
        result.failed ? "warn" : "clean",
        "Freed " + fmtBytes(result.bytes_freed) +
          (result.failed ? " — " + result.failed + " item(s) locked or failed" : "")
      );
      cleanupEls.status.textContent =
        "Freed " + fmtBytes(result.bytes_freed) +
        " — deleted " + result.deleted + " of " + result.attempted +
        (result.failed ? ", " + result.failed + " locked or failed" : "");
      cleanupEls.status.classList.remove("hidden");
      if (result.failures.length > 0) {
        cleanupEls.failures.innerHTML = "";
        for (const failure of result.failures) {
          const li = document.createElement("li");
          li.textContent = failure.path + " — " + failure.reason;
          cleanupEls.failures.appendChild(li);
        }
        cleanupEls.failures.classList.remove("hidden");
      }
    } catch (err) {
      stopToss(0);
      setCleanupPill("error", "Disk cleanup failed");
      cleanupEls.status.textContent =
        "cleanup failed: " + cleanErrText(err, String(err));
      cleanupEls.status.classList.remove("hidden");
    } finally {
      cleanupState.running = false;
      cleanupEls.btn.classList.remove("btn-active");
      updateCleanupButton();
      startCleanupScan(true);
    }
  });

  var killBtn = document.getElementById("kill-procs-btn");
  var killStatus = document.getElementById("kill-procs-status");
  if (killBtn) {
    killBtn.addEventListener("click", async function() {
      if (sweepState.running || killBtn.disabled) return;
      if (!sweepState.armed) {
        sweepState.armed = true;
        updateKillButton();
        clearTimeout(sweepState.armTimer);
        sweepState.armTimer = setTimeout(disarmKillButton, 4000);
        return;
      }
      sweepState.running = true;
      clearTimeout(sweepState.armTimer);
      sweepState.armed = false;
      killBtn.disabled = true;
      killBtn.classList.remove("arm-danger");
      killBtn.classList.add("btn-active");
      killBtn.textContent = "Killing…";
      if (killStatus) { killStatus.textContent = ""; killStatus.classList.add("hidden"); }
      var targets = [];
      sweepState.findings.forEach(function(f) {
        if (sweepState.checked.has(f.pid)) targets.push([f.name, f.pid]);
      });
      try {
        var report = await invoke("kill_high_risk_processes", { processes: targets });
        var killed = report.killed || [];
        var failed = report.failed || [];
        if (killed.length > 0) {
          var pidSet = new Set(killed.map(function(k) { return k.pid; }));
          var cards = document.querySelectorAll("#process-cards .review-card.proc-entry");
          cards.forEach(function(card) {
            if (pidSet.has(Number(card.dataset.pid))) {
              card.classList.add("proc-killed");
              var cb = card.querySelector('input[type="checkbox"]');
              if (cb) cb.disabled = true;
            }
          });
        }
        sweepState.checked.clear();
        var parts = [];
        if (killed.length > 0) parts.push("Killed " + killed.length + " process(es)");
        if (failed.length > 0) parts.push(failed.length + " failed");
        var msg = parts.join(", ") || "No processes were killed";
        setPill(killed.length > 0 && failed.length === 0 ? "clean" : "warn", msg);
        if (killStatus) { killStatus.textContent = msg; killStatus.classList.remove("hidden"); }
      } catch (err) {
        setPill("error", "Kill failed: " + String(err));
        if (killStatus) { killStatus.textContent = "Error: " + String(err); killStatus.classList.remove("hidden"); }
      } finally {
        sweepState.running = false;
        killBtn.classList.remove("btn-active");
        updateKillButton();
      }
    });
  }

  (async () => {
    scanView.classList.add("hidden");
    landingView.classList.remove("hidden");
    document.getElementById("start-rescue-btn").addEventListener("click", async () => {
      setPill("scanning", "checking for suspicious overlays…");
      await switchView(landingView, scanView);
      const lines = [];
      try {
        const rep = await invoke("dismiss_overlays");
        if (rep.closed && rep.closed.length > 0) {
          lines.push("Closed " + rep.closed.length + " suspicious window(s): " + rep.closed.map(function (c) { return c.process; }).join(", "));
          for (const c of rep.closed) {
            lines.push("  ↳ closed " + c.title + " — " + c.signature + (c.terminated ? " (terminated)" : ""));
          }
        } else {
          lines.push("No suspicious overlay windows found");
        }
      } catch (e) {
        lines.push("Overlay check unavailable: " + e);
      }
      runScan(lines);
    });
  })();
})();
