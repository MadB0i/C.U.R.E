/* ==========================================================================
   C.U.R.E · Detective Mode
   --------------------------------------------------------------------------
   "Your PC lost storage. C.U.R.E helps you discover where it went."

   This screen is a reading of a measurement the engine already made. It
   never scans on its own account, it never removes anything, and it never
   shows a figure the backend did not send.

   The board has three parts:

   * a storage map, where every arc is one measured category;
   * a ranked list of discoveries, each one an individual observation; and
   * a detail panel that shows the evidence behind the selected discovery.

   Two facts are kept strictly apart. *Storage usage* is what was found.
   *Reclaimable candidates* is what the engine is willing to offer for
   review. A large file is never called junk, and a protected category is
   never presented as a discovery to act on.
   ========================================================================== */
(function () {
  "use strict";

  const TAU = window.__TAURI__;
  const invoke = TAU && TAU.core ? TAU.core.invoke : null;
  const listen = TAU && TAU.event ? TAU.event.listen : null;
  const $ = (id) => document.getElementById(id);
  const NS = "http://www.w3.org/2000/svg";

  /* The same thresholds the engine uses. They are repeated here only so the
     board can label a slice; the authoritative classification is the one
     the backend sent, and this module never re-derives it. */
  const BAND_CLASS = {
    regenerable: "dt-band-regenerable",
    review: "dt-band-review",
    protected: "dt-band-protected",
    unknown: "dt-band-unknown",
  };

  const KIND_LABEL = {
    category: "Category",
    location: "Location",
    duplicate_set: "Verified duplicates",
    dormant_app: "Dormant application",
  };

  const RISK_LABEL = {
    safe: "Safe to review",
    review_required: "Review required",
    protected: "Protected",
    unknown: "Unknown",
  };

  const RISK_CLASS = {
    safe: "dt-risk-safe",
    review_required: "dt-risk-review",
    protected: "dt-risk-protected",
    unknown: "dt-risk-unknown",
  };

  const STEP_ORDER = ["scan", "discover", "investigate", "review", "verify", "closed"];

  const state = {
    scan: null,
    investigation: null,
    selected: null,
    filter: "all",
    focus: null,
    running: false,
    step: null,
    caseFile: null,
    demo: false,
    receipt: null,
  };

  let luma = null;

  function companion() {
    if (!luma && window.CureCompanion) {
      /* Claimed eagerly: a screen with its own persona must never be
         overwritten by a global event that arrives first. */
      luma = window.CureCompanion.onHost("detective");
    }
    return luma;
  }

  /* ---------------------------------------------------------------------
     helpers
     --------------------------------------------------------------------- */
  function fmtBytes(n) {
    if (n === null || n === undefined) return "—";
    if (n >= 1099511627776) return (n / 1099511627776).toFixed(2) + " TB";
    if (n >= 1073741824) return (n / 1073741824).toFixed(2) + " GB";
    if (n >= 1048576) return (n / 1048576).toFixed(1) + " MB";
    if (n >= 1024) return Math.round(n / 1024) + " KB";
    return n + " B";
  }

  function el(tag, cls, text) {
    const node = document.createElement(tag);
    if (cls) node.className = cls;
    if (text !== undefined && text !== null) node.textContent = text;
    return node;
  }

  function svgEl(name, attrs) {
    const node = document.createElementNS(NS, name);
    for (const key in attrs) {
      if (Object.prototype.hasOwnProperty.call(attrs, key)) {
        node.setAttribute(key, String(attrs[key]));
      }
    }
    return node;
  }

  function percent(value) {
    if (!value || value <= 0) return "—";
    if (value < 0.01) return "<1%";
    return Math.round(value * 100) + "%";
  }

  function clean(err, fallback) {
    const raw = String(err === null || err === undefined ? "" : err);
    if (!raw) return fallback;
    return raw.replace(/^error:\s*/i, "").slice(0, 220);
  }

  /* ---------------------------------------------------------------------
     the timeline
     ---------------------------------------------------------------------
     Every step is earned by something the application actually did. */
  function setStep(step) {
    state.step = step;
    const list = $("dt-timeline");
    if (!list) return;
    const index = STEP_ORDER.indexOf(step);
    Array.prototype.forEach.call(list.children, function (li, i) {
      const done = index >= 0 && i <= index;
      li.classList.toggle("is-done", done);
      li.classList.toggle("is-current", index >= 0 && i === index);
      if (index >= 0 && i === index) {
        li.setAttribute("aria-current", "step");
      } else {
        li.removeAttribute("aria-current");
      }
    });
  }

  /* ---------------------------------------------------------------------
     the storage map
     --------------------------------------------------------------------- */
  function arcPath(cx, cy, rOuter, rInner, a0, a1) {
    const large = a1 - a0 > Math.PI ? 1 : 0;
    const x0 = cx + Math.cos(a0) * rOuter;
    const y0 = cy + Math.sin(a0) * rOuter;
    const x1 = cx + Math.cos(a1) * rOuter;
    const y1 = cy + Math.sin(a1) * rOuter;
    const x2 = cx + Math.cos(a1) * rInner;
    const y2 = cy + Math.sin(a1) * rInner;
    const x3 = cx + Math.cos(a0) * rInner;
    const y3 = cy + Math.sin(a0) * rInner;
    return [
      "M", x0, y0,
      "A", rOuter, rOuter, 0, large, 1, x1, y1,
      "L", x2, y2,
      "A", rInner, rInner, 0, large, 0, x3, y3,
      "Z",
    ].join(" ");
  }

  function buildMap(investigation) {
    const host = $("dt-map");
    if (!host) return;
    host.replaceChildren();

    const slices = (investigation.map && investigation.map.slices) || [];
    const total = slices.reduce(function (sum, s) {
      return sum + (s.bytes_allocated || 0);
    }, 0);

    const svg = svgEl("svg", {
      class: "dt-map-svg",
      viewBox: "0 0 320 320",
      "aria-hidden": "true",
      focusable: "false",
    });

    const cx = 160;
    const cy = 160;
    const rOuter = 150;
    const rInner = 104;

    /* the untouched ring */
    svg.appendChild(
      svgEl("circle", {
        class: "dt-map-track",
        cx: cx,
        cy: cy,
        r: (rOuter + rInner) / 2,
        fill: "none",
        "stroke-width": rOuter - rInner,
      })
    );

    if (!slices.length || total === 0) {
      host.appendChild(svg);
      return;
    }

    let angle = -Math.PI / 2;
    slices.forEach(function (slice) {
      const value = slice.bytes_allocated || 0;
      if (value <= 0) return;
      const span = (value / total) * Math.PI * 2;
      const gap = slices.length > 1 ? 0.012 : 0;
      const path = svgEl("path", {
        class:
          "dt-arc " +
          (BAND_CLASS[slice.band] || "dt-band-review") +
          (state.focus && state.focus !== slice.key ? " is-dimmed" : ""),
        d: arcPath(cx, cy, rOuter, rInner, angle + gap, angle + span - gap),
        "data-key": slice.key,
      });
      svg.appendChild(path);
      angle += span;
    });

    host.appendChild(svg);

    /* The arcs are decorative; the legend beside them is the accessible
       version of the same figures and carries the same numbers. */
    host.setAttribute(
      "aria-label",
      "Storage map: " +
        slices
          .map(function (s) {
            return s.label + " " + s.bytes_allocated_label + ", " + percent(s.share);
          })
          .join("; ")
    );
  }

  function buildLegend(investigation) {
    const list = $("dt-legend");
    if (!list) return;
    list.replaceChildren();
    const slices = (investigation.map && investigation.map.slices) || [];
    slices.forEach(function (slice) {
      const li = el("li", "dt-legend-row");
      const button = el("button", "dt-legend-button");
      button.type = "button";
      button.setAttribute("aria-pressed", state.focus === slice.key ? "true" : "false");
      if (state.focus === slice.key) button.classList.add("is-on");

      const swatch = el("span", "dt-swatch " + (BAND_CLASS[slice.band] || ""));
      const name = el("span", "dt-legend-name", slice.label);
      const value = el("span", "dt-legend-value", slice.bytes_allocated_label);
      const share = el("span", "dt-legend-share", percent(slice.share));
      button.appendChild(swatch);
      button.appendChild(name);
      button.appendChild(value);
      button.appendChild(share);
      button.addEventListener("click", function () {
        state.focus = state.focus === slice.key ? null : slice.key;
        render();
      });
      li.appendChild(button);
      list.appendChild(li);
    });
  }

  function buildBands(investigation) {
    const host = $("dt-bands");
    if (!host) return;
    host.replaceChildren();
    const map = investigation.map || {};

    function bar(label, note, parts) {
      const total = parts.reduce(function (sum, p) {
        return sum + p.bytes;
      }, 0);
      const block = el("div", "dt-band");
      const head = el("div", "dt-band-head");
      head.appendChild(el("span", "dt-band-label", label));
      head.appendChild(el("span", "dt-band-total", fmtBytes(total)));
      block.appendChild(head);
      const track = el("div", "dt-band-track");
      parts.forEach(function (p) {
        const seg = el("span", "dt-band-seg " + p.cls);
        seg.style.flexGrow = String(Math.max(0.0001, p.bytes));
        seg.title = p.label + " · " + fmtBytes(p.bytes);
        track.appendChild(seg);
      });
      block.appendChild(track);
      const legend = el("div", "dt-band-legend");
      parts.forEach(function (p) {
        const item = el("span", "dt-band-key");
        const sw = el("span", "dt-swatch " + p.cls);
        item.appendChild(sw);
        item.appendChild(el("span", null, p.label + " " + fmtBytes(p.bytes)));
        legend.appendChild(item);
      });
      block.appendChild(legend);
      block.appendChild(el("p", "dim dt-band-note", note));
      return block;
    }

    host.appendChild(
      bar(
        "Removal candidates",
        "Storage the engine is willing to offer for review. Nothing is removed without an individual confirmation.",
        [
          {
            label: "Regenerable",
            bytes: map.safe_candidate_bytes || 0,
            cls: "dt-band-regenerable",
          },
          {
            label: "Needs review",
            bytes: map.careful_candidate_bytes || 0,
            cls: "dt-band-review",
          },
        ]
      )
    );
    host.appendChild(
      bar(
        "Off-limits",
        "Paths this engine never removes. Protected means it knows what it is; unknown means it does not.",
        [
          {
            label: "Protected",
            bytes: map.protected_bytes || 0,
            cls: "dt-band-protected",
          },
          {
            label: "Unknown",
            bytes: map.unknown_bytes || 0,
            cls: "dt-band-unknown",
          },
        ]
      )
    );
  }

  /* ---------------------------------------------------------------------
     the ranked discoveries
     --------------------------------------------------------------------- */
  function visibleLeads() {
    const leads = (state.investigation && state.investigation.leads) || [];
    return leads.filter(function (lead) {
      if (state.filter !== "all" && lead.kind !== state.filter) return false;
      if (state.focus && lead.class) {
        const key = lead.class.replace(/_/g, "_");
        const slice = ((state.investigation.map || {}).slices || []).find(function (s) {
          return s.key === lead.class;
        });
        if (slice && slice.key !== state.focus) return false;
      }
      return true;
    });
  }

  function buildLeads() {
    const list = $("dt-leads");
    const empty = $("dt-empty");
    if (!list) return;
    list.replaceChildren();
    const leads = visibleLeads();

    if (empty) empty.classList.toggle("hidden", leads.length > 0);

    leads.forEach(function (lead, index) {
      const li = el("li", "dt-lead");
      const button = el("button", "dt-lead-button");
      button.type = "button";
      if (state.selected === lead.id) button.classList.add("is-on");
      button.setAttribute("aria-pressed", state.selected === lead.id ? "true" : "false");

      const top = el("div", "dt-lead-top");
      const rank = el("span", "dt-lead-rank", String(index + 1).padStart(2, "0"));
      const kind = el("span", "dt-lead-kind", KIND_LABEL[lead.kind] || lead.kind);
      const size = el("span", "dt-lead-size", lead.bytes_allocated_label);
      top.appendChild(rank);
      top.appendChild(kind);
      top.appendChild(size);

      const title = el("div", "dt-lead-title", lead.title);

      const meta = el("div", "dt-lead-meta");
      const risk = el(
        "span",
        "dt-risk " + (RISK_CLASS[lead.risk] || ""),
        RISK_LABEL[lead.risk] || lead.risk
      );
      meta.appendChild(risk);
      if (lead.significance && lead.significance !== "ordinary") {
        const sig = el(
          "span",
          "dt-significance dt-significance-" + lead.significance,
          lead.significance === "major" ? "Major concentration" : "Notable"
        );
        meta.appendChild(sig);
      }
      meta.appendChild(el("span", "dt-lead-share", percent(lead.share)));

      const bar = el("div", "dt-lead-bar");
      const fill = el("span", "dt-lead-fill " + (RISK_CLASS[lead.risk] || ""));
      fill.style.width = Math.max(2, Math.min(100, (lead.share || 0) * 100)) + "%";
      bar.appendChild(fill);

      button.appendChild(top);
      button.appendChild(title);
      button.appendChild(meta);
      button.appendChild(bar);
      button.addEventListener("click", function () {
        selectLead(lead.id);
      });
      li.appendChild(button);
      list.appendChild(li);
    });
  }

  /* ---------------------------------------------------------------------
     the detail panel
     --------------------------------------------------------------------- */
  function fact(list, term, value, cls) {
    if (value === null || value === undefined || value === "") return;
    const row = el("div", "dt-fact");
    const dt = el("dt", "dt-fact-term", term);
    const dd = el("dd", "dt-fact-value" + (cls ? " " + cls : ""), value);
    row.appendChild(dt);
    row.appendChild(dd);
    list.appendChild(row);
  }

  function buildDetail() {
    const body = $("dt-detail-body");
    const empty = $("dt-detail-empty");
    const leads = (state.investigation && state.investigation.leads) || [];
    const lead = leads.find(function (l) {
      return l.id === state.selected;
    });

    if (!lead) {
      if (body) body.classList.add("hidden");
      if (empty) empty.classList.remove("hidden");
      return;
    }
    if (empty) empty.classList.add("hidden");
    if (!body) return;
    body.classList.remove("hidden");

    const kind = $("dt-detail-kind");
    if (kind) {
      kind.textContent = KIND_LABEL[lead.kind] || lead.kind;
      kind.className = "dt-detail-kind dt-kind-" + lead.kind;
    }
    const title = $("dt-detail-title");
    if (title) title.textContent = lead.title;

    const facts = $("dt-detail-facts");
    if (facts) {
      facts.replaceChildren();

      /* WHAT */
      fact(facts, "What", lead.title);
      /* WHERE — shown inside the application only. */
      fact(facts, "Where", lead.location || lead.location_label, "dt-fact-path");
      if (lead.location_label && lead.location_label !== lead.title) {
        fact(facts, "Measured in", lead.location_label);
      }
      /* WHY */
      fact(facts, "Why it matters", lead.significance_note);
      /* ORIGIN */
      const originConfidence =
        lead.origin && lead.origin.confidence
          ? lead.origin.confidence.replace(/_/g, " ")
          : "unknown";
      fact(
        facts,
        "Origin",
        (lead.origin ? lead.origin.label : "—") + " · " + originConfidence
      );
      if (lead.origin && lead.origin.evidence) {
        fact(facts, "Origin evidence", lead.origin.evidence);
      }
      /* SIZE */
      fact(
        facts,
        "Size on disk",
        lead.bytes_allocated_label +
          (lead.bytes_logical !== lead.bytes_allocated
            ? " (" + lead.bytes_logical_label + " of apparent length)"
            : "")
      );
      if (lead.item_count) {
        fact(facts, "Items", String(lead.item_count));
      }
      /* RISK */
      fact(facts, "Risk", (RISK_LABEL[lead.risk] || lead.risk) + " — " + lead.risk_note);
      /* ACTION */
      fact(facts, "Action", lead.action_label);
      /* CONSEQUENCE */
      fact(facts, "If removed", lead.consequence);

      if (lead.scope_note) {
        fact(facts, "Scope", lead.scope_note);
      }
      if (lead.evidence && lead.evidence.length) {
        const row = el("div", "dt-fact dt-fact-wide");
        row.appendChild(el("dt", "dt-fact-term", "Evidence"));
        const dd = el("dd", "dt-fact-value");
        const ul = el("ul", "dt-evidence");
        lead.evidence.forEach(function (item) {
          ul.appendChild(el("li", null, item));
        });
        dd.appendChild(ul);
        row.appendChild(dd);
        facts.appendChild(row);
      }
    }

    const actions = $("dt-detail-actions");
    if (actions) {
      actions.replaceChildren();
      if (lead.action_kind === "review_and_queue") {
        const open = el("button", "btn btn-ghost", "Open in Storage Workstation");
        open.type = "button";
        open.addEventListener("click", function () {
          gotoWorkstation(lead);
        });
        actions.appendChild(open);
      } else if (lead.action_kind === "not_offered") {
        actions.appendChild(
          el(
            "p",
            "dim dt-no-action",
            "This engine does not offer to remove these, and there is no control here that would."
          )
        );
      }
    }
  }

  /* ---------------------------------------------------------------------
     selection
     --------------------------------------------------------------------- */
  function selectLead(id) {
    state.selected = id;
    setStep("investigate");
    const leads = (state.investigation && state.investigation.leads) || [];
    const lead = leads.find(function (l) {
      return l.id === id;
    });
    if (lead && companion()) {
      if (lead.action_kind === "not_offered") {
        companion().thinking("This one is off-limits. Here is why.");
      } else {
        companion().curious("Looking at " + lead.title);
      }
    }
    render();
  }

  function gotoWorkstation(lead) {
    setStep("review");
    if (companion()) companion().review(1, "Ready for a decision");
    const ws = window.CureStorageWorkstation;
    /* Hand over the measurement the board is reading. The workstation must
       not rescan behind the operator's back, and the two screens must not
       describe different files. */
    if (ws && ws.adopt && state.scan) {
      ws.adopt(state.scan);
    }
    if (ws && ws.focusClass && lead.class) {
      ws.focusClass(lead.class);
    }
    const nav = document.querySelector('.nav-item[data-view="cleanup-view"]');
    if (nav) nav.click();
  }

  /* ---------------------------------------------------------------------
     rendering
     --------------------------------------------------------------------- */
  function render() {
    const inv = state.investigation;
    const subline = $("dt-subline");
    const total = $("dt-map-total");

    if (!inv) {
      if (subline) {
        subline.textContent = "Nothing is measured until you start an investigation.";
      }
      if (total) total.textContent = "—";
      buildLeads();
      buildDetail();
      return;
    }

    const map = inv.map || {};
    if (total) total.textContent = map.measured_bytes_label || "—";
    if (subline) {
      const head = inv.headline || {};
      const candidateBytes = head.candidate_bytes || map.candidate_bytes || 0;
      const candidateLabel =
        head.candidate_label || map.candidate_label || "—";
      subline.textContent =
        (map.measured_bytes_label || head.measured_label || "—") +
        " measured across " +
        ((inv.evidence && inv.evidence.files_measured) || 0) +
        " files. " +
        (candidateBytes
          ? candidateLabel + " can be reviewed for removal."
          : "Nothing in this measurement can be offered for removal.") +
        (map.certainty && map.certainty !== "measured"
          ? " Some locations could not be read, so this is a floor, not a total."
          : "");
    }

    buildMap(inv);
    buildLegend(inv);
    buildBands(inv);
    buildLeads();
    buildDetail();

    const mapHint = $("dt-map-hint");
    if (mapHint && state.focus) {
      const slice = (map.slices || []).find(function (s) {
        return s.key === state.focus;
      });
      if (slice) {
        mapHint.textContent =
          "Focused on " +
          slice.label +
          " — " +
          slice.bytes_allocated_label +
          ", " +
          percent(slice.share) +
          " of what was measured. Select it again to clear the focus.";
      }
    } else if (mapHint) {
      mapHint.textContent =
        "Each arc is one measured category, sized by the bytes it occupies on the volume. Select one to focus the discoveries beside it.";
    }
  }

  /* ---------------------------------------------------------------------
     the investigation itself
     --------------------------------------------------------------------- */
  async function startInvestigation() {
    if (state.running) return;
    state.running = true;
    const button = $("dt-investigate");
    const rescan = $("dt-rescan");
    if (button) {
      button.disabled = true;
      button.textContent = "Investigating…";
    }
    if (rescan) rescan.classList.add("hidden");
    setStep("scan");
    if (companion()) companion().scan("Measuring storage");
    if (window.CureTopbar && window.CureTopbar.storageScanStarted) {
      window.CureTopbar.storageScanStarted();
    }

    try {
      const scan = await invoke("storage_scan", {
        depth: "quick",
        roots: [],
        findDuplicates: true,
        findAbandoned: true,
      });
      adopt(scan);
    } catch (err) {
      state.running = false;
      if (window.CureTopbar && window.CureTopbar.storageScanFinished) {
        window.CureTopbar.storageScanFinished();
      }
      if (button) {
        button.disabled = false;
        button.textContent = "Start investigation";
      }
      setStep("scan");
      if (companion()) companion().failed("The measurement could not finish");
      const subline = $("dt-subline");
      if (subline) {
        subline.textContent =
          "The measurement could not finish: " + clean(err, "unknown error") + ".";
      }
      return;
    }
    state.running = false;
    if (button) {
      button.disabled = false;
      button.textContent = "Start investigation";
    }
  }

  /* Adopt a measurement. The workstation and the detective board read the
     same engine result, so neither can show a figure the other contradicts. */
  function adopt(scan) {
    if (!scan) return;
    state.scan = scan;
    state.investigation = scan.investigation || null;
    state.selected = null;
    state.focus = null;
    state.caseFile = null;
    const rescan = $("dt-rescan");
    if (rescan) rescan.classList.remove("hidden");
    const invest = $("dt-investigate");
    if (invest) invest.textContent = "Start investigation";

    if (!state.investigation) {
      setStep("scan");
      if (companion()) companion().idle("No investigation data");
      render();
      return;
    }

    if (window.CureTopbar && window.CureTopbar.storageScan) {
      const map = state.investigation.map || {};
      window.CureTopbar.storageScan({
        at: (scan.finished_unix || 0) * 1000,
        bytes: map.measured_bytes || 0,
        files: (scan.coverage && scan.coverage.files_seen) || 0,
        incomplete: map.certainty ? map.certainty !== "measured" : false,
      });
    }

    setStep("discover");
    const inv = state.investigation;
    if (inv.major_lead) {
      if (companion()) {
        companion().major("That is the biggest concentration here");
      }
      revealMajor(inv.major_lead);
    } else if (inv.notable_leads && inv.notable_leads.length) {
      if (companion()) {
        companion().notice("New clue discovered");
      }
      revealMajor(inv.notable_leads[0]);
    } else if (inv.leads && inv.leads.length) {
      if (companion()) {
        companion().investigating("Looking over the measurement");
      }
    } else {
      if (companion()) {
        companion().idle("Nothing to reclaim here");
      }
    }
    render();
  }

  /* The reveal moment. The focus moves to the relevant part of the map and
     the discovery card arrives on its own. Nothing here suggests that the
     discovery is dangerous or disposable: it is a concentration of storage,
     and the risk is stated separately, next to it. */
  function revealMajor(leadId) {
    const inv = state.investigation;
    if (!inv) return;
    const lead = (inv.leads || []).find(function (l) {
      return l.id === leadId;
    });
    if (!lead) return;
    if (lead.class) {
      const slice = (inv.map.slices || []).find(function (s) {
        return s.key === lead.class;
      });
      if (slice) state.focus = slice.key;
    }
    window.setTimeout(function () {
      state.selected = lead.id;
      render();
    }, 420);
  }

  /* ---------------------------------------------------------------------
     the case file
     --------------------------------------------------------------------- */
  function caseFileRequest() {
    const summary = $("dt-case-summary");
    const opt = $("dt-case-summary-opt");
    const privacy = $("dt-case-privacy");
    return {
      summary: summary ? summary.value : "",
      includeSummary: !!(opt && opt.checked),
      privacy: privacy ? privacy.value : "minimal",
    };
  }

  function setCaseStatus(text, isError) {
    const status = $("dt-case-status");
    if (!status) return;
    status.textContent = text;
    status.classList.toggle("is-error", !!isError);
  }

  async function buildCaseFile() {
    const req = caseFileRequest();
    try {
      if (invoke) {
        return await invoke("storage_case_file", req);
      }
      if (window.CureMockCaseFile) {
        return window.CureMockCaseFile(req);
      }
      throw new Error("This build cannot produce a case file.");
    } catch (err) {
      setCaseStatus(clean(err, "The case file could not be built."), true);
      return null;
    }
  }

  function showCaseFile(caseFile) {
    const stage = $("dt-case-preview-stage");
    if (!stage) return null;
    state.caseFile = caseFile;
    stage.replaceChildren();
    if (!window.CureCaseFile) {
      setCaseStatus("The renderer is not available in this build.", true);
      return null;
    }
    const rendered = window.CureCaseFile.render(caseFile);
    const img = document.createElement("img");
    img.className = "dt-case-image";
    img.src = rendered.dataUrl;
    img.alt =
      "Case file preview: " +
      caseFile.title +
      ", " +
      caseFile.status_label +
      ", " +
      caseFile.measured_label +
      " measured.";
    stage.appendChild(img);
    const note = el(
      "p",
      "dim dt-case-note",
      caseFile.demo
        ? "Synthetic demo only. Nothing on this machine was measured, written or uploaded."
        : "Preview only. Nothing has been written to disk and nothing has been uploaded."
    );
    stage.appendChild(note);
    return rendered;
  }

  async function previewCaseFile() {
    const caseFile = await buildCaseFile();
    if (!caseFile) return;
    const rendered = showCaseFile(caseFile);
    if (!rendered) return;
    setCaseStatus(
      caseFile.demo
        ? "Synthetic case file preview. Nothing was read from this machine."
        : "Preview ready. Export writes a PNG to this machine."
    );
    return rendered;
  }

  async function exportCaseFile() {
    const caseFile = await buildCaseFile();
    if (!caseFile) return;
    const rendered = showCaseFile(caseFile);
    if (!rendered) return;

    const status = caseFile.status_label;

    /* The public demo never writes a file and never calls the save command.
       It shows the same preview the real build shows, with the synthetic
       marker the backend put on the model. */
    if (caseFile.demo) {
      setCaseStatus(
        "Synthetic demo case file. Nothing was measured, written or uploaded."
      );
      if (companion()) companion().curious("A synthetic case file, ready to read");
      return;
    }

    setCaseStatus("Exporting the case file…");
    const base64 = rendered.dataUrl.split(",")[1] || "";
    try {
      if (invoke) {
        const path = await invoke("storage_save_case_file", {
          name: "cure-case-" + String(caseFile.case_id || "case"),
          dataBase64: base64,
        });
        setCaseStatus("Case file written to " + path + " · " + status);
      } else {
        /* No backend: the image is offered for download instead of being
           written anywhere. */
        const link = document.createElement("a");
        link.href = rendered.dataUrl;
        link.download = "cure-case-" + (caseFile.case_id || "case") + ".png";
        document.body.appendChild(link);
        link.click();
        link.remove();
        setCaseStatus("Case file prepared. It was not uploaded anywhere.");
      }
      /* A case file is only "case closed" when the backend verified the
         recovery. Investigation complete and review pending are not the
         same claim. */
      if (caseFile.status === "recovery_verified") {
        setStep("closed");
        if (companion()) companion().cleanup("success", "Recovery verified");
      } else {
        if (companion()) companion().curious("Case file ready");
      }
    } catch (err) {
      setCaseStatus(clean(err, "The case file could not be written."), true);
    }
  }

  /* ---------------------------------------------------------------------
     wiring
     --------------------------------------------------------------------- */
  function wire() {
    /* The demo bundle forces the mock backend. Reading that flag is enough
       to know this build must never write a case file or claim recovery. */
    state.demo = !!window.__CURE_FORCE_MOCK;

    let lastPhase = null;
    if (listen) {
      listen("storage-progress", function (event) {
        if (!state.running) return;
        const p = event && event.payload;
        if (!p) return;
        if (p.phase === lastPhase) return;
        lastPhase = p.phase;
        if (companion()) {
          if (p.phase === "hashing") {
            companion().thinking("Verifying duplicate copies byte for byte");
          } else if (p.phase === "walking") {
            companion().scan("scan", "Measuring storage");
          }
        }
      });
    }

    const investigate = $("dt-investigate");
    if (investigate) investigate.addEventListener("click", startInvestigation);
    const rescan = $("dt-rescan");
    if (rescan) rescan.addEventListener("click", startInvestigation);

    const filters = $("dt-lead-filters");
    if (filters) {
      filters.addEventListener("click", function (ev) {
        const button = ev.target.closest("[data-filter]");
        if (!button) return;
        state.filter = button.getAttribute("data-filter") || "all";
        Array.prototype.forEach.call(filters.children, function (chip) {
          const on = chip === button;
          chip.classList.toggle("is-on", on);
          chip.setAttribute("aria-pressed", on ? "true" : "false");
        });
        render();
      });
    }

    const preview = $("dt-case-preview");
    if (preview) preview.addEventListener("click", previewCaseFile);
    const exportButton = $("dt-case-export");
    if (exportButton) exportButton.addEventListener("click", exportCaseFile);

    /* A verified outcome is the only thing that closes a case, and only the
       receipt can say that space was really returned to the volume. */
    if (listen) {
      listen("storage-receipt", function (event) {
        const receipt = (event && event.payload) || null;
        if (!receipt) return;
        state.receipt = receipt;
        const gain = (receipt.volumes || []).reduce(function (sum, v) {
          return sum + (typeof v.delta === "number" && v.delta > 0 ? v.delta : 0);
        }, 0);
        if (gain > 0) {
          setStep("closed");
          if (companion()) companion().cleanup("success", "Space recovery verified");
        } else {
          setStep("verify");
          if (companion()) {
            companion().thinking("Files removed; free space not observed");
          }
        }
      });
    }

    window.addEventListener("cure-storage-review", function () {
      setStep("review");
    });

    render();
  }

  /* ---------------------------------------------------------------------
     public API
     --------------------------------------------------------------------- */
  window.CureDetective = {
    adopt: adopt,
    start: startInvestigation,
    render: render,
    state: state,
    selectLead: selectLead,
    isRunning: function () {
      return state.running;
    },
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", wire);
  } else {
    wire();
  }
})();
