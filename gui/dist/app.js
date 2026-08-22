(function () {
  "use strict";

  const TAU = window.__TAURI__;
  if (!TAU || !TAU.core || !TAU.event) {
    document.getElementById("fallback").classList.remove("hidden");
    return;
  }
  document.getElementById("app").classList.remove("hidden");

  const invoke = TAU.core.invoke;
  const listen = TAU.event.listen;

  const statusLine = document.getElementById("status-line");
  const logList = document.getElementById("log");
  const scanView = document.getElementById("scan-view");
  const resultsView = document.getElementById("results-view");

  function setStatus(text, mode) {
    statusLine.textContent = text;
    statusLine.className = "status" + (mode ? " " + mode : "");
  }

  function appendLog(stage, message) {
    const li = document.createElement("li");
    const tag = document.createElement("b");
    tag.textContent = "[" + stage + "]";
    li.appendChild(tag);
    li.appendChild(document.createTextNode(message));
    logList.appendChild(li);
    logList.scrollTop = logList.scrollHeight;
    while (logList.children.length > 200) {
      logList.removeChild(logList.firstChild);
    }
  }

  const radar = (() => {
    const canvas = document.getElementById("radar");
    const ctx = canvas.getContext("2d");
    const DPR = Math.min(window.devicePixelRatio || 1, 2);
    let sweep = 0;
    let rafId = null;
    let pings = [];

    function size() {
      const rect = canvas.getBoundingClientRect();
      canvas.width = rect.width * DPR;
      canvas.height = rect.height * DPR;
    }
    window.addEventListener("resize", size);
    size();

    function drawRing(cx, cy, r) {
      ctx.beginPath();
      ctx.arc(cx, cy, r, 0, Math.PI * 2);
      ctx.stroke();
    }

    function frame() {
      const w = canvas.width, h = canvas.height;
      const cx = w / 2, cy = h / 2;
      const R = Math.min(w, h) / 2 - 6 * DPR;

      ctx.clearRect(0, 0, w, h);

      ctx.strokeStyle = "rgba(80, 220, 170, 0.22)";
      ctx.lineWidth = DPR;
      drawRing(cx, cy, R);
      [0.33, 0.66].forEach((f) => drawRing(cx, cy, R * f));

      ctx.beginPath();
      ctx.moveTo(cx - R, cy); ctx.lineTo(cx + R, cy);
      ctx.moveTo(cx, cy - R); ctx.lineTo(cx, cy + R);
      ctx.stroke();

      sweep += 0.045;
      if (typeof ctx.createConicGradient === "function") {
        const trail = ctx.createConicGradient(sweep, cx, cy);
        trail.addColorStop(0.0, "rgba(80, 220, 170, 0.30)");
        trail.addColorStop(0.14, "rgba(80, 220, 170, 0)");
        trail.addColorStop(1.0, "rgba(80, 220, 170, 0)");
        ctx.fillStyle = trail;
        ctx.beginPath();
        ctx.arc(cx, cy, R, 0, Math.PI * 2);
        ctx.fill();
      }

      ctx.strokeStyle = "#50dcaa";
      ctx.lineWidth = 2 * DPR;
      ctx.shadowColor = "rgba(80, 220, 170, 0.8)";
      ctx.shadowBlur = 10 * DPR;
      ctx.beginPath();
      ctx.moveTo(cx, cy);
      ctx.lineTo(cx + Math.cos(sweep) * R, cy + Math.sin(sweep) * R);
      ctx.stroke();
      ctx.shadowBlur = 0;

      pings = pings.filter((p) => performance.now() - p.born < 900);
      for (const p of pings) {
        const age = (performance.now() - p.born) / 900;
        const pr = 4 * DPR + age * 26 * DPR;
        ctx.strokeStyle = "rgba(255, 85, 102, " + (1 - age).toFixed(3) + ")";
        ctx.lineWidth = 2 * DPR;
        ctx.beginPath();
        ctx.arc(p.x, p.y, pr, 0, Math.PI * 2);
        ctx.stroke();
        ctx.fillStyle = "rgba(255, 85, 102, " + ((1 - age) * 0.8).toFixed(3) + ")";
        ctx.beginPath();
        ctx.arc(p.x, p.y, 3 * DPR, 0, Math.PI * 2);
        ctx.fill();
      }

      rafId = requestAnimationFrame(frame);
    }

    return {
      start() { size(); pings = []; if (!rafId) frame(); },
      stop() {
        if (rafId) cancelAnimationFrame(rafId);
        rafId = null;
        ctx.clearRect(0, 0, canvas.width, canvas.height);
      },
      ping() {
        const cx = canvas.width / 2, cy = canvas.height / 2;
        const maxR = (Math.min(canvas.width, canvas.height) / 2 - 12 * DPR) * 0.8;
        const ang = Math.random() * Math.PI * 2;
        const rad = Math.sqrt(Math.random()) * maxR;
        pings.push({
          x: cx + Math.cos(ang) * rad,
          y: cy + Math.sin(ang) * rad,
          born: performance.now(),
        });
      },
    };
  })();

  function countUp(el, target) {
    const started = performance.now();
    function tick(now) {
      const t = Math.min((now - started) / 600, 1);
      el.textContent = String(Math.round(target * t));
      if (t < 1) requestAnimationFrame(tick);
    }
    requestAnimationFrame(tick);
  }

  function entryRow(entry, cleaned) {
    const li = document.createElement("li");
    const score = document.createElement("span");
    score.className = "score";
    score.textContent = String(entry.score);
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = entry.entry.name;
    name.title = entry.entry.command;
    const tag = document.createElement("span");
    tag.className = "tag";
    tag.textContent = entry.entry.source.tag || entry.entry.source;
    li.append(score, name, tag);

    if (cleaned) {
      const done = document.createElement("span");
      done.className = "manual-note";
      done.textContent = "QUARANTINED";
      li.appendChild(done);
    } else if ((entry.entry.source.tag || entry.entry.source) === "registry-run") {
      const note = document.createElement("span");
      note.className = "manual-note";
      note.textContent = "MANUAL REMOVAL REQUIRED";
      note.title =
        entry.entry.location +
        " — registry values are not auto-disabled in this version";
      li.appendChild(note);
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
          setStatus(String(err), "error");
        }
      });
      li.appendChild(btn);
    }
    return li;
  }

  function renderResults(summary) {
    scanView.classList.add("hidden");
    resultsView.classList.remove("hidden");
    setStatus("scan complete", "settled");

    const badge = document.getElementById("badge");
    const headline = document.getElementById("headline");
    const trouble = summary.high_risk_cleaned.length + summary.suspicious_for_review.length;
    badge.className = "badge " + (trouble ? "warn" : "clean");
    badge.textContent = trouble ? "⚠" : "✓";
    headline.textContent = trouble
      ? summary.high_risk_cleaned.length
        ? "THREATS NEUTRALIZED"
        : "REVIEW REQUIRED"
      : "ALL CLEAR";

    countUp(document.getElementById("stat-cleaned"), summary.high_risk_cleaned.length);
    countUp(document.getElementById("stat-review"), summary.suspicious_for_review.length);
    countUp(document.getElementById("stat-safe"), summary.safe);

    const cleanedBlock = document.getElementById("cleaned-block");
    const cleanedList = document.getElementById("cleaned-list");
    cleanedList.innerHTML = "";
    cleanedBlock.classList.toggle("hidden", summary.high_risk_cleaned.length === 0);
    for (const entry of summary.high_risk_cleaned) {
      cleanedList.appendChild(entryRow(entry, true));
    }

    const reviewBlock = document.getElementById("review-block");
    const reviewList = document.getElementById("review-list");
    reviewList.innerHTML = "";
    reviewBlock.classList.toggle("hidden", summary.suspicious_for_review.length === 0);
    for (const entry of summary.suspicious_for_review) {
      reviewList.appendChild(entryRow(entry, false));
    }
  }

  async function runScan() {
    resultsView.classList.add("hidden");
    scanView.classList.remove("hidden");
    logList.innerHTML = "";
    setStatus("sweeping persistence locations…", "");
    radar.start();
    try {
      const summary = await invoke("run_auto_scan");
      appendLog("done", summary.total + " entries processed — scan finished");
      radar.stop();
      renderResults(summary);
    } catch (err) {
      radar.stop();
      setStatus(String(err), "error");
      appendLog("error", String(err));
    }
  }

  listen("scan-progress", (event) => {
    const payload = event.payload;
    setStatus(payload.message, "");
    appendLog(payload.stage, payload.message);
    if (payload.stage === "cleaning") {
      radar.ping();
    }
  });

  document.getElementById("rescan-btn").addEventListener("click", runScan);
  runScan();
})();
