/* Deterministic topology. Only backend events move the sweep or add evidence. */
(function () {
  "use strict";
  const sources = [
    ["registry", "Registry", "Registry autoruns"],
    ["startup", "Startup", null],
    ["tasks", "Scheduled tasks", "Scheduled tasks"],
    ["services", "Services", "Services (auto-start)"],
    ["wmi", "WMI", "WMI subscriptions"],
    ["ifeo", "IFEO / AppInit", null],
    ["com", "COM registrations", null],
    ["processes", "Processes", null],
    ["ransom", "Ransom indicators", null],
  ];
  const aliases = {
    "startup-common": "startup",
    "process-scan": "processes",
    process: "processes",
    "ransom-scan": "ransom",
    "ransom-detect": "ransom",
  };
  const itemSource = {
    "registry-run": "registry",
    "startup-folder": "startup",
    "scheduled-task": "tasks",
    "windows-service": "services",
    "wmi-subscription": "wmi",
    "ifeo-debugger": "ifeo",
    "appinit-dlls": "ifeo",
    "com-hijack": "com",
  };
  const svg = document.getElementById("sweep-svg");
  const list = document.getElementById("sweep-sources");
  const ns = "http://www.w3.org/2000/svg";
  let active = null;
  let items = 0;
  let states = new Map();
  let counts = new Map();
  let coverage = new Map();
  let cursor = null;
  function reportedState(row) {
    const raw =
      typeof row.state === "string"
        ? row.state
        : Object.keys(row.state || {})[0];
    const checked = raw === "Checked" || raw === "Available";
    return {
      tone: checked ? "checked" : raw === "CheckFailed" ? "bad" : "warn",
      text: checked
        ? "Checked"
        : raw === "AccessDenied"
          ? "Access denied"
          : raw === "CheckFailed"
            ? "Check failed"
            : raw === "Partial"
              ? "Partial"
              : raw === "Unavailable"
                ? "Unavailable"
                : "Not checked",
    };
  }
  function el(tag, attrs, text) {
    const node = document.createElementNS(ns, tag);
    Object.entries(attrs).forEach(([k, v]) => node.setAttribute(k, v));
    if (text !== undefined) node.textContent = text;
    svg.append(node);
    return node;
  }
  function paint() {
    svg
      .querySelectorAll(":scope > :not(title):not(desc):not(.sweep-cursor)")
      .forEach((n) => n.remove());
    [65, 110, 145].forEach((r) =>
      el("circle", { cx: 240, cy: 160, r, class: "sweep-line" }),
    );
    el("path", {
      d: "M240 12V82M240 238V308M92 160H155M325 160H388",
      class: "sweep-line",
    });
    list.replaceChildren();
    sources.forEach(([id, label], i) => {
      const angle = -Math.PI / 2 + (i * Math.PI * 2) / sources.length;
      const x = 240 + Math.cos(angle) * 112,
        y = 160 + Math.sin(angle) * 112;
      const state = states.get(id) || { tone: "", text: "Pending" };
      el("line", { x1: 240, y1: 160, x2: x, y2: y, class: "sweep-spoke" });
      el("circle", { cx: x, cy: y, r: 4, class: "sweep-node " + state.tone });
      const lx = 240 + Math.cos(angle) * 145,
        ly = 160 + Math.sin(angle) * 145;
      el(
        "text",
        {
          x: lx,
          y: ly + 4,
          "text-anchor":
            Math.cos(angle) < -0.2
              ? "end"
              : Math.cos(angle) > 0.2
                ? "start"
                : "middle",
          class: "sweep-label",
        },
        String(i + 1).padStart(2, "0"),
      );
      if (counts.get(id)) {
        el(
          "text",
          { x: x + 8, y: y - 8, class: "sweep-meta" },
          "+" + counts.get(id),
        );
      }
      if (id === active) {
        if (!cursor)
          cursor = el("path", {
            d: "M240 90V20",
            class: "sweep-cursor",
            style:
              "transform:rotate(" + ((angle * 180) / Math.PI + 90) + "deg)",
          });
        cursor.style.transform =
          "rotate(" + ((angle * 180) / Math.PI + 90) + "deg)";
      }
      const li = document.createElement("li");
      li.className = state.tone;
      li.dataset.source = id;
      const num = document.createElement("span");
      num.className = "source-number";
      num.textContent = String(i + 1).padStart(2, "0");
      const name = document.createElement("span");
      name.textContent = label;
      const status = document.createElement("span");
      status.className = "source-state";
      status.textContent = state.text;
      li.append(num, name, status);
      list.append(li);
    });
    if (cursor) cursor.style.display = active ? "" : "none";
    // The opaque core clears intersecting spokes; no decorative live traffic.
    el("rect", {
      x: 177,
      y: 128,
      width: 126,
      height: 64,
      rx: 6,
      fill: "var(--surface)",
      stroke: "var(--line)",
    });
    el(
      "text",
      { x: 240, y: 155, "text-anchor": "middle", class: "sweep-core" },
      "C.U.R.E",
    );
    el(
      "text",
      { x: 240, y: 176, "text-anchor": "middle", class: "sweep-meta" },
      "EVIDENCE SWEEP",
    );
    document.getElementById("sweep-items").textContent =
      items + " entries inspected";
  }
  function completeActive() {
    if (active)
      states.set(
        active,
        coverage.get(active) || { tone: "", text: "Collected" },
      );
  }
  window.CureSweep = {
    start() {
      active = null;
      items = 0;
      states = new Map();
      counts = new Map();
      coverage = new Map();
      paint();
      document.getElementById("sweep-current").textContent =
        "Awaiting collection";
    },
    progress(p) {
      if (p.stage === "source-state") {
        const source = sources.find((s) => s[2] === p.area);
        if (source) {
          const state = reportedState(p);
          coverage.set(source[0], state);
          states.set(source[0], state);
        }
        paint();
        return;
      }
      const stage = aliases[p.stage] || p.stage;
      if (sources.some((s) => s[0] === stage)) {
        if (active !== stage) completeActive();
        active = stage;
        states.set(stage, { tone: "active", text: "Inspecting" });
      }
      if (["scoring", "item-scan", "done"].includes(p.stage)) {
        completeActive();
        active = null;
      }
      if (p.stage === "item-scanned") {
        completeActive();
        active = itemSource[p.source] || null;
        if (active)
          states.set(active, { tone: "active", text: "Inspecting entry" });
        items++;
        if (p.risk !== "Safe") {
          const id = itemSource[p.source];
          if (id) counts.set(id, (counts.get(id) || 0) + 1);
        }
      }
      if (p.stage === "process-flagged")
        counts.set("processes", (counts.get("processes") || 0) + 1);
      if (p.stage === "ransom-found")
        counts.set("ransom", (counts.get("ransom") || 0) + 1);
      document.getElementById("sweep-current").textContent =
        p.message ||
        (p.stage === "item-scanned"
          ? "Inspecting: " + p.name
          : "Collecting evidence");
      paint();
    },
    finish(summary) {
      completeActive();
      active = null;
      sources.forEach(([id, , area]) => {
        const row = (summary.source_states || []).find((r) => r.area === area);
        if (!row) {
          if (states.has(id))
            states.set(id, { tone: "", text: "Coverage unreported" });
          return;
        }
        const raw =
          typeof row.state === "string"
            ? row.state
            : Object.keys(row.state || {})[0];
        const checked = raw === "Checked" || raw === "Available";
        states.set(id, {
          tone: checked ? "checked" : raw === "CheckFailed" ? "bad" : "warn",
          text: checked
            ? "Checked"
            : raw === "AccessDenied"
              ? "Access denied"
              : raw === "CheckFailed"
                ? "Check failed"
                : raw === "Partial"
                  ? "Partial"
                  : raw === "Unavailable"
                    ? "Unavailable"
                    : "Not checked",
        });
      });
      document.getElementById("sweep-current").textContent = "Collection ended";
      paint();
    },
    fail() {
      if (active) states.set(active, { tone: "bad", text: "Interrupted" });
      active = null;
      document.getElementById("sweep-current").textContent =
        "Collection interrupted — retry available";
      paint();
    },
  };
  paint();
})();
