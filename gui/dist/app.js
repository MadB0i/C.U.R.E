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

  // ---- UI V2 session state (all derived from real backend responses) ----
  // lastSummary/lastScanAt/scanDurationMs are set only by runScan success.
  // eventLog holds session events (scan progress, canary alerts, actions).
  let lastSummary = null;
  let lastScanAt = null;
  let scanDurationMs = null;
  let scanStartedAt = 0;
  let scanPhase = "idle"; // idle | running | done
  const eventLog = [];
  const canarySessionAlerts = [];
  let canaryTriggered = false;

  function logEvent(kind, text) {
    eventLog.push({ at: new Date(), kind: kind || "info", text: String(text) });
    if (eventLog.length > 300) eventLog.splice(0, eventLog.length - 300);
    appendEventLogRow(eventLog[eventLog.length - 1]);
  }

  let scanToken = 0;
  let itemFeedCount = 0;
  // Items successfully quarantined this session (explicit confirm path
  // only — the backend never auto-cleans). Decremented on scoped undo.
  let sessionQuarantined = 0;
  function refreshQuarantinedTile() {
    const el = document.getElementById("stat-cleaned");
    if (el) el.textContent = String(sessionQuarantined);
  }

  function escHtml(s) {
    return String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  }

  function setPill(state, text) {
    statusPill.className = "pill " + state;
    statusText.textContent = text;
  }

  // Topbar session context — real scan metadata only, never invented.
  function renderSessionMeta(summary) {
    const el = document.getElementById("session-meta");
    if (!el) return;
    if (!summary) {
      el.textContent = "STANDBY — NO ACTIVE SESSION";
      return;
    }
    const review = (summary.suspicious_for_review || []).length;
    const when = lastScanAt ? lastScanAt.toLocaleTimeString() : "—";
    el.textContent = "LAST SCAN " + when + " · " + summary.total + " CHECKED · " + review + " TO REVIEW";
  }

  // Topbar clock — real local system time, refreshed every 15 s.
  function tickClock() {
    const el = document.getElementById("topbar-clock");
    if (!el) return;
    el.textContent = new Date().toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
  }
  tickClock();
  setInterval(tickClock, 15000);

  // Placed action receipt: quarantine/undo outcomes get a stable, inline
  // home in the results view (instead of only a floating toast).
  function showReceipt(title, detail, isError) {
    const box = document.getElementById("action-receipt");
    const txt = document.getElementById("action-receipt-text");
    if (!box || !txt) return;
    txt.textContent = title + " — " + detail + " · " + new Date().toLocaleTimeString();
    box.classList.toggle("error", !!isError);
    box.classList.remove("hidden");
  }
  function hideReceipt() {
    const box = document.getElementById("action-receipt");
    if (box) box.classList.add("hidden");
  }

  let typeChain = Promise.resolve();
  function appendLog(stage, message) {
    const li = document.createElement("li");
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
      // face: two dot eyes with pupils (eyes widen on impact)
      const eR = r * 0.27, pR = r * 0.11, eW = t >= 1 ? 1.12 : 1;
      ctx.fillStyle = "rgba(255,255,255,0.88)";
      ctx.beginPath(); ctx.arc(-r * 0.34, -r * 0.12, eR * eW, 0, Math.PI * 2); ctx.fill();
      ctx.beginPath(); ctx.arc( r * 0.34, -r * 0.12, eR * eW, 0, Math.PI * 2); ctx.fill();
      ctx.fillStyle = "#1a1a2e";
      ctx.beginPath(); ctx.arc(-r * 0.34, -r * 0.12, pR, 0, Math.PI * 2); ctx.fill();
      ctx.beginPath(); ctx.arc( r * 0.34, -r * 0.12, pR, 0, Math.PI * 2); ctx.fill();
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
      else if (!REDUCED) {
        el.style.transition = "color 0.4s ease";
        el.style.color = "var(--accent)";
        requestAnimationFrame(() => { el.style.color = ""; });
      }
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
    for (let i = 0; i < 16; i++) {
      motes.push({
        rf: 0.14 + Math.random() * 0.42,
        sp: (0.0003 + Math.random() * 0.0007) * (i % 2 ? 1 : -1),
        ph: Math.random() * Math.PI * 2,
        a: 0.07 + Math.random() * 0.14,
        tw: 0.6 + Math.random() * 1.8,
        sz: 0.8 + Math.random() * 0.9,
      });
    }
    // premium hover probe (tooltip) — purely visual, never affects counts
    let hovered = null;
    const tooltipEl = document.getElementById("map-tooltip");
    function updateMapLegend() {
      try {
        const all = NetStore.all();
        let hi = 0, su = 0, sa = 0;
        for (const n of all) {
          if (n.risk === "HighRisk") hi++;
          else if (n.risk === "Suspicious") su++;
          else sa++;
        }
        const eH = document.getElementById("map-n-high");
        const eS = document.getElementById("map-n-susp");
        const eF = document.getElementById("map-n-safe");
        if (eH) eH.textContent = String(hi);
        if (eS) eS.textContent = String(su);
        if (eF) eF.textContent = String(sa);
        const fill = document.getElementById("map-threat-fill");
        const txt = document.getElementById("map-threat-text");
        const total = all.length || 1;
        const score = Math.min(100, Math.round(((hi * 1 + su * 0.45) / total) * 100));
        if (fill) fill.style.width = score + "%";
        if (txt) {
          txt.textContent = all.length === 0 ? "—" : hi > 0 ? "HIGH " + score + "%" : su > 0 ? "WATCH " + score + "%" : "SECURE";
          txt.style.color = hi > 0 ? "#f4928a" : su > 0 ? "#e8c476" : "#7fd8ab";
        }
      } catch (_) { /* legend is decorative */ }
    }
    if (stageEl && !stageEl.__cureMapHoverWired) {
      stageEl.__cureMapHoverWired = true;
      stageEl.addEventListener("mousemove", (ev) => {
        if (!W || !H) return;
        const rect = canvas.getBoundingClientRect();
        const mx = ((ev.clientX - rect.left) / Math.max(rect.width, 1)) * W;
        const my = ((ev.clientY - rect.top) / Math.max(rect.height, 1)) * H;
        let best = null, bestD = 16 * DPR;
        for (const nd of NetStore.all()) {
          const p = nodeXY(nd);
          const d = Math.hypot(p.x - mx, p.y - my);
          if (d < bestD) { bestD = d; best = { nd, x: p.x, y: p.y }; }
        }
        hovered = best;
        if (tooltipEl) {
          if (best) {
            const rk = best.nd.risk === "HighRisk" ? "high" : best.nd.risk === "Suspicious" ? "suspicious" : "safe";
            tooltipEl.innerHTML = "";
            const nm = document.createElement("div");
            nm.textContent = String(best.nd.name || "?").slice(0, 60);
            const rs = document.createElement("div");
            rs.innerHTML = '<span class="tt-risk-' + rk + '">' + String(best.nd.risk || "?") + "</span>";
            tooltipEl.append(nm, rs);
            tooltipEl.classList.remove("hidden");
            const sx = (best.x / W) * rect.width;
            const sy = (best.y / H) * rect.height;
            tooltipEl.style.left = Math.min(Math.max(sx + 12, 4), Math.max(rect.width - 170, 4)) + "px";
            tooltipEl.style.top = Math.min(Math.max(sy - 10, 4), Math.max(rect.height - 60, 4)) + "px";
          } else {
            tooltipEl.classList.add("hidden");
          }
        }
        stageEl.style.cursor = best ? "crosshair" : "";
      });
      stageEl.addEventListener("mouseleave", () => {
        hovered = null;
        if (tooltipEl) tooltipEl.classList.add("hidden");
        if (stageEl) stageEl.style.cursor = "";
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

      // futuristic nebula wash: violet core + cyan off-axis + faint red depth
      const neb = ctx.createRadialGradient(CX, CY, 0, CX, CY, M * 1.05);
      neb.addColorStop(0, "rgba(139,124,246,0.075)");
      neb.addColorStop(0.45, "rgba(139,124,246,0.022)");
      neb.addColorStop(0.7, "rgba(80,200,220,0.018)");
      neb.addColorStop(1, "rgba(139,124,246,0)");
      ctx.fillStyle = neb;
      ctx.fillRect(0, 0, W, H);
      const neb2 = ctx.createRadialGradient(CX + RX * 0.45, CY - R * 0.4, 0, CX + RX * 0.45, CY - R * 0.4, M * 0.55);
      neb2.addColorStop(0, "rgba(80,200,220,0.05)");
      neb2.addColorStop(1, "rgba(80,200,220,0)");
      ctx.fillStyle = neb2;
      ctx.fillRect(0, 0, W, H);

      const glow = ctx.createRadialGradient(CX, CY, 0, CX, CY, M);
      glow.addColorStop(0, "rgba(124,108,240,0.05)");
      glow.addColorStop(0.5, "rgba(124,108,240,0.016)");
      glow.addColorStop(1, "rgba(124,108,240,0)");
      ctx.fillStyle = glow;
      ctx.fillRect(0, 0, W, H);

      ctx.lineWidth = 1 * DPR;
      // rotating dashed orbit rings — holographic depth
      const rot = REDUCED ? 0 : now / 9000;
      ctx.save();
      ctx.translate(CX, CY);
      ctx.rotate(rot * 0.35);
      ctx.strokeStyle = ACCENT(0.10);
      ctx.setLineDash([5 * DPR, 7 * DPR]);
      ellipse(0, 0, RX, R);
      ctx.restore();
      ctx.save();
      ctx.translate(CX, CY);
      ctx.rotate(-rot * 0.5);
      ctx.strokeStyle = ACCENT(0.07);
      ctx.setLineDash([2.5 * DPR, 6 * DPR]);
      ellipse(0, 0, RX * 0.62, R * 0.62);
      ctx.restore();
      ctx.setLineDash([]);
      ctx.strokeStyle = ACCENT(0.05);
      ellipse(CX, CY, RX * 0.34, R * 0.34);
      // faint crosshair ticks
      ctx.strokeStyle = ACCENT(0.05);
      ctx.beginPath();
      for (let k = 0; k < 4; k++) {
        const a = (k / 4) * Math.PI * 2 + (REDUCED ? 0 : now / 14000);
        ctx.moveTo(CX + Math.cos(a) * RX * 0.34, CY + Math.sin(a) * R * 0.34);
        ctx.lineTo(CX + Math.cos(a) * RX * 0.38, CY + Math.sin(a) * R * 0.38);
      }
      ctx.stroke();

      for (const m of motes) {
        const a = m.ph + m.sp * now;
        const x = CX + Math.cos(a) * RX * m.rf;
        const y = CY + Math.sin(a) * R * m.rf;
        const tw = REDUCED ? 1 : 0.55 + 0.45 * Math.sin(now / (700 * m.tw) + m.ph * 3);
        ctx.fillStyle = ACCENT((m.a * tw).toFixed(3));
        ctx.beginPath();
        ctx.arc(x, y, m.sz * DPR, 0, Math.PI * 2);
        ctx.fill();
      }
    }

    function drawCore(now) {
      const k = Math.min(1.2, Math.max(0.6, Math.min(RX, R) / 260));
      const breathe =
        REDUCED ? 0.5 : 0.5 + 0.5 * Math.sin(now / 2100);
      const haloR = (14 + 10 * breathe) * k * DPR;
      const halo = ctx.createRadialGradient(CX, CY, 0, CX, CY, haloR);
      halo.addColorStop(0, ACCENT((0.24 + 0.12 * breathe).toFixed(3)));
      halo.addColorStop(0.6, ACCENT((0.08 + 0.05 * breathe).toFixed(3)));
      halo.addColorStop(1, ACCENT(0));
      ctx.fillStyle = halo;
      ctx.beginPath();
      ctx.arc(CX, CY, haloR, 0, Math.PI * 2);
      ctx.fill();

      // holographic double ring
      ctx.lineWidth = 1.2 * DPR;
      ctx.strokeStyle = ACCENT(REDUCED ? 0.38 : 0.38 + 0.14 * breathe);
      ring(CX, CY, 12.5 * k * DPR);
      if (!REDUCED) {
        ctx.save();
        ctx.strokeStyle = ACCENT(0.5);
        ctx.setLineDash([6 * DPR, 5 * DPR]);
        ctx.lineDashOffset = -now / 60;
        ring(CX, CY, 17 * k * DPR);
        ctx.restore();
        ctx.strokeStyle = "rgba(155,231,244," + (0.22 + 0.12 * breathe).toFixed(3) + ")";
        ctx.lineWidth = 1 * DPR;
        ring(CX, CY, 8 * k * DPR);
      } else {
        ctx.strokeStyle = ACCENT(0.3);
        ring(CX, CY, 17 * k * DPR);
      }

      const cg = ctx.createRadialGradient(CX - 1 * DPR, CY - 1 * DPR, 0, CX, CY, 4 * k * DPR);
      cg.addColorStop(0, "#e6e1ff");
      cg.addColorStop(0.5, "#7c6cf0");
      cg.addColorStop(1, "#4a3fa0");
      ctx.fillStyle = cg;
      ctx.shadowColor = "rgba(124, 108, 240, 0.6)";
      ctx.shadowBlur = (5 + 3 * breathe) * DPR;
      ctx.beginPath();
      ctx.arc(CX, CY, (2.8 + 0.8 * breathe) * k * DPR, 0, Math.PI * 2);
      ctx.fill();
      ctx.shadowBlur = 0;
    }

    function drawGraph(now) {
      const nodes = NetStore.all();
      ctx.textBaseline = "middle";
      for (let ni = 0; ni < nodes.length; ni++) {
        const nd = nodes[ni];
        const p = nodeXY(nd);
        const c = RGB[nd.risk] || RGB.Safe;
        const gapX = CX + Math.cos(nd.ang) * 9 * DPR;
        const gapY = CY + Math.sin(nd.ang) * 9 * DPR;
        const isHover = hovered && hovered.nd === nd;
        const isThreat = nd.risk === "HighRisk";
        const isSusp = nd.risk === "Suspicious";
        const pulse = REDUCED ? 0 : 0.5 + 0.5 * Math.sin(now / 620 + ni * 1.7);

        // premium connector: gradient beam with soft glow
        const beam = ctx.createLinearGradient(gapX, gapY, p.x, p.y);
        beam.addColorStop(0, ACCENT(0.05));
        beam.addColorStop(1, rgba(c, isThreat ? 0.42 : isSusp ? 0.32 : 0.20));
        ctx.strokeStyle = beam;
        ctx.lineWidth = (isHover ? 1.8 : isThreat ? 1.4 : 1) * DPR;
        ctx.shadowColor = rgba(c, 0.35);
        ctx.shadowBlur = (isThreat ? 5 : 2) * DPR;
        ctx.beginPath();
        ctx.moveTo(gapX, gapY);
        ctx.lineTo(p.x, p.y);
        ctx.stroke();
        ctx.shadowBlur = 0;

        // threat echo rings on high-risk nodes
        if (isThreat && !REDUCED) {
          const ph = ((now / 1400) + ni * 0.23) % 1;
          ctx.lineWidth = 1 * DPR;
          ctx.strokeStyle = rgba(c, ((1 - ph) * 0.4).toFixed(3));
          ring(p.x, p.y, (dotRadius(nd.risk) + ph * 11) * DPR);
        }
        // hover halo
        if (isHover) {
          ctx.lineWidth = 1.4 * DPR;
          ctx.strokeStyle = rgba(c, 0.65);
          ring(p.x, p.y, (dotRadius(nd.risk) + 5.5) * DPR);
        }

        const baseR = dotRadius(nd.risk) * (isHover ? 1.35 : 1) * (isThreat && !REDUCED ? 1 + 0.14 * pulse : 1);
        // outer aura
        const aura = ctx.createRadialGradient(p.x, p.y, 0, p.x, p.y, baseR * 3.2 * DPR);
        aura.addColorStop(0, rgba(c, isThreat ? 0.5 : 0.32));
        aura.addColorStop(1, rgba(c, 0));
        ctx.fillStyle = aura;
        ctx.beginPath();
        ctx.arc(p.x, p.y, baseR * 3.2 * DPR, 0, Math.PI * 2);
        ctx.fill();
        // core dot
        ctx.fillStyle = rgba(c, 0.95);
        ctx.shadowColor = rgba(c, 0.85);
        ctx.shadowBlur = (isThreat ? 7 : 4) * DPR;
        ctx.beginPath();
        ctx.arc(p.x, p.y, baseR * DPR, 0, Math.PI * 2);
        ctx.fill();
        ctx.shadowBlur = 0;
        // specular highlight
        ctx.fillStyle = "rgba(255,255,255,0.85)";
        ctx.beginPath();
        ctx.arc(p.x - baseR * 0.3 * DPR, p.y - baseR * 0.3 * DPR, Math.max(0.7 * DPR, baseR * 0.28 * DPR), 0, Math.PI * 2);
        ctx.fill();

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
      // face: two dot eyes with patrol-aware pupils
      const eR = r * 0.27, pR = r * 0.11, eW = guarding ? 1.12 : 1;
      ctx.fillStyle = "rgba(255,255,255,0.88)";
      ctx.beginPath(); ctx.arc(x - r * 0.34, y - r * 0.12, eR * eW, 0, Math.PI * 2); ctx.fill();
      ctx.beginPath(); ctx.arc(x + r * 0.34, y - r * 0.12, eR * eW, 0, Math.PI * 2); ctx.fill();
      let pdx = 0, pdy = 0;
      if (!guarding) {
        const _pt = now / 1000;
        pdx = Math.cos(_pt * 0.42) * pR * 0.4;
        pdy = Math.sin(_pt * 0.84 + 1.2) * pR * 0.3;
      }
      ctx.fillStyle = "#1a1a2e";
      ctx.beginPath(); ctx.arc(x - r * 0.34 + pdx, y - r * 0.12 + pdy, pR, 0, Math.PI * 2); ctx.fill();
      ctx.beginPath(); ctx.arc(x + r * 0.34 + pdx, y - r * 0.12 + pdy, pR, 0, Math.PI * 2); ctx.fill();
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
        updateMapLegend();
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
    WindowsService:
      '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3"/><path d="M12 2v4M12 18v4M2 12h4M18 12h4"/></svg>',
    WmiSubscription:
      '<svg viewBox="0 0 24 24"><ellipse cx="12" cy="6" rx="7" ry="3"/><path d="M5 6v12c0 1.7 3.1 3 7 3s7-1.3 7-3V6"/><path d="M5 12c0 1.7 3.1 3 7 3s7-1.3 7-3"/></svg>',
    IfeoDebugger:
      '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3"/><path d="M12 5V2M12 22v-3M5 12H2M22 12h-3"/></svg>',
    AppInitDlls:
      '<svg viewBox="0 0 24 24"><path d="M4 7h16M4 12h16M4 17h10"/></svg>',
    ComHijack:
      '<svg viewBox="0 0 24 24"><rect x="4" y="4" width="16" height="16" rx="3"/><path d="M9 12h6"/></svg>',
  };

  const SOURCE_LABELS = {
    StartupFolder: "Startup folder",
    ScheduledTask: "Scheduled task",
    RegistryRun: "Registry run",
    WindowsService: "Windows service",
    WmiSubscription: "WMI subscription",
    IfeoDebugger: "IFEO debugger",
    AppInitDlls: "AppInit DLLs",
    ComHijack: "COM hijack",
  };

  // Fallback only: the backend attaches {id, name} per finding (single
  // source of truth) and the UI prefers it — see attackFor().
  const ATTACK_MAP = {
    StartupFolder: { id: "T1547.001", name: "Startup Folder" },
    ScheduledTask: { id: "T1053.005", name: "Scheduled Task" },
    RegistryRun: { id: "T1547.001", name: "Registry Run Keys" },
    WindowsService: { id: "T1543.003", name: "Windows Service" },
    WmiSubscription: { id: "T1546.003", name: "WMI Event Subscription" },
    IfeoDebugger: { id: "T1546.012", name: "Image File Execution Options Injection" },
    AppInitDlls: { id: "T1546.010", name: "AppInit DLLs" },
    ComHijack: { id: "T1546.015", name: "Component Object Model Hijacking" },
  };

  function attackFor(scored) {
    if (scored && scored.attack && scored.attack.id) {
      return { id: scored.attack.id, name: scored.attack.name || "" };
    }
    const raw = scored && scored.entry ? String(scored.entry.source) : "";
    return ATTACK_MAP[raw] || null;
  }

  function recommendedAction(scored) {
    if (!scored) return "";
    if (scored.risk === "HighRisk") {
      const src = scored.entry ? String(scored.entry.source) : "";
      if (src === "StartupFolder" || src === "ScheduledTask") {
        return "Quarantine for review, then investigate before any deletion.";
      }
      return "Investigate before disabling. Back up the location first; removal is manual and must be reversible.";
    }
    if (scored.risk === "Suspicious") return "Investigate before disabling.";
    return "No action needed.";
  }

  function findingsHeadline(n) {
    return n === 1
      ? "1 finding needs a decision"
      : n + " findings need a decision";
  }

  // Evidence-based scope caption: what was actually checked, what was
  // skipped, and when. Never states more than the backend established.
  function scopeLine(summary) {
    const states = (summary && summary.source_states) || [];
    let skipped = 0;
    for (const row of states) {
      const st = row.state;
      if (st && typeof st === "object" && "Partial" in st && st.Partial) {
        skipped += st.Partial.skipped || 0;
      }
    }
    let s = summary.total + " entries checked";
    s += " · " + skipped + " skipped";
    if (lastScanAt) s += " · " + lastScanAt.toLocaleTimeString();
    return s;
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
    if (lower.includes("missing from disk")) return ["Missing image", "red"];
    if (lower.includes("class redirection")) return ["COM redirect", "amber"];
    if (lower.includes("service starts")) return ["Auto-start", ""];
    if (lower.includes("runs as")) return ["Account", ""];
    if (lower.includes("state at scan")) return ["State", ""];
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
    card.className = "review-card reveal" + (cleaned ? " cleaned" : "") + " risk-" + scoreChipClass(entry.score);

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
    const atk = attackFor(entry);
    if (atk) {
      const atkChip = document.createElement("span");
      atkChip.className = "chip attack";
      atkChip.textContent = atk.id;
      atkChip.title = "MITRE ATT&CK: " + atk.name;
      chips.appendChild(atkChip);
    }
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

    const loc = document.createElement("div");
    loc.className = "finding-loc selectable trunc";
    loc.tabIndex = 0;
    loc.textContent = "Location: " + (entry.entry.location || "Not collected");
    loc.title = entry.entry.location || "Not collected";
    main.appendChild(loc);

    const detToggle = document.createElement("button");
    detToggle.className = "detail-toggle finding-details-toggle";
    detToggle.textContent = "View details";
    detToggle.setAttribute("aria-expanded", "false");
    const drawer = document.createElement("dl");
    drawer.className = "detail-drawer hidden";
    const dRows = [
      ["Category", "Persistence"],
      ["Source", rawSource + (atk ? " · MITRE " + atk.id + " " + atk.name : "")],
      ["Severity / status", entry.risk + " · score " + entry.score],
      ["Name", entry.entry.name],
      ["Location", entry.entry.location || "Not collected"],
      ["Command", entry.entry.command || "Not collected"],
      ["Reasons", reasons.join(" · ") || "No scored signals — listed for context."],
      ["Available evidence", reasons.join("; ") || "No scored signals — listed for context."],
      ["Action", cleaned ? "Quarantined — restore from Quarantine with Undo." : (rawSource === "RegistryRun" ? "Manual removal required — registry values are not auto-disabled in this version." : "Quarantine available — asks for confirmation, reversible from Quarantine.")],
    ];
    for (const [k, v] of dRows) {
      const dt = document.createElement("dt");
      dt.textContent = k;
      const dd = document.createElement("dd");
      dd.className = "selectable";
      dd.textContent = v;
      if (String(v).length > 60) dd.title = v;
      drawer.append(dt, dd);
    }
    detToggle.addEventListener("click", () => {
      const open = drawer.classList.toggle("hidden");
      detToggle.setAttribute("aria-expanded", String(!open));
      detToggle.textContent = open ? "View details" : "Hide details";
    });
    main.appendChild(detToggle);
    main.appendChild(drawer);

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
        const ok = await requestConfirm({
          kicker: "Confirm quarantine",
          title: "Move this item to quarantine?",
          facts: [
            ["Item", entry.entry.name],
            ["Location", entry.entry.location || "Not collected"],
            ["Action", "Move this file to C.U.R.E. quarantine."],
            ["Reversible", "Yes — it can be restored from Quarantine."],
          ],
          note: "This action changes the filesystem. The item will be listed under Quarantine with Undo.",
          okLabel: "Quarantine item",
        });
        if (!ok) return;
        btn.disabled = true;
        try {
          await invoke("quarantine_entry", {
            id: entry.entry.id,
            name: entry.entry.name,
            command: entry.entry.command,
          });
          btn.textContent = "Quarantined ✓";
          btn.classList.add("row-done");
          sessionQuarantined += 1;
          refreshQuarantinedTile();
          showReceipt("Quarantined: " + entry.entry.name, "moved to quarantine · Undo in the Quarantine view", false);
          logEvent("action", "quarantined: " + entry.entry.name);
        } catch (err) {
          btn.disabled = false;
          setPill("error", String(err));
          showReceipt("Quarantine failed", cleanErrText(err, "Quarantine failed"), true);
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

  // Results coverage strip: same backend source_states as Overview, but
  // compact. PARTIAL is amber, CHECK FAILED / ACCESS DENIED are red,
  // UNAVAILABLE is grey — a partial scan never resembles a fully checked one.
  function renderResultsCoverage(summary) {
    const cov = document.getElementById("res-coverage");
    if (!cov) return;
    cov.innerHTML = "";
    const states = (summary && summary.source_states) || [];
    if (!states.length) {
      cov.append(covRow("Coverage", "per-source states not reported for this scan", "idle"));
      return;
    }
    const levelOf = (st) => {
      // Real backend CoverageState serializes as "Checked" / "NotChecked" /
      // "Unavailable" / "CheckFailed" / "AccessDenied" strings or
      // {"Partial": {"skipped": N}}; the dev mock uses "Available".
      if (st === "Available" || st === "Checked") return "ok";
      if (st === "Unavailable" || st === "NotChecked") return "idle";
      if (st === "CheckFailed" || st === "AccessDenied") return "bad";
      if (st && typeof st === "object") {
        if ("Partial" in st) return "warn";
        if ("CheckFailed" in st || "AccessDenied" in st) return "bad";
        if ("Unavailable" in st || "NotChecked" in st) return "idle";
        if ("Available" in st || "Checked" in st) return "ok";
      }
      return "idle";
    };
    for (const row of states) {
      const st = row.state;
      const info = sourceStatusInfo(st);
      cov.append(covRow(row.area || "source", info.label + (info.detail && info.detail !== "fully enumerated" ? " — " + info.detail : "") + (row.detail ? " · " + row.detail : ""), levelOf(st)));
    }
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

    badge.className = "rakshak-sm reveal";
    const dot = document.getElementById("badge-dot");
    if (dot) {
      dot.setAttribute("class", "badge-dot " + (trouble ? "warn" : "clean"));
    }

    const subline = document.getElementById("subline");
    const scope = scopeLine(summary);
    if (reviewCount > 0) {
      headline.textContent = findingsHeadline(reviewCount);
      subline.textContent = scope;
    } else {
      headline.textContent = "No findings detected in this scan";
      subline.textContent = scope;
    }
    renderResultsCoverage(summary);

    countUp(document.getElementById("stat-cleaned"), sessionQuarantined);
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
      updateKillButton();
      const procCards = document.getElementById("process-cards");
      if (procCards) {
        procCards.innerHTML = "";
        procFindings.forEach(function(p) {
          const card = document.createElement("li");
          card.className = "review-card proc-entry risk-" + scoreChipClass(p.score);
          card.dataset.name = p.name;
          card.dataset.pid = String(p.pid);
          card.dataset.exe = p.exe_path || "";
          const box = document.createElement("input");
          box.type = "checkbox";
          box.setAttribute("aria-label", "Select " + p.name + " (pid " + p.pid + ") for termination");
          box.addEventListener("change", function() {
            if (box.checked) sweepState.checked.add(p.pid);
            else sweepState.checked.delete(p.pid);
            updateKillButton();
            const killStatusEl = document.getElementById("kill-procs-status");
            if (killStatusEl) { killStatusEl.textContent = ""; killStatusEl.classList.add("hidden"); }
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
      setPill("clean", "Scan complete — no findings");
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
      rkStatus.innerHTML = '<span class="rk-name">Rakshak</span> checked ' +
        summary.total + " node" + (summary.total === 1 ? "" : "s");
    }

    const revealables = resultsView.querySelectorAll(".reveal");
    if (!REDUCED && revealables.length > 0) {
      void resultsView.offsetWidth;
    }
    netmap.show(summary.total);
    syncCanaryStatus();
    enablePostScanNav();
    renderOverview(summary);
    renderAudit(summary);
    renderProcesses(summary);
    if (!resultsView.classList.contains("hidden")) focusViewHeading(resultsView);
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

  function currentView() {
    const views = document.querySelectorAll(".stage > .view");
    for (const v of views) {
      if (!v.classList.contains("hidden")) return v;
    }
    return landingView;
  }

  const VIEW_TITLES = {
    "view-overview": "Overview",
    "scan-center": "Scan Center",
    "scan-view": "Scan Center",
    "landing-view": "Scan Center",
    "results-view": "Scan Results",
    "view-audit": "Startup Audit",
    "view-processes": "Process Sentinel",
    "view-quarantine": "Quarantine",
    "cleanup-view": "Disk Cleanup",
    "view-eventlog": "Event Log",
    "view-canary": "Canary Guard",
    "view-incident": "Incident Investigation",
  };

  function setNav(viewId) {
    const items = document.querySelectorAll(".nav-item");
    items.forEach((item) => {
      const target = item.getAttribute("data-view");
      const active =
        target === viewId ||
        (target === "scan-center" && (viewId === "scan-view" || viewId === "landing-view"));
      item.setAttribute("aria-current", active ? "true" : "false");
    });
    const title = document.getElementById("view-title");
    if (title) title.textContent = VIEW_TITLES[viewId] || "C.U.R.E";
  }

  function showViewInstant(el) {
    const views = document.querySelectorAll(".stage > .view");
    views.forEach((v) => {
      if (v === el) v.classList.remove("hidden", "exiting", "pre-enter");
      else v.classList.add("hidden");
    });
    if (el && el.id) setNav(el.id);
    focusViewHeading(el);
  }

  // SPA focus management: view changes move focus to the view heading so
  // screen-reader users land on the new content. Visual focus ring stays
  // on :focus-visible only; mouse users see no change.
  function focusViewHeading(viewEl) {
    if (!viewEl || !viewEl.querySelector) return;
    const h = viewEl.querySelector(".headline");
    if (!h) return;
    if (!h.hasAttribute("tabindex")) h.setAttribute("tabindex", "-1");
    try { h.focus({ preventScroll: true }); } catch (_) { h.focus(); }
  }

  // Sidebar navigation. Cleanup entry/exit reuse the existing open/close
  // paths so pill save/restore and the armed-button state stay consistent.
  async function navTo(viewId) {
    if (viewId === "scan-center") {
      if (cleanupState.open) closeCleanup(scanPhase === "running" ? scanView : landingView);
      else showViewInstant(scanPhase === "running" ? scanView : landingView);
      return;
    }
    if (viewId === "cleanup-view") {
      await openCleanup();
      showViewInstant(cleanupView);
      return;
    }
    const el = document.getElementById(viewId);
    if (!el) return;
    if (cleanupState.open) closeCleanup(el);
    else showViewInstant(el);
    if (viewId === "view-quarantine") refreshQuarantine();
    if (viewId === "view-eventlog") renderEventLog();
    if (viewId === "view-canary") { renderCanaryView(); syncCanaryStatus(); }
  }

  function enablePostScanNav() {
    ["nav-results", "nav-audit", "nav-processes"].forEach((id) => {
      const btn = document.getElementById(id);
      if (btn) btn.disabled = false;
    });
  }

  async function runScan(preLines = []) {
    const token = ++scanToken;
    scanPhase = "running";
    scanStartedAt = performance.now();
    resultsView.classList.add("hidden");
    netmap.hide();
    scanView.classList.remove("hidden", "exiting", "pre-enter");
    setNav("scan-view");
    logList.innerHTML = "";
    itemFeedCount = 0;
    hideReceipt();
    if (feedCountEl) feedCountEl.textContent = "0 ITEMS";
    const elapsedEl = document.getElementById("scan-elapsed");
    if (elapsedEl) elapsedEl.textContent = "elapsed 0.0s";
    const elapsedTimer = setInterval(() => {
      if (token !== scanToken) { clearInterval(elapsedTimer); return; }
      if (elapsedEl) elapsedEl.textContent = "elapsed " + ((performance.now() - scanStartedAt) / 1000).toFixed(1) + "s";
    }, 500);
    for (const line of preLines) {
      appendLog("overlay", line);
    }
    setPill("scanning", "sweeping persistence locations…");
    radar.start();
    try {
      const summary = await invoke("run_auto_scan");
      if (token !== scanToken) return;
      scanDurationMs = Math.round(performance.now() - scanStartedAt);
      clearInterval(elapsedTimer);
      if (elapsedEl) elapsedEl.textContent = "finished in " + fmtDuration(scanDurationMs) + " · " + summary.total + " entries checked";
      lastSummary = summary;
      lastScanAt = new Date();
      scanPhase = "done";
      renderSessionMeta(summary);
      logEvent("info", "scan finished: " + summary.total + " checks in " + fmtDuration(scanDurationMs));
      appendLog("done", summary.total + " entries processed — scan finished");
      radar.stop();
      await switchView(currentView(), resultsView);
      setNav("results-view");
      if (token !== scanToken) return;
      renderResults(summary);
    } catch (err) {
      clearInterval(elapsedTimer);
      radar.stop();
      scanPhase = "idle";
      setPill("error", String(err));
      appendLog("error", String(err));
      logEvent("info", "scan failed: " + String(err));
    }
  }

  listen("scan-progress", (event) => {
    const payload = event.payload;
    if (payload.stage === "item-scanned") {
      appendItemLine(payload);
      radar.addNode(payload.risk, payload.name);
      return;
    }
    if (payload.stage === "process-flagged") {
      appendProcessLine(payload);
      radar.addNode(payload.risk, payload.name);
      logEvent("info", "process flagged: " + payload.name + " (pid " + payload.pid + ", " + payload.risk + " " + payload.score + ")");
      return;
    }
    if (payload.stage === "ransom-found") {
      appendRansomLine(payload);
      logEvent("canary", "ransom indicator: " + (payload.finding_type || "unknown") + " — " + (payload.detail || payload.path || ""));
      return;
    }
    setPill("scanning", payload.message);
    if (payload.stage === "done") {
      logEvent("info", payload.message);
      appendLog(payload.stage, payload.message);
      return;
    }
    logEvent("info", "[" + payload.stage + "] " + payload.message);
    appendLog(payload.stage, payload.message);
  });

  document.getElementById("rescan-btn").addEventListener("click", runScan);
  document.getElementById("action-receipt-x").addEventListener("click", hideReceipt);

  const footMsg = document.getElementById("footbar-msg");
  let footTimer = null;
  function footFeedback(text, isError) {
    footMsg.textContent = text;
    footMsg.classList.toggle("error", !!isError);
    footMsg.classList.add("show");
    clearTimeout(footTimer);
    // Durable enough to read; the same outcome is also kept in the Event
    // Log and the relevant view (receipt/status line), never toast-only.
    footTimer = setTimeout(() => footMsg.classList.remove("show"), 6000);
  }

  function cleanErrText(err, fallback) {
    const s = String(err == null ? "" : err).replace(/^Error:\s*/i, "").trim();
    return s || fallback;
  }

  // ---- shared destructive-action confirm dialog ----
  // requestConfirm({kicker,title,facts:[[label,value]...],note,okLabel})
  // resolves true only on explicit Confirm. Escape, backdrop click, and
  // Cancel resolve false. Focus starts on Cancel, stays inside while open,
  // and returns to the invoking control afterwards.
  let confirmOpen = false;
  function requestConfirm(opts) {
    if (confirmOpen) return Promise.resolve(false);
    const overlay = document.getElementById("confirm-overlay");
    const kicker = document.getElementById("confirm-kicker");
    const title = document.getElementById("confirm-title");
    const facts = document.getElementById("confirm-facts");
    const note = document.getElementById("confirm-note");
    const cancelBtn = document.getElementById("confirm-cancel");
    const okBtn = document.getElementById("confirm-ok");
    if (!overlay || !cancelBtn || !okBtn) return Promise.resolve(false);
    const invoker = document.activeElement;
    kicker.textContent = opts.kicker || "Confirm action";
    title.textContent = opts.title || "Are you sure?";
    facts.innerHTML = "";
    for (const [label, value] of opts.facts || []) {
      const dt = document.createElement("dt");
      dt.textContent = label;
      const dd = document.createElement("dd");
      dd.textContent = value;
      facts.append(dt, dd);
    }
    note.textContent = opts.note || "";
    note.classList.toggle("hidden", !opts.note);
    okBtn.textContent = opts.okLabel || "Confirm";
    confirmOpen = true;
    overlay.classList.remove("hidden");
    return new Promise((resolve) => {
      const done = (value) => {
        confirmOpen = false;
        overlay.classList.add("hidden");
        overlay.removeEventListener("mousedown", onBackdrop);
        document.removeEventListener("keydown", onKey, true);
        if (invoker && invoker.focus) invoker.focus();
        resolve(value);
      };
      const onBackdrop = (ev) => {
        if (ev.target === overlay) done(false);
      };
      const onKey = (ev) => {
        if (ev.key === "Escape") {
          ev.stopPropagation();
          done(false);
        } else if (ev.key === "Tab") {
          // Minimal two-button trap: keep Tab cycling inside the dialog.
          ev.preventDefault();
          (document.activeElement === cancelBtn ? okBtn : cancelBtn).focus();
        }
      };
      overlay.addEventListener("mousedown", onBackdrop);
      document.addEventListener("keydown", onKey, true);
      cancelBtn.onclick = () => done(false);
      okBtn.onclick = () => done(true);
      cancelBtn.focus();
    });
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
    const second = document.getElementById("can-toggle");
    if (second) second.textContent = canaryActive ? "Disable guard" : "Enable guard";
    const badge = document.getElementById("can-state");
    if (badge) {
      if (canaryTriggered) {
        badge.textContent = "TRIGGERED";
        badge.className = "severity-badge sev-bad";
      } else if (canaryActive) {
        badge.textContent = "ACTIVE";
        badge.className = "severity-badge sev-info";
      } else {
        badge.textContent = "OFF";
        badge.className = "severity-badge";
      }
    }
  }

  async function toggleCanary() {
    try {
      if (canaryActive) {
        await invoke("stop_canary_guard");
        canaryActive = false;
        footFeedback("Canary guard deactivated", false);
        logEvent("action", "canary guard deactivated");
      } else {
        await invoke("start_canary_guard");
        canaryActive = true;
        footFeedback("Canary guard active — decoys planted", false);
        logEvent("action", "canary guard activated — decoys planted");
      }
    } catch (err) {
      footFeedback(cleanErrText(err, "Could not toggle canary guard"), true);
    }
    updateCanaryUI();
  }

  if (canaryToggle) {
    canaryToggle.addEventListener("click", toggleCanary);
  }

  // Listen for canary-alert events from backend
  const canaryOverlay = document.getElementById("canary-alert-overlay");
  const canaryDetail = document.getElementById("canary-alert-detail");
  const canaryDismissBtn = document.getElementById("canary-dismiss-btn");

  // Canary alert dialog: modal while visible. Escape dismisses, focus
  // moves to Dismiss on open and returns to the invoker on close, and
  // Tab is kept on the dialog's single action while open.
  let canaryInvoker = null;
  function canaryKeyTrap(ev) {
    if (ev.key === "Escape") {
      ev.stopPropagation();
      hideCanaryAlert();
    } else if (ev.key === "Tab") {
      ev.preventDefault();
      if (canaryDismissBtn) canaryDismissBtn.focus();
    }
  }
  function showCanaryAlert() {
    if (!canaryOverlay || !canaryOverlay.classList.contains("hidden")) return;
    canaryInvoker = document.activeElement;
    canaryOverlay.classList.remove("hidden");
    document.addEventListener("keydown", canaryKeyTrap, true);
    if (canaryDismissBtn) canaryDismissBtn.focus();
  }
  function hideCanaryAlert() {
    if (!canaryOverlay) return;
    canaryOverlay.classList.add("hidden");
    document.removeEventListener("keydown", canaryKeyTrap, true);
    if (canaryInvoker && canaryInvoker.focus) canaryInvoker.focus();
    canaryInvoker = null;
  }

  if (canaryDismissBtn) {
    canaryDismissBtn.addEventListener("click", hideCanaryAlert);
  }

  TAU.event.listen("canary-alert", (ev) => {
    const payload = typeof ev.payload === "string" ? JSON.parse(ev.payload) : ev.payload;
    const kind = payload.kind || "unknown";
    const folder = payload.folder || "";
    const file = payload.file || "";
    const action = payload.action || "";
    const text =
      kind.replace(/-/g, " ").toUpperCase() + " — " +
      (folder ? folder + " " : "") + file +
      (action ? " (" + action + ")" : "");
    canaryTriggered = true;
    canarySessionAlerts.push({ at: new Date(), text });
    if (canarySessionAlerts.length > 100) canarySessionAlerts.shift();
    logEvent("canary", "canary alert: " + text);
    appendCanaryAlertRow({ at: new Date(), text });
    updateCanaryUI();
    if (canaryDetail) {
      canaryDetail.textContent = text;
    }
    showCanaryAlert();
  });

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
    btnLabel: document.getElementById("cleanup-btn-label"),
    status: document.getElementById("cleanup-status"),
    failures: document.getElementById("cleanup-failures"),
    stage: document.getElementById("toss-stage"),
    liveCounter: document.getElementById("cleanup-live-counter"),
    progWrap: document.getElementById("cleanup-progress-wrap"),
    progBar: document.getElementById("cleanup-progress-bar"),
    progPct: document.getElementById("cleanup-progress-pct"),
    panel: document.getElementById("cleanup-panel"),
  };
  function setCleanupBtnLabel(t) {
    if (cleanupEls.btnLabel) cleanupEls.btnLabel.textContent = t;
    else cleanupEls.btn.textContent = t;
  }
  const CLEANUP_ICONS = {
    temp: "🧹",
    browser_cache: "🌐",
    recycle_bin: "♻️",
    windows_old: "🗂️",
  };
  function cleanupConfettiBurst() {
    if (REDUCED || !cleanupEls.panel) return;
    const host = cleanupEls.panel;
    const prev = host.style.position;
    if (!prev || prev === "static") host.style.position = "relative";
    const colors = ["#7c6cf0", "#9be7f4", "#43b581", "#e8c476", "#e9ebf4"];
    const r = host.getBoundingClientRect();
    for (let i = 0; i < 22; i++) {
      const s = document.createElement("span");
      s.className = "cleanup-confetti";
      s.style.left = (12 + Math.random() * 76) + "%";
      s.style.top = "18%";
      s.style.background = colors[i % colors.length];
      s.style.animationDelay = (Math.random() * 0.25) + "s";
      host.appendChild(s);
      setTimeout(() => s.remove(), 1600);
    }
  }

  const cleanupState = {
    summary: null,
    selectedCats: new Set(),
    checkedDownloads: new Set(),
    running: false,
    open: false,
    savedPill: null,
  };

  const sweepState = {
    findings: [],
    checked: new Set(),
    running: false,
  };

  function updateKillButton() {
    const btn = document.getElementById("kill-procs-btn");
    if (!btn) return;
    const enabled = sweepState.checked.size > 0 && !sweepState.running;
    btn.disabled = !enabled;
    btn.textContent = "Kill selected (" + sweepState.checked.size + ")";
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
    if (cleanupEls.progBar) {
      const pct = Math.round(ramp * 92 + (cycleT * 8));
      const clamped = Math.min(99, pct);
      cleanupEls.progBar.style.width = clamped + "%";
      if (cleanupEls.progPct) cleanupEls.progPct.textContent = clamped + "%";
    }

    toss.raf = requestAnimationFrame(tossFrame);
  }

  function startToss(expectedBytes) {
    window.__cureTossSeen = true;
    tossInit();
    cleanupEls.stage.classList.remove("hidden");
    cleanupEls.stage.classList.add("toss-active");
    if (cleanupEls.progWrap) cleanupEls.progWrap.classList.remove("hidden");
    if (cleanupEls.progBar) cleanupEls.progBar.style.width = "4%";
    if (cleanupEls.progPct) cleanupEls.progPct.textContent = "4%";
    if (cleanupEls.panel) cleanupEls.panel.classList.remove("cleanup-success");
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
    if (cleanupEls.progBar) cleanupEls.progBar.style.width = "100%";
    if (cleanupEls.progPct) cleanupEls.progPct.textContent = "100%";
    if (cleanupEls.stage) cleanupEls.stage.classList.remove("toss-active");
    setTimeout(() => { if (cleanupEls.progWrap) cleanupEls.progWrap.classList.add("hidden"); }, 900);
    if (REDUCED || !toss.active) {
      if (REDUCED) tossSetOrb(ORB_REST[0], ORB_REST[1], 1, 1);
      if (cleanupEls.progWrap) cleanupEls.progWrap.classList.add("hidden");
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
    if (cleanupEls.stage) cleanupEls.stage.classList.remove("toss-active");
    if (cleanupEls.progWrap) cleanupEls.progWrap.classList.add("hidden");
    if (cleanupEls.progBar) cleanupEls.progBar.style.width = "0%";
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

  // Clear a stale cleanup result when the selection changes or the
  // view closes. Destructive confirmation now goes through requestConfirm.
  function resetCleanupResult() {
    cleanupEls.status.textContent = "";
    cleanupEls.status.classList.add("hidden");
    cleanupEls.status.classList.remove("cleanup-ok", "cleanup-fail");
    if (cleanupPhase === "done-ok" || cleanupPhase === "done-fail") {
      cleanupPhase = "ready";
      setCleanupStep(1);
    }
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
    if (!cleanupState.running) {
      cleanupEls.btn.classList.remove("btn-active");
    }
    if (!cleanupState.running && s) {
      const selB = cleanupSelectionBytes();
      setCleanupBtnLabel(selB > 0 ? "Clean up • " + fmtBytes(selB) : "Clean up");
    } else if (!cleanupState.running) {
      setCleanupBtnLabel("Clean up");
    }
    renderCleanupSummary();
  }

  let cleanupPhase = "idle"; // idle | ready | running | done-ok | done-fail

  // 3-step pipeline strip (Candidates → Confirmation → Cleanup).
  function setCleanupStep(n, allDone) {
    document.querySelectorAll("#cleanup-steps .step").forEach((li) => {
      const k = Number(li.dataset.step);
      li.classList.toggle("done", allDone ? true : k < n);
      li.classList.toggle("current", !allDone && k === n);
    });
  }

  // Live summary panel — counts come straight from the scan summary and
  // the current selection. Never invented.
  function renderCleanupSummary() {
    const probe = document.getElementById("cs-total-items");
    if (!probe) return;
    const set = (id, v) => { const el = document.getElementById(id); if (el) el.textContent = v; };
    const s = cleanupState.summary;
    if (!s) {
      set("cs-total-items", "—"); set("cs-total-size", "—"); set("cs-selected", "—");
      const st0 = document.getElementById("cs-status");
      if (st0) { st0.textContent = "Idle"; st0.classList.remove("on", "bad"); }
      return;
    }
    const totalN = s.categories.reduce((n, c) => n + c.item_count, 0) + s.downloads.length;
    let selN = 0, selB = 0;
    for (const c of s.categories) {
      if (c.item_count > 0 && cleanupState.selectedCats.has(c.key)) { selN += c.item_count; selB += c.total_bytes; }
    }
    selN += cleanupState.checkedDownloads.size;
    for (const dl of s.downloads) {
      if (cleanupState.checkedDownloads.has(dl.path)) selB += dl.size_bytes;
    }
    set("cs-total-items", String(totalN));
    set("cs-total-size", fmtBytes(s.total_bytes));
    set("cs-selected", selN + " items · " + fmtBytes(selB));
    const labels = { idle: "Idle", ready: "Ready", running: "Cleaning…", "done-ok": "Complete", "done-fail": "Attention" };
    const st = document.getElementById("cs-status");
    if (st) {
      st.textContent = labels[cleanupPhase] || "—";
      st.classList.toggle("on", cleanupPhase !== "idle");
      st.classList.toggle("bad", cleanupPhase === "done-fail");
    }
  }

  function renderCleanup(summary, keepResult = false) {
    cleanupState.summary = summary;
    cleanupState.selectedCats = new Set();
    cleanupState.checkedDownloads = new Set();
    cleanupState.running = false;
    if (!keepResult) {
      cleanupPhase = "ready";
      setCleanupStep(1);
    }

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
      '<span class="total-orb" aria-hidden="true"></span><span>≈ <b>' + fmtBytes(summary.total_bytes) + "</b> reclaimable across " +
      (itemCount + summary.downloads.length) + " items</span>";
    cleanupEls.subline.textContent =
      itemCount + summary.downloads.length + " cleanable items found on this machine";

    cleanupEls.grid.innerHTML = "";
    const maxCatBytes = Math.max(1, ...summary.categories.map((c) => c.total_bytes));
    let catIdx = 0;
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
      if (!REDUCED) card.style.animationDelay = (catIdx * 70) + "ms";
      catIdx++;
      card.setAttribute("aria-pressed", String(on));
      card.setAttribute("aria-label", cat.label + " — " + fmtBytes(cat.total_bytes) + ", " + cat.item_count + " items");
      card.title = on
        ? "Click to skip this category"
        : cat.item_count === 0
          ? "Nothing found in this category"
          : "Currently skipped — click to include";
      const top = document.createElement("span");
      top.className = "cc-top";
      const icon = document.createElement("span");
      icon.className = "cc-icon";
      icon.setAttribute("aria-hidden", "true");
      icon.textContent = CLEANUP_ICONS[cat.key] || "📦";
      const name = document.createElement("span");
      name.className = "cc-name";
      name.textContent = cat.label;
      top.append(icon, name);
      const size = document.createElement("span");
      size.className = "cc-size";
      size.textContent = fmtBytes(cat.total_bytes);
      const count = document.createElement("span");
      count.className = "cc-count";
      count.textContent =
        cat.item_count + (cat.item_count === 1 ? " item" : " items");
      const bar = document.createElement("span");
      bar.className = "cc-bar";
      bar.setAttribute("aria-hidden", "true");
      const fill = document.createElement("i");
      fill.style.width = Math.max(cat.item_count === 0 ? 0 : 6, Math.round((cat.total_bytes / maxCatBytes) * 100)) + "%";
      bar.appendChild(fill);
      card.append(top, size, count, bar);
      card.addEventListener("click", () => {
        if (card.disabled) return;
        const nowOn = !cleanupState.selectedCats.has(cat.key);
        if (nowOn) cleanupState.selectedCats.add(cat.key);
        else cleanupState.selectedCats.delete(cat.key);
        card.classList.toggle("on", nowOn);
        card.classList.toggle("off", !nowOn);
        card.setAttribute("aria-pressed", String(nowOn));
        card.title = nowOn ? "Click to skip this category" : "Currently skipped — click to include";
        resetCleanupResult();
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
      box.setAttribute("aria-label", "Delete " + dl.name + " (" + fmtBytes(dl.size_bytes) + ")");
      box.addEventListener("change", () => {
        if (box.checked) cleanupState.checkedDownloads.add(dl.path);
        else cleanupState.checkedDownloads.delete(dl.path);
        resetCleanupResult();
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
    // premium skeleton shimmer while measuring
    if (cleanupEls.panel) cleanupEls.panel.classList.add("cleanup-scanning");
    let skel = cleanupEls.panel ? cleanupEls.panel.querySelector(".cleanup-skeleton") : null;
    if (!skel && cleanupEls.panel && !REDUCED) {
      skel = document.createElement("div");
      skel.className = "cleanup-skeleton";
      skel.setAttribute("aria-hidden", "true");
      skel.innerHTML = "<i></i><i></i><i></i><i></i>";
      cleanupEls.loading.after(skel);
    }
    if (skel) skel.classList.remove("hidden");
    if (!keepResult) {
      setCleanupPill("scanning", "measuring reclaimable space…");
    }
    try {
      const summary = await invoke("scan_cleanup");
      if (skel) skel.remove();
      if (cleanupEls.panel) cleanupEls.panel.classList.remove("cleanup-scanning");
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
    cleanupPhase = "idle";
    setCleanupStep(1);
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
    await switchView(currentView(), cleanupView);
    setNav("cleanup-view");
  }

  function closeCleanup(backTo) {
    if (!cleanupState.open) return;
    cleanupState.open = false;
    resetCleanupResult();
    resetToss();
    cleanupEls.stage.classList.add("hidden");
    if (cleanupState.savedPill) {
      statusPill.className = cleanupState.savedPill.cls;
      statusText.textContent = cleanupState.savedPill.text;
    }
    const target = backTo && backTo.classList ? backTo : resultsView;
    switchView(cleanupView, target);
    if (target.id) setNav(target.id);
  }

  cleanupEls.openBtn.addEventListener("click", openCleanup);
  cleanupEls.backBtn.addEventListener("click", () => closeCleanup(resultsView));
  cleanupEls.scanBtn.addEventListener("click", () => startCleanupScan(false));

  cleanupEls.btn.addEventListener("click", async () => {
    if (cleanupState.running || cleanupEls.btn.disabled) return;
    const selBytes = cleanupSelectionBytes();
    const selCats = (cleanupState.summary.categories || [])
      .filter((c) => c.item_count > 0 && cleanupState.selectedCats.has(c.key))
      .map((c) => c.label || c.key);
    if (cleanupState.checkedDownloads.size > 0) {
      selCats.push(cleanupState.checkedDownloads.size + " selected download(s)");
    }
    setCleanupStep(2);
    const ok = await requestConfirm({
      kicker: "Confirm disk cleanup",
      title: "Permanently delete selected files?",
      facts: [
        ["Selection", selCats.join(", ") || "Nothing selected"],
        ["Amount", fmtBytes(selBytes)],
        ["Action", "Permanently delete the selected files to free disk space."],
        ["Reversible", "No — deleted files are not recoverable."],
      ],
      note: "Only the selected categories and checked downloads are deleted. Quarantined items are never touched.",
      okLabel: "Delete " + fmtBytes(selBytes),
    });
    if (!ok) { setCleanupStep(1); return; }
    cleanupPhase = "running";
    setCleanupStep(3);
    renderCleanupSummary();
    cleanupState.running = true;
    cleanupEls.btn.disabled = true;
    cleanupEls.btn.classList.add("btn-active");
    setCleanupBtnLabel("Cleaning…");
    const expected = cleanupSelectionBytes();
    startToss(expected);
    try {
      const result = await invoke("run_cleanup", {
        categories: Array.from(cleanupState.selectedCats),
        downloadPaths: Array.from(cleanupState.checkedDownloads),
      });
      stopToss(result.bytes_freed);
      cleanupPhase = result.failed ? "done-fail" : "done-ok";
      setCleanupStep(3, true);
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
      cleanupEls.status.classList.toggle("cleanup-ok", !result.failed);
      cleanupEls.status.classList.toggle("cleanup-fail", !!result.failed);
      if (!result.failed && cleanupEls.panel) {
        cleanupEls.panel.classList.add("cleanup-success");
        cleanupConfettiBurst();
        setTimeout(() => { if (cleanupEls.panel) cleanupEls.panel.classList.remove("cleanup-success"); }, 1400);
      }
      logEvent("action", "cleanup: freed " + fmtBytes(result.bytes_freed) + ", deleted " + result.deleted + " of " + result.attempted + (result.failed ? ", " + result.failed + " failed" : ""));
      if (result.failures.length > 0) {
        cleanupEls.failures.innerHTML = "";
        for (const failure of result.failures) {
          const li = document.createElement("li");
          li.className = "cf-card";
          const badge = document.createElement("span");
          badge.className = "cf-badge";
          badge.textContent = "locked · skipped";
          const p = document.createElement("span");
          p.className = "cf-path selectable";
          p.textContent = failure.path;
          p.title = failure.path;
          const r = document.createElement("span");
          r.className = "cf-reason selectable";
          r.textContent = failure.reason;
          li.append(badge, p, r);
          cleanupEls.failures.appendChild(li);
        }
        cleanupEls.failures.classList.remove("hidden");
      }
    } catch (err) {
      stopToss(0);
      cleanupPhase = "done-fail";
      setCleanupStep(3, true);
      setCleanupPill("error", "Disk cleanup failed");
      cleanupEls.status.textContent =
        "cleanup failed: " + cleanErrText(err, String(err));
      cleanupEls.status.classList.remove("hidden");
      cleanupEls.status.classList.remove("cleanup-ok");
      cleanupEls.status.classList.add("cleanup-fail");
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
      var targets = [];
      sweepState.findings.forEach(function(f) {
        if (sweepState.checked.has(f.pid)) targets.push([f.name, f.pid]);
      });
      if (targets.length === 0) return;
      const ok = await requestConfirm({
        kicker: "Confirm process termination",
        title: "Terminate " + targets.length + " selected process(es)?",
        facts: [
          ["Targets", targets.map(function(t) { return t[0] + " (pid " + t[1] + ")"; }).join(", ")],
          ["Action", "Terminate the selected processes immediately."],
          ["Reversible", "No — termination cannot be undone, but nothing is deleted."],
        ],
        note: "PIDs are re-validated before termination. Re-run a scan afterwards to verify.",
        okLabel: "Terminate " + targets.length + " process(es)",
      });
      if (!ok) return;
      sweepState.running = true;
      killBtn.disabled = true;
      killBtn.classList.add("btn-active");
      killBtn.textContent = "Killing…";
      if (killStatus) { killStatus.textContent = ""; killStatus.classList.add("hidden"); }
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
        logEvent("action", "process kill: " + msg + (failed.length ? " — " + failed.join("; ") : ""));
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

  // ══════════════ UI V2 views (all data from real backend responses) ══════

  function fmtDuration(ms) {
    if (ms == null) return "—";
    const s = ms / 1000;
    if (s < 60) return s.toFixed(1) + "s";
    const m = Math.floor(s / 60);
    return m + "m " + Math.round(s % 60) + "s";
  }

  // ---- event log view ----

  function appendEventLogRow(e) {
    const list = document.getElementById("event-log-list");
    if (!list) return;
    const li = document.createElement("li");
    if (e.kind === "canary") li.className = "ev-canary";
    else if (e.kind === "action") li.className = "ev-action";
    const time = document.createElement("span");
    time.className = "ev-time";
    time.textContent = e.at.toLocaleTimeString();
    const tag = document.createElement("b");
    tag.textContent = "[" + e.kind + "]";
    const body = document.createElement("span");
    body.className = "selectable";
    body.textContent = e.text;
    li.append(time, tag, body);
    list.appendChild(li);
    while (list.children.length > 300) list.removeChild(list.firstChild);
    list.scrollTop = list.scrollHeight;
    const empty = document.getElementById("event-log-empty");
    if (empty) empty.classList.add("hidden");
  }

  function renderEventLog() {
    const list = document.getElementById("event-log-list");
    if (!list) return;
    list.innerHTML = "";
    for (const e of eventLog) {
      const li = document.createElement("li");
      if (e.kind === "canary") li.className = "ev-canary";
      else if (e.kind === "action") li.className = "ev-action";
      const time = document.createElement("span");
      time.className = "ev-time";
      time.textContent = e.at.toLocaleTimeString();
      const tag = document.createElement("b");
      tag.textContent = "[" + e.kind + "]";
      const body = document.createElement("span");
      body.className = "selectable";
      body.textContent = e.text;
      li.append(time, tag, body);
      list.appendChild(li);
    }
    const empty = document.getElementById("event-log-empty");
    if (empty) empty.classList.toggle("hidden", eventLog.length > 0);
  }

  // ---- canary view ----

  function appendCanaryAlertRow(a) {
    const list = document.getElementById("can-alerts");
    if (!list) return;
    const li = document.createElement("li");
    li.className = "ev-canary";
    const time = document.createElement("span");
    time.className = "ev-time";
    time.textContent = a.at.toLocaleTimeString();
    const tag = document.createElement("b");
    tag.textContent = "[alert]";
    const body = document.createElement("span");
    body.textContent = a.text;
    li.append(time, tag, body);
    list.appendChild(li);
    const empty = document.getElementById("can-empty");
    if (empty) empty.classList.add("hidden");
  }

  function renderCanaryView() {
    const list = document.getElementById("can-alerts");
    if (!list) return;
    list.innerHTML = "";
    for (const a of canarySessionAlerts) appendCanaryAlertRow(a);
    const empty = document.getElementById("can-empty");
    if (empty) empty.classList.toggle("hidden", canarySessionAlerts.length > 0);
    updateCanaryUI();
  }

  // ---- overview ----

  function troubleCounts(s) {
    const cleaned = s.high_risk_cleaned.length;
    const review = s.suspicious_for_review || [];
    const procs = s.process_findings || [];
    const ransom = s.ransom_findings || [];
    const reviewHigh = review.filter((e) => e.risk === "HighRisk").length;
    const reviewSusp = review.filter((e) => e.risk === "Suspicious").length;
    const procHigh = procs.filter((p) => p.risk === "HighRisk").length;
    const procSusp = procs.filter((p) => p.risk === "Suspicious").length;
    return {
      cleaned, review: review.length, proc: procs.length, ransom: ransom.length,
      safe: s.safe, total: s.total,
      critical: reviewHigh + procHigh + ransom.length,
      suspicious: reviewSusp + procSusp,
      findings: cleaned + review.length + procs.length + ransom.length,
    };
  }

  function setMetric(id, text, tone) {
    const el = document.getElementById(id);
    if (!el) return;
    el.textContent = text;
    el.classList.remove("is-ok", "is-warn", "is-bad");
    if (tone) el.classList.add(tone);
  }

  function covRow(name, detail, level) {
    const li = document.createElement("li");
    const dot = document.createElement("span");
    dot.className = "cov-dot " + (level || "idle");
    const nm = document.createElement("span");
    nm.className = "cov-name";
    nm.textContent = name;
    const det = document.createElement("span");
    det.className = "cov-detail";
    det.textContent = detail;
    li.append(dot, nm, det);
    return li;
  }

  function updateOvIncident() {
    const el = document.getElementById("ov-incident");
    if (!el) return;
    try {
      if (typeof lastIncident !== "undefined" && lastIncident) {
        el.textContent = "Last observation " + (lastIncident.investigation_id || "") + ": " +
          ((typeof VERDICT_TEXT !== "undefined" && VERDICT_TEXT[lastIncident.verdict]) || lastIncident.verdict || "") +
          " — " + (lastIncident.processes || []).length + " processes, " +
          (lastIncident.windows || []).length + " windows, " +
          (lastIncident.correlations || []).length + " correlations. See the Incident view for the full timeline.";
        return;
      }
    } catch (_) { /* TDZ-safe: fall through to default */ }
    el.textContent = "No observation recorded — run a login investigation from the Incident view.";
  }

  function renderOverview(summary) {
    const t = troubleCounts(summary);
    const posture = document.getElementById("ov-posture");
    const subline = document.getElementById("ov-subline");
    if (t.critical > 0) {
      posture.textContent = "Investigation required";
      posture.classList.add("is-bad"); posture.classList.remove("is-warn", "is-ok");
      subline.textContent = t.critical + " critical finding(s) need a decision — see Startup Audit and Process Sentinel.";
    } else if (t.findings > 0) {
      posture.textContent = "Review required";
      posture.classList.add("is-warn"); posture.classList.remove("is-bad", "is-ok");
      subline.textContent = t.findings + " finding(s) recorded" +
        (t.cleaned ? ", " + t.cleaned + " auto-quarantined" : "") + " — nothing was deleted.";
    } else {
      posture.textContent = "No findings";
      posture.classList.add("is-ok"); posture.classList.remove("is-bad", "is-warn");
      subline.textContent = summary.total + " checks completed — no persistence, process, or ransom findings" +
        (lastScanAt ? " · last scan " + lastScanAt.toLocaleString() : "");
    }
    setMetric("ov-last-scan", lastScanAt ? lastScanAt.toLocaleString() : "—", null);
    setMetric("ov-checks", String(summary.total), null);
    const ovStates = (summary && summary.source_states) || [];
    let skipped = 0;
    let covState = "FULL";
    let covTone = "is-ok";
    for (const row of ovStates) {
      const st = row.state;
      if (st === "CheckFailed" || st === "AccessDenied") { covState = "FAILED"; covTone = "is-bad"; }
      else if (st && typeof st === "object") {
        if ("Partial" in st) { skipped += (st.Partial && st.Partial.skipped) || 0; if (covState !== "FAILED") { covState = "PARTIAL"; covTone = "is-warn"; } }
        else if ("CheckFailed" in st || "AccessDenied" in st) { covState = "FAILED"; covTone = "is-bad"; }
        else if (("Unavailable" in st || "NotChecked" in st) && covState === "FULL") { covState = "LIMITED"; covTone = null; }
      }
      else if ((st === "Unavailable" || st === "NotChecked") && covState === "FULL") { covState = "LIMITED"; covTone = null; }
    }
    setMetric("ov-skipped", String(skipped), skipped ? "is-warn" : null);
    setMetric("ov-coverage-state", ovStates.length ? covState : "—", ovStates.length ? covTone : null);
    setMetric("ov-findings", String(t.findings), t.findings ? "is-warn" : "is-ok");
    setMetric("ov-critical", String(t.critical), t.critical ? "is-bad" : "is-ok");
    setMetric("ov-suspicious", String(t.suspicious), t.suspicious ? "is-warn" : "is-ok");
    setMetric("ov-duration", fmtDuration(scanDurationMs), null);
    const ovScope = document.getElementById("ov-scope");
    if (ovScope) {
      const areas = ovStates.length ? ovStates.map((r) => r.area).join(" · ") : "Startup entries · Services · Tasks · WMI";
      ovScope.textContent = "Scope: " + areas + " · " + summary.total + " checked · " + skipped + " skipped" +
        (lastScanAt ? " · " + lastScanAt.toLocaleString() : "");
    }
    const ovActions = document.getElementById("ov-actions");
    const ovActionsEmpty = document.getElementById("ov-actions-empty");
    if (ovActions) {
      ovActions.innerHTML = "";
      const recent = eventLog.slice(-5).reverse();
      for (const ev of recent) {
        const li = document.createElement("li");
        const time = document.createElement("span");
        time.className = "ev-time";
        try { time.textContent = ev.at.toLocaleTimeString(); } catch (_) { time.textContent = ""; }
        const tag = document.createElement("b");
        tag.textContent = "[" + (ev.kind || "info") + "]";
        const body = document.createElement("span");
        body.className = "selectable";
        body.textContent = ev.text || "";
        li.append(time, tag, body);
        ovActions.appendChild(li);
      }
      if (ovActionsEmpty) ovActionsEmpty.classList.toggle("hidden", recent.length > 0);
    }
    updateOvIncident();

    const bySource = {};
    summary.high_risk_cleaned.concat(summary.suspicious_for_review).forEach((e) => {
      const src = e.entry.source;
      bySource[src] = bySource[src] || { n: 0, high: 0 };
      bySource[src].n += 1;
      if (e.risk === "HighRisk") bySource[src].high += 1;
    });
    const srcRow = (key, label) => {
      const info = bySource[key] || { n: 0, high: 0 };
      const detail = info.n ? info.n + " flagged" : "checked — nothing flagged";
      const level = info.high ? "bad" : info.n ? "warn" : "ok";
      return covRow(label, detail, level);
    };
    const cov = document.getElementById("ov-coverage");
    cov.innerHTML = "";
    // Prefer backend access states when present: a failed/skipped check
    // renders its state — never a bare "checked — nothing flagged" zero.
    const states = summary.source_states || [];
    const stateByArea = {};
    for (const row of states) stateByArea[row.area] = row;
    const stateLevel = (st) => {
      // Same CoverageState shapes as renderResultsCoverage (see above).
      if (st === "Available" || st === "Checked") return "ok";
      if (st === "Unavailable" || st === "NotChecked") return "idle";
      if (st === "CheckFailed" || st === "AccessDenied") return "bad";
      if (st && typeof st === "object") {
        if ("Partial" in st) return "warn";
        if ("CheckFailed" in st || "AccessDenied" in st) return "bad";
        if ("Unavailable" in st || "NotChecked" in st) return "idle";
        if ("Available" in st || "Checked" in st) return "ok";
      }
      return "idle";
    };
    const stateDetail = (row) => {
      let detail = row.detail || "";
      if (row.state && typeof row.state === "object" && "Partial" in row.state) {
        detail += " — " + row.state.Partial.skipped + " skipped";
      }
      return detail;
    };
    const stateCovRow = (area, fallbackLabel) => {
      const row = stateByArea[area];
      if (!row) return covRow(area, fallbackLabel, "idle");
      return covRow(area, stateDetail(row), stateLevel(row.state));
    };
    if (states.length) {
      cov.append(
        stateCovRow("Registry autoruns", "no data"),
        srcRow("StartupFolder", "Startup folder"),
        stateCovRow("Scheduled tasks", "no data"),
        stateCovRow("Services (auto-start)", "no data"),
        stateCovRow("WMI subscriptions", "no data"),
        srcRow("IfeoDebugger", "IFEO debuggers"),
        srcRow("AppInitDlls", "AppInit DLLs"),
        srcRow("ComHijack", "COM hijacks (HKCU)"),
        covRow("Running processes", t.proc ? t.proc + " flagged" : "checked — none flagged", t.proc ? "warn" : "ok"),
        covRow("Ransom indicators", t.ransom ? t.ransom + " found" : "none found", t.ransom ? "bad" : "ok"),
        covRow(
          "Canary guard",
          canaryTriggered ? "TRIGGERED — see Canary Guard" : canaryActive ? "active — watching decoys" : "off",
          canaryTriggered ? "bad" : canaryActive ? "ok" : "idle"
        )
      );
    } else {
      cov.append(
        srcRow("RegistryRun", "Registry autoruns (Run / RunOnce)"),
        srcRow("StartupFolder", "Startup folder"),
        srcRow("ScheduledTask", "Scheduled tasks"),
        srcRow("WindowsService", "Auto-start services"),
        srcRow("WmiSubscription", "WMI event subscriptions"),
        srcRow("IfeoDebugger", "IFEO debuggers"),
        srcRow("AppInitDlls", "AppInit DLLs"),
        srcRow("ComHijack", "COM hijacks (HKCU)"),
        covRow("Running processes", t.proc ? t.proc + " flagged" : "checked — none flagged", t.proc ? "warn" : "ok"),
        covRow("Ransom indicators", t.ransom ? t.ransom + " found" : "none found", t.ransom ? "bad" : "ok"),
        covRow(
          "Canary guard",
          canaryTriggered ? "TRIGGERED — see Canary Guard" : canaryActive ? "active — watching decoys" : "off",
          canaryTriggered ? "bad" : canaryActive ? "ok" : "idle"
        )
      );
    }
    const ovSub = document.getElementById("ov-subline");
    if (ovSub && typeof summary.elevated === "boolean") {
      ovSub.textContent += summary.elevated ? " · elevated scan" : " · standard-user scan";
    }
  }

  // ---- startup audit view ----

  function evidenceText(scored) {
    const e = scored.entry;
    return (
      "[CURE evidence] " + e.name + " | " + e.source + " | score " + scored.score + " (" + scored.risk + ")\n" +
      "command: " + e.command + "\n" +
      "location: " + e.location + "\n" +
      "reasons: " + (scored.reasons || []).join("; ")
    );
  }

  function copyEvidence(text, btn) {
    const done = () => {
      btn.textContent = "Copied ✓";
      setTimeout(() => { btn.textContent = "Copy evidence"; }, 1800);
    };
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(done, () => footFeedback("Copy failed", true));
    } else {
      const ta = document.createElement("textarea");
      ta.value = text;
      document.body.appendChild(ta);
      ta.select();
      try { document.execCommand("copy"); done(); }
      catch (_) { footFeedback("Copy failed", true); }
      ta.remove();
    }
  }

  function buildAuditCard(scored, cleaned) {
    const rawSource = String(scored.entry.source);
    const card = document.createElement("li");
    card.className = "review-card audit-card risk-" + scoreChipClass(scored.score);

    const top = document.createElement("div");
    top.className = "audit-top";
    const iconWrap = document.createElement("div");
    iconWrap.className = "src-icon " + (rawSource === "ScheduledTask" ? "icon-task" : rawSource === "RegistryRun" ? "icon-registry" : "icon-startup");
    iconWrap.innerHTML = SOURCE_ICONS[rawSource] || SOURCE_ICONS.StartupFolder;
    const main = document.createElement("div");
    main.className = "rc-main";
    const topRow = document.createElement("div");
    topRow.className = "rc-top";
    const name = document.createElement("span");
    name.className = "rc-name";
    name.textContent = scored.entry.name;
    name.title = scored.entry.command;
    const scoreEl = document.createElement("span");
    scoreEl.className = "score-chip " + scoreChipClass(scored.score);
    scoreEl.textContent = String(scored.score);
    scoreEl.title = scored.risk + " · risk score " + scored.score;
    topRow.append(name, scoreEl);
    main.appendChild(topRow);
    const chips = document.createElement("div");
    chips.className = "chips";
    const riskChip = document.createElement("span");
    riskChip.className = "chip " + (scored.risk === "HighRisk" ? "red" : scored.risk === "Suspicious" ? "amber" : "teal");
    riskChip.textContent = scored.risk === "HighRisk" ? "HIGH RISK" : scored.risk === "Suspicious" ? "SUSPICIOUS" : "SAFE";
    chips.appendChild(riskChip);
    const srcChip = document.createElement("span");
    srcChip.className = "chip src";
    srcChip.textContent = SOURCE_LABELS[rawSource] || "Persistence";
    chips.appendChild(srcChip);
    const atk = attackFor(scored);
    if (atk) {
      const atkChip = document.createElement("span");
      atkChip.className = "chip attack";
      atkChip.textContent = atk.id;
      atkChip.title = "MITRE ATT&CK: " + atk.name;
      chips.appendChild(atkChip);
    }
    const reasons = Array.isArray(scored.reasons) ? scored.reasons : [];
    for (const reason of reasons.slice(0, 3)) {
      const [label, tone] = reasonChipLabel(String(reason));
      const chip = document.createElement("span");
      chip.className = "chip" + (tone ? " " + tone : "");
      chip.textContent = label;
      chip.title = reason;
      chips.appendChild(chip);
    }
    main.appendChild(chips);
    top.append(iconWrap, main);

    const actions = document.createElement("div");
    actions.className = "audit-actions";
    const toggle = document.createElement("button");
    toggle.className = "detail-toggle";
    toggle.textContent = "View details";
    toggle.setAttribute("aria-expanded", "false");
    const copy = document.createElement("button");
    copy.className = "copy-btn";
    copy.textContent = "Copy evidence";
    copy.addEventListener("click", () => copyEvidence(evidenceText(scored), copy));
    actions.append(toggle, copy);
    // Open Location: strict id-based reveal (Explorer select, no shell).
    // Offered only when something file-backed exists to reveal.
    const reveal = document.createElement("button");
    reveal.className = "copy-btn";
    reveal.textContent = "Open location";
    reveal.title = "Select this file in Explorer (opens nothing)";
    reveal.addEventListener("click", async () => {
      try {
        await invoke("reveal_location", { id: scored.entry.id });
      } catch (err) {
        footFeedback(cleanErrText(err, "Nothing file-backed to reveal"), true);
      }
    });
    actions.append(reveal);
    if (cleaned) {
      const undo = document.createElement("button");
      undo.className = "quarantine-btn";
      undo.textContent = "Undo";
      undo.title = "Restore this file from quarantine";
      undo.addEventListener("click", async () => {
        undo.disabled = true;
        try {
          await invoke("undo_entry", { id: scored.entry.id });
          undo.textContent = "Restored ✓";
          undo.classList.add("row-done");
          sessionQuarantined = Math.max(0, sessionQuarantined - 1);
          refreshQuarantinedTile();
          showReceipt("Restored: " + scored.entry.name, "returned to " + (scored.entry.location || "its original location"), false);
          logEvent("action", "restored from quarantine: " + scored.entry.name);
        } catch (err) {
          undo.disabled = false;
          footFeedback(cleanErrText(err, "Undo failed"), true);
        }
      });
      actions.append(undo);
    } else if (rawSource === "RegistryRun") {
      const note = document.createElement("span");
      note.className = "manual-note";
      note.textContent = "Manual removal required";
      actions.append(note);
    } else {
      const btn = document.createElement("button");
      btn.className = "quarantine-btn";
      btn.textContent = "Quarantine";
      btn.addEventListener("click", async () => {
        const ok = await requestConfirm({
          kicker: "Confirm quarantine",
          title: "Move this item to quarantine?",
          facts: [
            ["Item", scored.entry.name],
            ["Location", scored.entry.location || "Not collected"],
            ["Action", "Move this file to C.U.R.E. quarantine."],
            ["Reversible", "Yes — it can be restored from Quarantine."],
          ],
          note: "This action changes the filesystem. The item will be listed under Quarantine with Undo.",
          okLabel: "Quarantine item",
        });
        if (!ok) return;
        btn.disabled = true;
        try {
          await invoke("quarantine_entry", { id: scored.entry.id, name: scored.entry.name, command: scored.entry.command });
          btn.textContent = "Quarantined ✓";
          btn.classList.add("row-done");
          sessionQuarantined += 1;
          refreshQuarantinedTile();
          showReceipt("Quarantined: " + scored.entry.name, "moved to quarantine · Undo in the Quarantine view", false);
          logEvent("action", "quarantined: " + scored.entry.name);
        } catch (err) {
          btn.disabled = false;
          footFeedback(cleanErrText(err, "Quarantine failed"), true);
          showReceipt("Quarantine failed", cleanErrText(err, "Quarantine failed"), true);
        }
      });
      actions.append(btn);
    }
    top.append(actions);
    card.append(top);

    const drawer = document.createElement("dl");
    drawer.className = "detail-drawer hidden";
    const rows = [
      ["Name", scored.entry.name],
      ["Source", (SOURCE_LABELS[rawSource] || rawSource) + (atk ? " · MITRE " + atk.id + " " + atk.name : "")],
      ["Command", scored.entry.command],
      ["Path", scored.entry.location],
      ["Score", String(scored.score) + " (" + scored.risk + ")"],
      ["Reasons", reasons.join(" · ") || "—"],
      ["Recommended action", recommendedAction(scored)],
    ];
    for (const [k, v] of rows) {
      const dt = document.createElement("dt");
      dt.textContent = k;
      const dd = document.createElement("dd");
      dd.textContent = v;
      if (k === "Recommended action") dd.classList.add("rec-action");
      drawer.append(dt, dd);
    }
    // Forensic rows, filled lazily on first expand (one backend round-trip
    // per card): signature/publisher plus shortcut/task specifics.
    const sigDt = document.createElement("dt");
    sigDt.textContent = "Signature";
    const sigDd = document.createElement("dd");
    sigDd.textContent = "…";
    const pubDt = document.createElement("dt");
    pubDt.textContent = "Publisher";
    const pubDd = document.createElement("dd");
    pubDd.textContent = "…";
    drawer.append(sigDt, sigDd, pubDt, pubDd);
    const evDt = document.createElement("dt");
    evDt.textContent = "Evidence";
    const evDd = document.createElement("dd");
    const evUl = document.createElement("ul");
    evUl.className = "evidence-list";
    for (const reason of reasons.length ? reasons : ["No scored signals — listed for context."]) {
      const li = document.createElement("li");
      li.textContent = reason;
      evUl.appendChild(li);
    }
    evDd.appendChild(evUl);
    drawer.append(evDt, evDd);
    let detailsLoaded = false;
    async function loadDetails() {
      if (detailsLoaded) return;
      detailsLoaded = true;
      let details = null;
      try {
        details = await invoke("entry_details", { id: scored.entry.id });
      } catch (_) {
        sigDd.textContent = "Unavailable";
        pubDd.textContent = "Unavailable";
        return;
      }
      if (!details) {
        sigDd.textContent = "Unavailable";
        pubDd.textContent = "Unavailable";
        return;
      }
      sigDd.textContent = details.signature || "UNKNOWN";
      pubDd.textContent = details.publisher || "Unavailable (unsigned or verdict-only check)";
      const extra = [];
      if (details.shortcut) {
        const sc = details.shortcut;
        if (sc.expanded_target) extra.push(["Shortcut target", sc.expanded_target + (sc.target_exists ? "" : "  [MISSING]")]);
        if (sc.info && sc.info.arguments) extra.push(["Shortcut arguments", sc.info.arguments]);
        if (sc.info && sc.info.working_dir) extra.push(["Shortcut workdir", sc.info.working_dir]);
      }
      if (details.task) {
        const t = details.task;
        t.actions.forEach((a, i) => {
          extra.push(["Action " + (i + 1), a.command + (a.arguments ? " " + a.arguments : "")]);
          if (a.working_dir) extra.push(["Action " + (i + 1) + " workdir", a.working_dir]);
        });
        if (t.author) extra.push(["Task author", t.author]);
        if (t.run_level) extra.push(["Run level", t.run_level]);
        if (t.user_id) extra.push(["Runs as", t.user_id]);
        if (t.triggers && t.triggers.length) extra.push(["Triggers", t.triggers.join(", ")]);
        extra.push(["Task enabled", t.enabled === false ? "No" : "Yes"]);
        if (t.hidden) extra.push(["Task hidden flag", "Yes"]);
      }
      for (const [k, v] of extra) {
        const dt = document.createElement("dt");
        dt.textContent = k;
        const dd = document.createElement("dd");
        dd.textContent = v;
        drawer.append(dt, dd);
      }
    }
    toggle.addEventListener("click", () => {
      const open = drawer.classList.toggle("hidden");
      toggle.setAttribute("aria-expanded", String(!open));
      toggle.textContent = open ? "View details" : "Hide details";
      if (!open) loadDetails();
    });
    card.append(drawer);
    return card;
  }

  function renderAudit(summary) {
    lastAuditSummary = summary;
    paintAuditList();
  }

  function paintAuditList() {
    const summary = lastAuditSummary;
    const list = document.getElementById("audit-list");
    if (!list || !summary) return;
    list.innerHTML = "";
    const entries = summary.high_risk_cleaned
      .map((e) => ({ e, cleaned: true }))
      .concat(summary.suspicious_for_review.map((e) => ({ e, cleaned: false })));
    entries.sort((a, b) => b.e.score - a.e.score);
    const shown = entries.filter(({ e }) => auditFilter === "all" || e.risk === auditFilter);
    for (const { e, cleaned } of shown) list.append(buildAuditCard(e, cleaned));
    const sub = document.getElementById("audit-subline");
    if (sub) sub.textContent = entries.length + " persistence finding(s) in the last scan — showing " + shown.length + " — nothing is disabled automatically.";
    const note = document.getElementById("audit-note");
    if (note) note.textContent = summary.safe + " safe entries are not listed individually. Publisher data is unavailable (signature checks are verdict-only).";
  }

  // ---- process sentinel view ----

  let procFilter = "all";
  let procCache = [];
  let auditFilter = "all";
  let lastAuditSummary = null;

  function procRow(p) {
    const tr = document.createElement("tr");
    const nameTd = document.createElement("td");
    nameTd.className = "proc-name";
    nameTd.textContent = p.name;
    const pidTd = document.createElement("td");
    pidTd.className = "mono";
    pidTd.textContent = String(p.pid);
    const pathTd = document.createElement("td");
    pathTd.className = "mono selectable trunc-cell";
    pathTd.tabIndex = 0;
    const short = (p.exe_path || "").split(/[/\\]/).pop() || "—";
    pathTd.textContent = short;
    pathTd.title = p.exe_path || "Not collected";
    const scoreTd = document.createElement("td");
    scoreTd.className = "mono";
    scoreTd.textContent = String(p.score);
    const riskTd = document.createElement("td");
    const badge = document.createElement("span");
    badge.className = "severity-badge " + (p.risk === "HighRisk" ? "sev-bad" : p.risk === "Suspicious" ? "sev-warn" : "sev-safe");
    badge.textContent = p.risk === "HighRisk" ? "HIGH RISK" : (p.risk || "").toUpperCase();
    riskTd.append(badge);
    const rsnTd = document.createElement("td");
    rsnTd.className = "row-reasons selectable";
    rsnTd.tabIndex = 0;
    const reasons = Array.isArray(p.reasons) ? p.reasons : [];
    rsnTd.textContent = reasons.slice(0, 3).join(" · ") || "—";
    rsnTd.title = reasons.join("\n") || "Not collected";
    const actTd = document.createElement("td");
    const kill = document.createElement("button");
    kill.className = "kill-one";
    kill.textContent = "Kill";
    kill.title = "Terminate this process (asks for confirmation)";
    kill.addEventListener("click", async () => {
      if (kill.disabled) return;
      const ok = await requestConfirm({
        kicker: "Confirm process termination",
        title: "Terminate this process now?",
        facts: [
          ["Process", p.name],
          ["PID", String(p.pid)],
          ["Path", p.exe_path || "Not collected"],
          ["Action", "Terminate the process immediately."],
          ["Reversible", "No — termination cannot be undone, but nothing is deleted."],
        ],
        note: "The PID is re-validated before termination. Re-run a scan afterwards to verify.",
        okLabel: "Terminate process",
      });
      if (!ok) return;
      kill.disabled = true;
      kill.textContent = "Killing…";
      try {
        const report = await invoke("kill_high_risk_processes", { processes: [[p.name, p.pid]] });
        const ok = (report.killed || []).length > 0;
        kill.textContent = ok ? "Killed" : "Failed";
        if (!ok) kill.disabled = false;
        const msg = ok ? "Killed " + p.name + " (pid " + p.pid + ")" : "Kill failed: " + (report.failed || []).join("; ");
        footFeedback(msg, !ok);
        logEvent("action", msg);
      } catch (err) {
        kill.disabled = false;
        kill.textContent = "Kill";
        footFeedback(cleanErrText(err, "Kill failed"), true);
      }
    });
    actTd.append(kill);
    tr.append(nameTd, pidTd, pathTd, scoreTd, riskTd, rsnTd, actTd);
    return tr;
  }

  function renderProcesses(summary) {
    procCache = summary.process_findings || [];
    const sub = document.getElementById("proc-subline");
    if (sub) sub.textContent = procCache.length + " flagged process(es) in the last scan. Killing requires confirmation per process — PIDs are re-validated before termination.";
    paintProcessRows();
  }

  function paintProcessRows() {
    const body = document.getElementById("proc-rows");
    if (!body) return;
    body.innerHTML = "";
    const rows = procCache.filter((p) => procFilter === "all" || p.risk === procFilter);
    for (const p of rows) body.append(procRow(p));
    const empty = document.getElementById("proc-empty");
    if (empty) {
      empty.classList.toggle("hidden", rows.length > 0);
      if (!rows.length) empty.textContent = procCache.length ? "No processes match this filter." : "No flagged processes in the last scan.";
    }
  }

  // ---- quarantine view (real backend data) ----

  async function refreshQuarantine() {
    const list = document.getElementById("q-list");
    const empty = document.getElementById("q-empty");
    if (!list) return;
    let records = [];
    try {
      records = await invoke("list_quarantine");
    } catch (err) {
      footFeedback(cleanErrText(err, "Could not list quarantine"), true);
      return;
    }
    list.innerHTML = "";
    for (const r of records) {
      const li = document.createElement("li");
      li.className = "review-card q-row";
      const top = document.createElement("div");
      top.className = "q-top";
      const name = document.createElement("span");
      name.className = "rc-name";
      name.textContent = r.name;
      name.title = r.original_path;
      const src = document.createElement("span");
      src.className = "chip src";
      src.textContent = r.source || "quarantine";
      const when = document.createElement("span");
      when.className = "q-archived";
      try { when.textContent = new Date(r.archived_at).toLocaleString(); }
      catch (_) { when.textContent = r.archived_at || ""; }
      const undo = document.createElement("button");
      undo.className = "copy-btn q-undo";
      undo.textContent = "Undo";
      undo.title = "Restore this file to its original location";
      undo.addEventListener("click", async () => {
        undo.disabled = true;
        try {
          await invoke("undo_entry", { id: r.id });
          logEvent("action", "restored from quarantine: " + r.name);
          sessionQuarantined = Math.max(0, sessionQuarantined - 1);
          refreshQuarantinedTile();
          showReceipt("Restored: " + r.name, "returned to " + (r.original_path || "its original location"), false);
          await refreshQuarantine();
        } catch (err) {
          undo.disabled = false;
          footFeedback(cleanErrText(err, "Undo failed"), true);
        }
      });
      top.append(name, src, when, undo);
      const paths = document.createElement("div");
      paths.className = "q-paths selectable";
      paths.tabIndex = 0;
      paths.textContent = r.original_path + "  →  " + r.quarantine_path;
      paths.title = "Original: " + r.original_path + "\nQuarantine: " + r.quarantine_path;
      const rev = document.createElement("div");
      rev.className = "q-rev dim";
      rev.textContent = "Reversible — restore with Undo. Quarantined items are never deleted.";
      li.append(top, paths, rev);
      list.append(li);
    }
    if (empty) empty.classList.toggle("hidden", records.length > 0);
    const sub = document.getElementById("q-subline");
    if (sub && records.length) sub.textContent = records.length + " item(s) in quarantine — nothing is ever deleted by quarantine. Restore any item with Undo.";
  }

  // ══════════════ incident investigation view ══════

  let incidentDuration = 30;
  let lastIncident = null;

  const VERDICT_CLASS = {
    CauseIdentified: "sev-bad",
    StrongCorrelation: "sev-bad",
    ReviewRequired: "sev-warn",
    NoDirectEvidence: "sev-safe",
    InsufficientObservation: "sev-warn",
  };

  const VERDICT_TEXT = {
    CauseIdentified: "CAUSE IDENTIFIED",
    StrongCorrelation: "STRONG CORRELATION",
    ReviewRequired: "REVIEW REQUIRED",
    NoDirectEvidence: "NO DIRECT EVIDENCE",
    InsufficientObservation: "INSUFFICIENT OBSERVATION",
  };

  const VERDICT_WHY = {
    CauseIdentified: "A startup entry's exact executable launched with a transient window on the same process.",
    StrongCorrelation: "A startup entry matches an observed launch. Consistent — not proof of intent.",
    ReviewRequired: "Only weak or partial relationships found — investigate manually.",
    NoDirectEvidence: "Observation completed; nothing links activity to startup persistence.",
    InsufficientObservation: "Observation failed or produced no usable data — not a clean bill of health.",
  };

  const LEVEL_CLASS = { Direct: "sev-bad", Strong: "sev-warn", Partial: "", Weak: "", None: "" };
  // Exact backend semantics (core/src/incident.rs module docs). Display only.
  const LEVEL_DEFS = {
    Direct: "finding's executable path matches the observed executable path (normalized). Service entries additionally require the reported PID to match.",
    Strong: "finding command and observed command line reference each other (executable substring either direction, or shared distinctive arguments ≥12 chars) with the launch inside the window.",
    Partial: "same executable file name, different paths (both shown).",
    Weak: "same folder but different program, or a child of a correlated process. Proximity only.",
    None: "no observed relationship.",
  };

  // Normalize a SourceStatus DTO (string "Available" from the mock, or the
  // serde-tagged object from Rust) into a display label + tone + detail.
  // Labels are the backend's own tokens: CHECKED / PARTIAL / UNAVAILABLE /
  // CHECK FAILED / ACCESS DENIED. Uncertainty never renders as success.
  // (Scan CoverageState "Checked" is also accepted defensively.)
  function sourceStatusInfo(status) {
    if (status === "Available" || status === "Checked" || (status && typeof status === "object" && ("Available" in status || "Checked" in status))) {
      return { label: "CHECKED", level: "ok", detail: "fully enumerated" };
    }
    if (typeof status === "string") {
      if (status === "Unavailable" || status === "NotChecked") return { label: status === "NotChecked" ? "NOT CHECKED" : "UNAVAILABLE", level: "idle", detail: "cannot run here" };
      if (status === "CheckFailed") return { label: "CHECK FAILED", level: "bad", detail: "enumeration failed" };
      if (status === "AccessDenied") return { label: "ACCESS DENIED", level: "bad", detail: "access denied (elevation may help)" };
      return { label: String(status).toUpperCase(), level: "idle", detail: "" };
    }
    if (status && typeof status === "object") {
      if ("Partial" in status) {
        const p = status.Partial || {};
        return { label: "PARTIAL", level: "warn", detail: (p.skipped || 0) + " skipped" + (p.reason ? " — " + p.reason : "") };
      }
      if ("Unavailable" in status) {
        const u = status.Unavailable || {};
        return { label: "UNAVAILABLE", level: "idle", detail: (typeof u === "string" ? u : u.reason) || "cannot run here" };
      }
      if ("CheckFailed" in status) {
        const f = status.CheckFailed || {};
        return { label: "CHECK FAILED", level: "bad", detail: (typeof f === "string" ? f : f.reason) || "enumeration failed" };
      }
      if ("AccessDenied" in status) {
        const a = status.AccessDenied || {};
        return { label: "ACCESS DENIED", level: "bad", detail: (typeof a === "string" ? a : a.reason) || "access denied (elevation may help)" };
      }
    }
    return { label: "NOT COLLECTED", level: "idle", detail: "" };
  }

  function pidOf(text) {
    const m = /pid\s+(\d+)/i.exec(String(text || ""));
    return m ? Number(m[1]) : null;
  }

  // Forensic drawer (evidence inspector). Focus moves to Close on open and
  // returns to the invoker on close; Escape and backdrop clicks close.
  let incDrawerInvoker = null;
  function openIncDrawer(title, sub, groups) {
    const overlay = document.getElementById("inc-drawer-overlay");
    const body = document.getElementById("inc-drawer-body");
    const titleEl = document.getElementById("inc-drawer-title");
    const subEl = document.getElementById("inc-drawer-sub");
    const closeBtn = document.getElementById("inc-drawer-close");
    if (!overlay || !body || !titleEl) return;
    incDrawerInvoker = document.activeElement;
    titleEl.textContent = title || "Evidence";
    if (subEl) subEl.textContent = sub || "";
    body.innerHTML = "";
    for (const g of groups || []) {
      const h = document.createElement("dt");
      h.className = "drawer-section";
      h.textContent = g.heading;
      body.appendChild(h);
      const spacer = document.createElement("dd");
      spacer.className = "drawer-section-val";
      spacer.textContent = "";
      body.appendChild(spacer);
      for (const [k, v] of g.rows || []) {
        const dt = document.createElement("dt");
        dt.textContent = k;
        const dd = document.createElement("dd");
        dd.className = "selectable";
        dd.textContent = (v === undefined || v === null || v === "") ? "Not collected" : String(v);
        if (v !== undefined && v !== null && String(v).length > 60) dd.title = String(v);
        body.appendChild(dt);
        body.appendChild(dd);
      }
    }
    overlay.classList.remove("hidden");
    if (closeBtn) closeBtn.focus();
  }
  function closeIncDrawer() {
    const overlay = document.getElementById("inc-drawer-overlay");
    if (overlay) overlay.classList.add("hidden");
    if (incDrawerInvoker && incDrawerInvoker.focus) incDrawerInvoker.focus();
    incDrawerInvoker = null;
  }

  function incLifetime(ms) {
    if (ms === undefined || ms === null) return "Not collected";
    return (ms / 1000).toFixed(2) + "s (±0.5 s poll precision)";
  }

  function buildCorrCard(c, procOf) {
    const li = document.createElement("li");
    li.className = "review-card";
    li.dataset.pid = String(c.process_pid);
    const main = document.createElement("div");
    main.className = "rc-main";
    const topRow = document.createElement("div");
    topRow.className = "rc-top";
    const name = document.createElement("span");
    name.className = "rc-name selectable trunc";
    name.textContent = (c.process_name || "Not collected") + " (pid " + c.process_pid + ")";
    name.title = (c.process_name || "Not collected") + " (pid " + c.process_pid + ")";
    name.tabIndex = 0;
    const badgeEl = document.createElement("span");
    badgeEl.className = "severity-badge " + (LEVEL_CLASS[c.level] || "");
    badgeEl.textContent = c.level;
    badgeEl.title = LEVEL_DEFS[c.level] || c.level;
    topRow.append(name, badgeEl);
    main.appendChild(topRow);
    const chips = document.createElement("div");
    chips.className = "chips";
    const fchip = document.createElement("span");
    fchip.className = "chip src selectable";
    fchip.textContent = (c.finding_name || "no finding") + (c.finding_source ? " · " + c.finding_source : "");
    fchip.title = "finding id: " + (c.finding_id || "Not collected");
    fchip.tabIndex = 0;
    chips.appendChild(fchip);
    const p = procOf ? procOf.get(c.process_pid) : null;
    if (p && p.ppid !== undefined) {
      const ppidChip = document.createElement("span");
      ppidChip.className = "chip";
      ppidChip.textContent = "ppid " + p.ppid;
      chips.appendChild(ppidChip);
    }
    main.appendChild(chips);
    if (c.evidence && c.evidence.length) {
      const ul = document.createElement("ul");
      ul.className = "evidence-list";
      for (const ev of c.evidence) {
        const item = document.createElement("li");
        item.className = "selectable";
        item.textContent = ev;
        ul.appendChild(item);
      }
      main.appendChild(ul);
    }
    const def = document.createElement("div");
    def.className = "corr-def dim";
    def.textContent = LEVEL_DEFS[c.level] || "";
    main.appendChild(def);
    li.appendChild(main);
    const inspect = document.createElement("button");
    inspect.className = "copy-btn";
    inspect.textContent = "Inspect evidence";
    inspect.addEventListener("click", () => {
      const rows = [
        ["Process", c.process_name || "Not collected"],
        ["PID", String(c.process_pid)],
        ["PPID", p ? String(p.ppid) : "Not collected"],
        ["Path", p ? (p.exe_path || "Not collected") : "Not collected"],
        ["Command line", p && p.command_line ? p.command_line : "Not collected"],
      ];
      openIncDrawer("Correlation · " + (c.level || "?"), (c.process_name || "?") + " ↔ " + (c.finding_name || "no finding"), [
        { heading: "CORRELATION", rows: [["Classification", (c.level || "Not collected") + " — " + (LEVEL_DEFS[c.level] || "")], ["Finding", (c.finding_name || "Not collected") + (c.finding_source ? " · " + c.finding_source : "")], ["Finding id", c.finding_id || "Not collected"], ["Related evidence", (c.evidence || []).join("; ") || "Not collected"]] },
        { heading: "EXECUTION", rows: rows.slice(3) },
        { heading: "IDENTITY", rows: rows.slice(0, 3) },
      ]);
    });
    li.appendChild(inspect);
    return li;
  }

  function setIncidentStatus(text, isError) {
    const el = document.getElementById("inc-status");
    if (el) {
      el.textContent = text;
      el.classList.toggle("hidden", !text);
    }
    if (text) logEvent(isError ? "info" : "action", "incident: " + text);
  }

  function renderIncident(result) {
    lastIncident = result;
    const badge = document.getElementById("inc-verdict");
    if (badge) {
      badge.textContent = VERDICT_TEXT[result.verdict] || result.verdict;
      badge.className = "severity-badge " + (VERDICT_CLASS[result.verdict] || "");
    }
    const why = document.getElementById("inc-verdict-why");
    if (why) why.textContent = VERDICT_WHY[result.verdict] || "";
    for (const id of ["inc-results", "inc-scope-panel", "inc-timeline-panel", "inc-review-panel", "inc-processes-panel", "inc-windows-panel", "inc-export-panel"]) {
      const el = document.getElementById(id);
      if (el) el.classList.remove("hidden");
    }
    const recPanel = document.getElementById("inc-recovery-panel");
    if (recPanel) recPanel.classList.toggle("hidden", result.verdict !== "InsufficientObservation");
    const meta = document.getElementById("inc-meta");
    if (meta) {
      meta.textContent = (result.investigation_id || "unknown id") + " · started " + (result.started_at || "Not collected") +
        " · observed " + (result.duration_secs !== undefined ? result.duration_secs + "s" : "Not collected") +
        " · " + (result.elevated ? "elevated" : "standard-user") +
        (result.truncated ? " · truncated — results capped" : " · complete");
    }
    const receipt = document.getElementById("inc-receipt");
    if (receipt) {
      receipt.classList.remove("hidden");
      receipt.textContent = "Investigation " + (result.investigation_id || "") + " complete: " +
        (VERDICT_TEXT[result.verdict] || result.verdict || "Not collected") + " — observed " +
        (result.processes || []).length + " processes, " + (result.windows || []).length + " windows, " +
        (result.correlations || []).length + " correlations.";
    }
    const scope = document.getElementById("inc-scope");
    if (scope) {
      scope.innerHTML = "";
      const winInfo = sourceStatusInfo(result.window_observation);
      const procInfo = sourceStatusInfo(result.process_observation);
      const winRow = covRow("Window observation", winInfo.label + (winInfo.detail ? " — " + winInfo.detail : ""), winInfo.level);
      winRow.title = "Titles + classes only — no screenshots, no keystrokes, no contents.";
      const procRowEl = covRow("Process observation", procInfo.label + (procInfo.detail ? " — " + procInfo.detail : ""), procInfo.level);
      procRowEl.title = "Command lines are fetched only for correlated processes (bounded).";
      const durRow = covRow("Observation window", (result.duration_secs !== undefined ? result.duration_secs + "s" : "Not collected") + (result.elevated ? " · elevated" : " · standard-user"), "idle");
      scope.append(durRow, procRowEl, winRow);
    }
    const procOf = new Map((result.processes || []).map((p) => [p.pid, p]));
    const corrByPid = new Map();
    for (const c of result.correlations || []) {
      if (!corrByPid.has(c.process_pid)) corrByPid.set(c.process_pid, []);
      corrByPid.get(c.process_pid).push(c);
    }
    const winsByPid = new Map();
    for (const w of result.windows || []) {
      if (!winsByPid.has(w.pid)) winsByPid.set(w.pid, []);
      winsByPid.get(w.pid).push(w);
    }
    const tl = document.getElementById("incident-timeline");
    if (tl) {
      tl.innerHTML = "";
      for (const e of result.timeline || []) {
        const li = document.createElement("li");
        li.className = "inc-event";
        const pid = pidOf(e.text);
        const proc = (pid !== null && procOf.get(pid)) || null;
        const top = document.createElement("div");
        top.className = "inc-event-top";
        const time = document.createElement("span");
        time.className = "ev-time";
        time.textContent = e.wall_time || "Not collected";
        const tag = document.createElement("b");
        tag.textContent = "[" + String(e.kind || "event") + "]";
        const body = document.createElement("span");
        body.className = "selectable";
        body.textContent = e.text || "";
        top.append(time, tag, body);
        li.appendChild(top);
        const det = document.createElement("div");
        det.className = "inc-event-det";
        const fields = [];
        fields.push(["process", proc ? proc.name : "Not collected"]);
        fields.push(["pid", pid !== null ? String(pid) : "Not collected"]);
        fields.push(["ppid", proc ? String(proc.ppid) : "Not collected"]);
        fields.push(["path", proc ? (proc.exe_path || "Not collected") : "Not collected"]);
        fields.push(["command line", proc && proc.command_line ? proc.command_line : "Not collected"]);
        const corrs = pid !== null ? (corrByPid.get(pid) || []) : [];
        fields.push(["correlation", corrs.length ? corrs.map((c) => c.level + ": " + (c.finding_name || "finding")).join("; ") : "Not collected"]);
        let winText = "Not collected";
        if (String(e.kind || "").toLowerCase().includes("window")) {
          const match = (result.windows || []).find((w) => w.title && String(e.text || "").includes(w.title));
          if (match) winText = "“" + match.title + "” · class " + (match.class_name || "Not collected") + " · pid " + match.pid;
          else if (pid !== null && winsByPid.get(pid)) {
            winText = winsByPid.get(pid).map((w) => "“" + (w.title || "(no title)") + "” · " + (w.class_name || "?")).join("; ");
          }
        } else if (pid !== null && winsByPid.get(pid)) {
          winText = winsByPid.get(pid).map((w) => "“" + (w.title || "(no title)") + "”").join("; ");
        }
        fields.push(["window", winText]);
        for (const [k, v] of fields) {
          const s = document.createElement("span");
          s.className = "inc-field selectable trunc";
          s.tabIndex = 0;
          s.textContent = k + ": " + v;
          s.title = k + ": " + v;
          det.appendChild(s);
        }
        li.appendChild(det);
        if (proc) {
          const btn = document.createElement("button");
          btn.className = "copy-btn inc-inspect";
          btn.textContent = "Inspect pid " + proc.pid;
          btn.addEventListener("click", () => {
            const pc = corrByPid.get(proc.pid) || [];
            const ws = winsByPid.get(proc.pid) || [];
            openIncDrawer(proc.name || ("pid " + proc.pid), "pid " + proc.pid + " · ppid " + proc.ppid, [
              { heading: "IDENTITY", rows: [["Name", proc.name || "Not collected"], ["PID", String(proc.pid)], ["PPID", String(proc.ppid)]] },
              { heading: "EXECUTION", rows: [["Path", proc.exe_path || "Not collected"], ["Command line", proc.command_line || "Not collected"], ["Creation source", proc.via_events ? "WMI creation event" : "snapshot diff"], ["Publisher", "Not collected"], ["Signature", "Not collected"]] },
              { heading: "OBSERVATION", rows: [["First seen", "+" + proc.first_seen_ms + " ms"], ["Last seen", "+" + proc.last_seen_ms + " ms"], ["Lifetime", incLifetime((proc.last_seen_ms || 0) - (proc.first_seen_ms || 0))], ["Exited", proc.exited ? "Yes" : "No"], ["Baseline context", proc.pre_existing ? "already running when observation started" : "created during the window"]] },
              { heading: "WINDOW", rows: ws.length ? ws.map((w, i) => ["Window " + (i + 1), "“" + (w.title || "(no title)") + "” · " + (w.class_name || "?") + " · " + incLifetime((w.last_seen_ms || 0) - (w.first_seen_ms || 0))]) : [["Window", "No window observed for this PID"]] },
              { heading: "CORRELATION", rows: pc.length ? pc.map((c, i) => ["Link " + (i + 1), c.level + ": " + (c.finding_name || "") + " — " + (LEVEL_DEFS[c.level] || "")]) : [["Classification", "No startup correlation"]] },
              { heading: "SOURCE STATUS", rows: [["Process observation", sourceStatusInfo(result.process_observation).label], ["Window observation", sourceStatusInfo(result.window_observation).label]] },
            ]);
          });
          li.appendChild(btn);
        }
        tl.appendChild(li);
      }
      if (!(result.timeline || []).length) {
        const li = document.createElement("li");
        li.textContent = "No timeline events recorded — see Limitations below.";
        tl.appendChild(li);
      }
    }
    const corrStrong = document.getElementById("inc-corr-strong");
    const corr = document.getElementById("inc-correlations");
    const corrEmpty = document.getElementById("inc-corr-empty");
    if (corrStrong) corrStrong.innerHTML = "";
    if (corr) corr.innerHTML = "";
    {
      const strong = (result.correlations || []).filter((c) => c.level === "Direct" || c.level === "Strong");
      const weak = (result.correlations || []).filter((c) => c.level !== "Direct" && c.level !== "Strong");
      for (const c of strong) if (corrStrong) corrStrong.appendChild(buildCorrCard(c, procOf));
      for (const c of weak) if (corr) corr.appendChild(buildCorrCard(c, procOf));
      const total = (result.correlations || []).length;
      if (corrEmpty) corrEmpty.classList.toggle("hidden", total > 0);
      if (!strong.length && corrStrong) {
        const li = document.createElement("li");
        li.className = "review-card";
        li.textContent = total ? "No supporting (DIRECT/STRONG) evidence in this observation." : "No supporting (DIRECT/STRONG) evidence in this observation.";
        corrStrong.appendChild(li);
      }
      if (!weak.length && corr) {
        const li = document.createElement("li");
        li.className = "review-card";
        li.textContent = total ? "No weak or partial signals in this observation." : "No observed process relates to a startup finding.";
        corr.appendChild(li);
      }
      if (!total) {
        if (corrStrong && !corrStrong.children.length) {
          const li = document.createElement("li");
          li.className = "review-card";
          li.textContent = "No supporting (DIRECT/STRONG) evidence in this observation.";
          corrStrong.appendChild(li);
        }
        if (corr && !corr.children.length) {
          const li = document.createElement("li");
          li.className = "review-card";
          li.textContent = "No observed process relates to a startup finding.";
          corr.appendChild(li);
        }
      }
    }
    const procList = document.getElementById("inc-processes");
    const procEmpty = document.getElementById("inc-processes-empty");
    if (procList) {
      procList.innerHTML = "";
      const procs = (result.processes || []).slice().sort((a, b) => (a.first_seen_ms || 0) - (b.first_seen_ms || 0));
      for (const p of procs) {
        const li = document.createElement("li");
        li.className = "review-card";
        li.dataset.pid = String(p.pid);
        const main = document.createElement("div");
        main.className = "rc-main";
        const topRow = document.createElement("div");
        topRow.className = "rc-top";
        const name = document.createElement("span");
        name.className = "rc-name selectable trunc";
        name.tabIndex = 0;
        name.textContent = (p.name || "Not collected") + " (pid " + p.pid + " · ppid " + p.ppid + ")";
        name.title = (p.exe_path || "Not collected") + (p.command_line ? "\n" + p.command_line : "\ncommand line: Not collected");
        const corr0 = (corrByPid.get(p.pid) || [])[0];
        const badgeEl = document.createElement("span");
        badgeEl.className = "severity-badge " + (corr0 ? (LEVEL_CLASS[corr0.level] || "") : "");
        badgeEl.textContent = corr0 ? corr0.level : "NO LINK";
        badgeEl.title = corr0 ? (LEVEL_DEFS[corr0.level] || "") : "No startup correlation for this process.";
        topRow.append(name, badgeEl);
        main.appendChild(topRow);
        const chips = document.createElement("div");
        chips.className = "chips";
        const pathChip = document.createElement("span");
        pathChip.className = "chip src selectable trunc";
        pathChip.tabIndex = 0;
        pathChip.textContent = p.exe_path || "Not collected";
        pathChip.title = p.exe_path || "Not collected";
        chips.appendChild(pathChip);
        const lifeChip = document.createElement("span");
        lifeChip.className = "chip";
        const lifeMs = (p.last_seen_ms || 0) - (p.first_seen_ms || 0);
        lifeChip.textContent = (p.exited ? "exited · " : "running · ") + (lifeMs / 1000).toFixed(2) + "s";
        lifeChip.title = "Lifetime " + incLifetime(lifeMs) + (p.pre_existing ? " · already running when observation started" : "") + (p.via_events ? " · first noticed via WMI creation event" : " · first noticed via snapshot diff");
        chips.appendChild(lifeChip);
        main.appendChild(chips);
        const cmd = document.createElement("div");
        cmd.className = "inc-cmd selectable trunc";
        cmd.tabIndex = 0;
        cmd.textContent = "cmd: " + (p.command_line || "Not collected");
        cmd.title = p.command_line || "Not collected";
        main.appendChild(cmd);
        li.appendChild(main);
        const inspect = document.createElement("button");
        inspect.className = "copy-btn";
        inspect.textContent = "Inspect";
        inspect.addEventListener("click", () => {
          const pc = corrByPid.get(p.pid) || [];
          const ws = winsByPid.get(p.pid) || [];
          openIncDrawer(p.name || ("pid " + p.pid), "pid " + p.pid + " · ppid " + p.ppid, [
            { heading: "IDENTITY", rows: [["Name", p.name || "Not collected"], ["PID", String(p.pid)], ["PPID", String(p.ppid)]] },
            { heading: "EXECUTION", rows: [["Path", p.exe_path || "Not collected"], ["Command line", p.command_line || "Not collected"], ["Creation source", p.via_events ? "WMI creation event" : "snapshot diff"]] },
            { heading: "OBSERVATION", rows: [["First seen", "+" + p.first_seen_ms + " ms"], ["Last seen", "+" + p.last_seen_ms + " ms"], ["Lifetime", incLifetime(lifeMs)], ["Exited", p.exited ? "Yes" : "No"], ["Baseline context", p.pre_existing ? "already running when observation started" : "created during the window"]] },
            { heading: "WINDOW", rows: ws.length ? ws.map((w, i) => ["Window " + (i + 1), "“" + (w.title || "(no title)") + "” · " + (w.class_name || "?")]) : [["Window", "No window observed for this PID"]] },
            { heading: "CORRELATION", rows: pc.length ? pc.map((c, i) => ["Link " + (i + 1), c.level + ": " + (c.finding_name || "")]) : [["Classification", "No startup correlation"]] },
          ]);
        });
        li.appendChild(inspect);
        procList.appendChild(li);
      }
      if (procEmpty) procEmpty.classList.toggle("hidden", procs.length > 0);
    }
    const wins = document.getElementById("inc-windows");
    const winsEmpty = document.getElementById("inc-windows-empty");
    if (wins) {
      wins.innerHTML = "";
      const all = (result.windows || []).slice().sort((a, b) => (a.first_seen_ms || 0) - (b.first_seen_ms || 0));
      for (const w of all) {
        const li = document.createElement("li");
        li.className = "review-card";
        li.dataset.pid = String(w.pid);
        const main = document.createElement("div");
        main.className = "rc-main";
        const topRow = document.createElement("div");
        topRow.className = "rc-top";
        const name = document.createElement("span");
        name.className = "rc-name selectable trunc";
        name.tabIndex = 0;
        name.textContent = "“" + (w.title || "(no title)") + "”";
        name.title = "class " + (w.class_name || "Not collected") + "\ntitle: " + (w.title || "(no title)");
        const ms = (w.last_seen_ms || 0) - (w.first_seen_ms || 0);
        const transient = w.closed && ms < 5000;
        const life = document.createElement("span");
        life.className = "score-chip low";
        life.textContent = (transient ? "TRANSIENT · " : w.closed ? "closed · " : "open · ") + (ms / 1000).toFixed(2) + "s";
        life.title = "Visible lifetime (poll granularity ±0.5 s)" + (w.pre_existing ? " · already open when observation started" : "");
        topRow.append(name, life);
        main.appendChild(topRow);
        const chips = document.createElement("div");
        chips.className = "chips";
        const pidChip = document.createElement("span");
        pidChip.className = "chip";
        pidChip.textContent = "pid " + w.pid;
        chips.appendChild(pidChip);
        const clsChip = document.createElement("span");
        clsChip.className = "chip selectable trunc";
        clsChip.tabIndex = 0;
        clsChip.textContent = "class " + (w.class_name || "Not collected");
        clsChip.title = w.class_name || "Not collected";
        chips.appendChild(clsChip);
        const corr0 = (corrByPid.get(w.pid) || []).find((c) => c.level !== "None") || (corrByPid.get(w.pid) || [])[0];
        const corrChip = document.createElement("span");
        corrChip.className = "chip" + (corr0 ? " amber" : "");
        corrChip.textContent = corr0 ? (corr0.level + ": " + corr0.finding_name) : "no startup correlation";
        corrChip.title = corr0 ? (LEVEL_DEFS[corr0.level] || "") : "No observed relationship for this window's process.";
        chips.appendChild(corrChip);
        const sigChip = document.createElement("span");
        sigChip.className = "chip";
        sigChip.textContent = "metadata only — no capture";
        chips.appendChild(sigChip);
        main.appendChild(chips);
        li.appendChild(main);
        const row2 = document.createElement("div");
        row2.className = "audit-actions";
        const inspect = document.createElement("button");
        inspect.className = "copy-btn";
        inspect.textContent = "Inspect";
        inspect.addEventListener("click", () => {
          const p = procOf.get(w.pid) || null;
          openIncDrawer("“" + (w.title || "(no title)") + "”", "pid " + w.pid + " · " + (transient ? "transient" : w.closed ? "closed" : "open"), [
            { heading: "IDENTITY", rows: [["Window title", w.title || "Not collected"], ["Window class", w.class_name || "Not collected"], ["PID", String(w.pid)], ["Process", p ? (p.name || "Not collected") : "Not collected"], ["PPID", p ? String(p.ppid) : "Not collected"]] },
            { heading: "OBSERVATION", rows: [["First seen", "+" + w.first_seen_ms + " ms"], ["Last seen", "+" + w.last_seen_ms + " ms"], ["Lifetime", incLifetime(ms)], ["Closed", w.closed ? "Yes" : "No"], ["Transient status", transient ? "transient (closed after a brief life — short life alone means nothing about intent)" : "not transient"], ["Baseline context", w.pre_existing ? "already open when observation started" : "opened during the window"]] },
            { heading: "EXECUTION", rows: [["Path", p ? (p.exe_path || "Not collected") : "Not collected"], ["Command line", p && p.command_line ? p.command_line : "Not collected"]] },
            { heading: "CORRELATION", rows: corr0 ? [["Classification", corr0.level + " — " + (LEVEL_DEFS[corr0.level] || "")], ["Finding", (corr0.finding_name || "") + (corr0.finding_source ? " · " + corr0.finding_source : "")]] : [["Classification", "No startup correlation"]] },
          ]);
        });
        const copy = document.createElement("button");
        copy.className = "copy-btn";
        copy.textContent = "Copy evidence";
        copy.addEventListener("click", () => {
          copyEvidence(
            "[CURE window] " + (w.title || "(no title)") + " | pid " + w.pid +
            " | class " + (w.class_name || "Not collected") + " | lifetime " + ms + " ms" +
            (corr0 ? " | " + corr0.level + ": " + corr0.finding_name : " | no startup correlation"),
            copy
          );
        });
        row2.append(inspect, copy);
        li.append(row2);
        wins.appendChild(li);
      }
      if (winsEmpty) {
        const transientCount = all.filter((w) => w.closed && ((w.last_seen_ms || 0) - (w.first_seen_ms || 0)) < 5000).length;
        winsEmpty.classList.toggle("hidden", all.length > 0);
        winsEmpty.textContent = all.length ? (transientCount + " transient window(s) of " + all.length + " observed — all windows listed above with titles/classes only.") : "No windows observed in this window.";
      }
    }
    try { updateOvIncident(); } catch (_) { /* overview not ready */ }
    const incView = document.getElementById("view-incident");
    if (incView && !incView.classList.contains("hidden")) focusViewHeading(incView);
  }

  async function exportIncident(format) {
    if (!lastIncident) {
      footFeedback("Run an investigation first", true);
      return;
    }
    const redactBox = document.getElementById("exp-redact");
    const redact = redactBox ? !!redactBox.checked : true;
    footFeedback("Exporting incident " + format.toUpperCase() + "…", false);
    try {
      const path = await invoke("export_incident_report", { result: lastIncident, format, redact });
      footFeedback("Incident report saved: " + path, false);
      logEvent("action", "exported incident " + format.toUpperCase() + " to " + path);
    } catch (err) {
      footFeedback(cleanErrText(err, "Incident export failed"), true);
    }
  }

  listen("incident-progress", (event) => {
    const p = event.payload || {};
    const el = document.getElementById("inc-progress");
    if (el) {
      el.classList.remove("hidden");
      el.textContent = "Observing… " + (p.polls || 0) + " polls · " + (p.processes || 0) + " processes · " + (p.windows || 0) + " windows";
    }
  });

  // Overlay review cards: one per candidate window. Every action is an
  // explicit per-window click — Close (graceful WM_CLOSE), Force close
  // (terminate, explicit only), Don't-ask-again (path+hash allowlist).
  // All attacker-controlled strings (title, path, process) go through
  // textContent, never innerHTML.
  function renderOverlayReview(candidates, lines) {
    const done = new Set();
    function maybeFinish() {
      if (done.size === candidates.length) {
        appendLog("overlay", "review complete: " + lines.filter(function (l) { return l.indexOf("overlay:") === 0; }).length + " action(s)");
        runScan(lines);
      }
    }
    candidates.forEach(function (c) {
      const li = document.createElement("li");
      li.className = "item-line fresh risk-high";
      const head = document.createElement("span");
      head.className = "iname";
      head.textContent = "overlay candidate: " + c.process + " (pid " + c.pid + ")";
      const detail = document.createElement("span");
      detail.className = "isrc";
      detail.textContent = c.path + " · " + c.signature + " · " + c.width + "x" + c.height + " (" + Math.round(c.coverage_pct) + "% of monitor) · " + c.title;
      const btnRow = document.createElement("span");
      function markDone(label) {
        done.add(c.hwnd);
        Array.prototype.forEach.call(btnRow.querySelectorAll("button"), function (b) { b.disabled = true; });
        lines.push(label);
        appendLog("overlay", label);
        maybeFinish();
      }
      const closeBtn = document.createElement("button");
      closeBtn.className = "btn";
      closeBtn.textContent = "Close window";
      closeBtn.addEventListener("click", async () => {
        closeBtn.disabled = true;
        try {
          const r = await invoke("close_overlay_window", { hwnd: c.hwnd, force: false });
          if (r && r.closed) markDone("overlay: closed " + c.process + " (graceful)");
          else { closeBtn.disabled = false; appendLog("overlay", "still open (use Force close to terminate): " + c.process); }
        } catch (e) { closeBtn.disabled = false; appendLog("error", String(e)); }
      });
      const forceBtn = document.createElement("button");
      forceBtn.className = "btn btn-danger";
      forceBtn.textContent = "Force close";
      forceBtn.title = "Terminate the owning process. Use only for a window you have reviewed.";
      forceBtn.addEventListener("click", async () => {
        if (!window.confirm("Terminate " + c.process + " (pid " + c.pid + ")? Unsaved work in that process will be lost.")) return;
        forceBtn.disabled = true;
        try {
          const r = await invoke("close_overlay_window", { hwnd: c.hwnd, force: true });
          if (r && r.closed) markDone("overlay: force-closed " + c.process + " (terminated)");
          else { forceBtn.disabled = false; appendLog("overlay", "force close failed: " + c.process); }
        } catch (e) { forceBtn.disabled = false; appendLog("error", String(e)); }
      });
      const allowBtn = document.createElement("button");
      allowBtn.className = "ghost-btn";
      allowBtn.textContent = "Don't ask again for this app";
      allowBtn.title = "Allowlist this exact binary (path + hash). Future matches skip it.";
      allowBtn.addEventListener("click", async () => {
        allowBtn.disabled = true;
        try {
          const msg = await invoke("allowlist_overlay", { path: c.path });
          markDone("overlay: " + msg);
        } catch (e) { allowBtn.disabled = false; appendLog("error", String(e)); }
      });
      btnRow.append(closeBtn, document.createTextNode(" "), forceBtn, document.createTextNode(" "), allowBtn);
      li.append(head, document.createElement("br"), detail, document.createElement("br"), btnRow);
      logList.appendChild(li);
    });
    const contLi = document.createElement("li");
    const contBtn = document.createElement("button");
    contBtn.className = "btn";
    contBtn.textContent = "Continue scan without closing →";
    contBtn.addEventListener("click", () => {
      contBtn.disabled = true;
      lines.push("overlay: review skipped by operator (" + (candidates.length - done.size) + " candidate(s) left open)");
      runScan(lines);
    });
    contLi.appendChild(contBtn);
    logList.appendChild(contLi);
    logList.scrollTop = logList.scrollHeight;
  }

  (async () => {
    scanView.classList.add("hidden");
    landingView.classList.remove("hidden");
    setNav("scan-center");
    document.getElementById("start-rescue-btn").addEventListener("click", async () => {
      setPill("scanning", "checking for suspicious overlays…");
      await switchView(landingView, scanView);
      setNav("scan-view");
      const lines = [];
      let rep = null;
      try {
        rep = await invoke("list_overlay_candidates");
      } catch (e) {
        lines.push("Overlay check unavailable: " + e);
        runScan(lines);
        return;
      }
      const candidates = (rep && rep.candidates) || [];
      appendLog("overlay", "checked " + (rep ? rep.checked : 0) + " windows, " + candidates.length + " candidate(s) need review");
      if (candidates.length === 0) {
        lines.push("No suspicious overlay windows found");
        runScan(lines);
        return;
      }
      // Matches are SHOWN, never closed here: each card carries its own
      // Close / Force close / Don't-ask-again buttons, plus one button to
      // continue the scan. Nothing closes without a per-window click.
      renderOverlayReview(candidates, lines);
    });
    document.getElementById("start-cleanup-btn").addEventListener("click", async () => {
      cleanupState.open = true;
      cleanupState.savedPill = { cls: statusPill.className, text: statusText.textContent };
      await switchView(currentView(), cleanupView);
      setNav("cleanup-view");
      showCleanupIdle();
    });
    document.querySelectorAll(".nav-item").forEach((btn) => {
      btn.addEventListener("click", () => {
        if (!btn.disabled) navTo(btn.getAttribute("data-view"));
      });
    });
    const canToggle2 = document.getElementById("can-toggle");
    if (canToggle2) canToggle2.addEventListener("click", toggleCanary);
    const qRefresh = document.getElementById("q-refresh");
    if (qRefresh) qRefresh.addEventListener("click", refreshQuarantine);
    async function exportReport(format) {
      const redactBox = document.getElementById("exp-redact");
      const redact = redactBox ? !!redactBox.checked : true;
      footFeedback("Exporting " + format.toUpperCase() + " report…", false);
      try {
        const path = await invoke("export_report", { format, redact });
        footFeedback("Report saved: " + path, false);
        logEvent("action", "exported " + format.toUpperCase() + " report to " + path);
      } catch (err) {
        footFeedback(cleanErrText(err, "Report export failed"), true);
      }
    }
    const expTxt = document.getElementById("exp-txt");
    if (expTxt) expTxt.addEventListener("click", () => exportReport("txt"));
    const expJson = document.getElementById("exp-json");
    if (expJson) expJson.addEventListener("click", () => exportReport("json"));
    const logClear = document.getElementById("log-clear");
    if (logClear) logClear.addEventListener("click", () => {
      eventLog.length = 0;
      renderEventLog();
    });
    document.querySelectorAll("#view-processes .filter-btn[data-f]").forEach((btn) => {
      btn.addEventListener("click", () => {
        document.querySelectorAll("#view-processes .filter-btn[data-f]").forEach((b) => b.classList.remove("on"));
        btn.classList.add("on");
        procFilter = btn.getAttribute("data-f") || "all";
        paintProcessRows();
      });
    });
    document.querySelectorAll("#view-audit .filter-btn[data-af]").forEach((btn) => {
      btn.addEventListener("click", () => {
        document.querySelectorAll("#view-audit .filter-btn[data-af]").forEach((b) => b.classList.remove("on"));
        btn.classList.add("on");
        auditFilter = btn.getAttribute("data-af") || "all";
        paintAuditList();
      });
    });
    document.querySelectorAll("#view-incident .filter-btn[data-dur]").forEach((btn) => {
      btn.addEventListener("click", () => {
        document.querySelectorAll("#view-incident .filter-btn[data-dur]").forEach((b) => b.classList.remove("on"));
        btn.classList.add("on");
        incidentDuration = Number(btn.getAttribute("data-dur")) || 30;
      });
    });
    const incDrawerOverlay = document.getElementById("inc-drawer-overlay");
    const incDrawerClose = document.getElementById("inc-drawer-close");
    if (incDrawerClose) incDrawerClose.addEventListener("click", closeIncDrawer);
    if (incDrawerOverlay) incDrawerOverlay.addEventListener("mousedown", (ev) => {
      if (ev.target === incDrawerOverlay) closeIncDrawer();
    });
    document.addEventListener("keydown", (ev) => {
      if (ev.key === "Escape") {
        const ov = document.getElementById("inc-drawer-overlay");
        if (ov && !ov.classList.contains("hidden")) {
          ev.stopPropagation();
          closeIncDrawer();
        }
      }
    }, true);
    const incStart = document.getElementById("btn-incident-start");
    if (incStart) incStart.addEventListener("click", async () => {
      if (incStart.disabled) return;
      incStart.disabled = true;
      setIncidentStatus("Observing startup activity for " + incidentDuration + " s — popups welcome, machine otherwise idle.", false);
      const prog = document.getElementById("inc-progress");
      if (prog) {
        prog.classList.remove("hidden");
        prog.textContent = "Observing…";
      }
      try {
        const result = await invoke("start_incident_observation", { durationSecs: incidentDuration });
        renderIncident(result);
        const counts = "observed " + (result.processes || []).length + " processes, " + (result.windows || []).length + " windows, " + (result.correlations || []).length + " correlations.";
        setIncidentStatus("Investigation " + (result.investigation_id || "") + " complete: " + ((result.verdict || "")).replace(/([A-Z])/g, " $1").trim() + ". " + counts, false);
      } catch (err) {
        setIncidentStatus(cleanErrText(err, "Investigation failed"), true);
      } finally {
        incStart.disabled = false;
        if (prog) prog.classList.add("hidden");
      }
    });
    const expIncTxt = document.getElementById("exp-inc-txt");
    if (expIncTxt) expIncTxt.addEventListener("click", () => exportIncident("txt"));
    const expIncJson = document.getElementById("exp-inc-json");
    if (expIncJson) expIncJson.addEventListener("click", () => exportIncident("json"));
  })();
})();
