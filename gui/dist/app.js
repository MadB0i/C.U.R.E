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

  const radar = (() => {
    const stage = document.getElementById("net-stage");
    const canvas = document.getElementById("radar");
    const ctx = canvas.getContext("2d");
    const DPR = Math.min(window.devicePixelRatio || 1, 2);
    const ACCENT = (a) => "rgba(77, 227, 176, " + a + ")";

    const RGB = {
      Safe: [77, 227, 176],
      Suspicious: [255, 196, 107],
      HighRisk: [255, 93, 110],
    };
    const TRAVEL = 360;

    let rafId = null;
    let nodes = [];
    let pings = [];
    let pulses = [];
    let lastPulse = 0;
    let nodeSeq = 0;
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
      vg.addColorStop(0, "rgba(2,5,9,0)");
      vg.addColorStop(1, "rgba(2,5,9,0.62)");
      ctx.fillStyle = vg;
      ctx.fillRect(0, 0, W, H);

      const glow = ctx.createRadialGradient(CX, CY, 0, CX, CY, M);
      glow.addColorStop(0, "rgba(77,227,176,0.07)");
      glow.addColorStop(0.45, "rgba(77,227,176,0.025)");
      glow.addColorStop(1, "rgba(77,227,176,0)");
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
      halo.addColorStop(0, ACCENT((0.30 + 0.18 * breathe).toFixed(3)));
      halo.addColorStop(1, ACCENT(0));
      ctx.fillStyle = halo;
      ctx.beginPath();
      ctx.arc(CX, CY, haloR, 0, Math.PI * 2);
      ctx.fill();

      ctx.lineWidth = 1.2 * DPR;
      if (!REDUCED) {
        ctx.strokeStyle = ACCENT(0.65);
        ctx.beginPath();
        ctx.arc(CX, CY, 15 * k * DPR, now / 2400, now / 2400 + 1.15);
        ctx.stroke();
        ctx.strokeStyle = ACCENT(0.35);
        ctx.beginPath();
        ctx.arc(CX, CY, 20 * k * DPR, -now / 3600, -now / 3600 + 0.7);
        ctx.stroke();
      } else {
        ctx.strokeStyle = ACCENT(0.45);
        ring(CX, CY, 15 * k * DPR);
      }

      ctx.fillStyle = "#eafff6";
      ctx.shadowColor = "rgba(77, 227, 176, 0.95)";
      ctx.shadowBlur = (10 + 10 * breathe) * DPR;
      ctx.beginPath();
      ctx.arc(CX, CY, (3.4 + 1.1 * breathe) * k * DPR, 0, Math.PI * 2);
      ctx.fill();
      ctx.shadowBlur = 0;
    }

    function edgeAlpha(risk) {
      if (risk === "HighRisk") return 0.3;
      if (risk === "Suspicious") return 0.24;
      return 0.17;
    }

    function labelAlphaFor(risk) {
      if (risk === "HighRisk") return 0.78;
      if (risk === "Suspicious") return 0.62;
      return 0.38;
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
      return nodes.length <= 60 || nd.risk !== "Safe";
    }

    function drawNetwork(now) {
      ctx.textBaseline = "middle";
      for (const nd of nodes) {
        const t = Math.min((now - nd.born) / TRAVEL, 1);
        const ease = 1 - Math.pow(1 - t, 3);
        const p = nodeXY(nd);
        const c = RGB[nd.risk] || RGB.Safe;
        const gapX = CX + Math.cos(nd.ang) * 12 * DPR;
        const gapY = CY + Math.sin(nd.ang) * 12 * DPR;
        const hx = gapX + (p.x - gapX) * ease;
        const hy = gapY + (p.y - gapY) * ease;

        if (t < 1) {
          ctx.strokeStyle = rgba(c, (0.65 * (0.35 + 0.65 * t)).toFixed(3));
          ctx.lineWidth = 1.3 * DPR;
          ctx.beginPath();
          ctx.moveTo(gapX, gapY);
          ctx.lineTo(hx, hy);
          ctx.stroke();
          ctx.fillStyle = "rgba(234,255,246,0.95)";
          ctx.shadowColor = rgba(c, 0.95);
          ctx.shadowBlur = 11 * DPR;
          ctx.beginPath();
          ctx.arc(hx, hy, 2.4 * DPR, 0, Math.PI * 2);
          ctx.fill();
          ctx.shadowBlur = 0;
        } else {
          ctx.strokeStyle = rgba(c, edgeAlpha(nd.risk));
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
          ctx.shadowBlur = (2.5 + flash * 10) * DPR;
          ctx.beginPath();
          ctx.arc(p.x, p.y, dotRadius(nd.risk) * (1 + flash * 0.45) * DPR, 0, Math.PI * 2);
          ctx.fill();
          ctx.shadowBlur = 0;
        }

        if (t >= 1 && showLabel(nd)) {
          const la =
            Math.min((now - (nd.born + TRAVEL)) / 420, 1) *
            labelAlphaFor(nd.risk);
          if (la > 0.01) {
            const right = Math.cos(nd.ang) >= 0;
            ctx.font = 10 * DPR + 'px Consolas, "Cascadia Mono", monospace';
            ctx.textAlign = right ? "left" : "right";
            ctx.fillStyle = rgba(c, la.toFixed(2));
            ctx.shadowColor = "rgba(2,5,9,0.9)";
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
          "rgba(255,93,110," + ((1 - t) * 0.85).toFixed(3) + ")";
        ring(pos.x, pos.y, t * 46 * DPR);
        ctx.lineWidth = 1 * DPR;
        ctx.strokeStyle =
          "rgba(255,93,110," + ((1 - t) * 0.4).toFixed(3) + ")";
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
        ctx.fillStyle = "rgba(255,93,110,0.75)";
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
        nodes = [];
        nodeSeq = Math.floor(Math.random() * 100);
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
        const GOLDEN = 2.399963229728653;
        const nd = {
          ang: (nodeSeq++ * GOLDEN) % (Math.PI * 2),
          rf: 0.58 + Math.random() * 0.34,
          born: performance.now() - (REDUCED ? TRAVEL : 0),
          risk: RGB[risk] ? risk : "Safe",
          name: String(name || "?"),
        };
        nodes.push(nd);
        window.__cureNodeCount = (window.__cureNodeCount || 0) + 1;
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

  const SOURCE_ICONS = {
    StartupFolder:
      '<svg viewBox="0 0 24 24"><path d="M14 2H7a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V7z"/><path d="M14 2v5h5"/></svg>',
    ScheduledTask:
      '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3.5 2"/></svg>',
    RegistryRun:
      '<svg viewBox="0 0 24 24"><circle cx="7.5" cy="15.5" r="4.5"/><path d="M11 12L21 2"/><path d="M17 6l3 3"/><path d="M14 9l2.5 2.5"/></svg>',
  };

  function reasonChipLabel(reason) {
    const lower = reason.toLowerCase();
    if (lower.includes("drop zone")) return ["Suspicious path", "red"];
    if (lower.includes("randomly generated")) return ["Random name", "red"];
    if (lower.includes("powershell")) return ["Hidden PowerShell", "red"];
    if (lower.includes("trusted install")) return ["Trusted location", "teal"];
    if (lower.includes("user profile folder")) return ["Profile exe", "amber"];
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
      done.textContent = "QUARANTINED";
      card.appendChild(done);
    } else if (rawSource === "RegistryRun") {
      const note = document.createElement("span");
      note.className = "manual-note";
      note.textContent = "MANUAL REMOVAL REQUIRED";
      note.title =
        entry.entry.location +
        " — registry values are not auto-disabled in this version";
      card.appendChild(note);
    } else {
      const btn = document.createElement("button");
      btn.className = "quarantine-btn";
      btn.textContent = "QUARANTINE";
      btn.addEventListener("click", async () => {
        btn.disabled = true;
        try {
          await invoke("quarantine_entry", {
            id: entry.entry.id,
            name: entry.entry.name,
            command: entry.entry.command,
          });
          btn.textContent = "QUARANTINED ✓";
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
    badge.textContent = trouble ? "⚠" : "✓";
    headline.textContent = trouble
      ? cleanedCount
        ? "THREATS NEUTRALIZED"
        : "REVIEW REQUIRED"
      : "ALL CLEAR";

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
