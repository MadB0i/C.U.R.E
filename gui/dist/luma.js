/* ==========================================================================
   LUMA — C.U.R.E utility node.

   LUMA is a status indicator, not a character: a compact floating graphite
   node whose ring reports what the console is doing. There is no face, no
   figure and no artwork to supply — the whole node is a few circles, so it
   is authored inline and costs nothing to load.

   Presentation only. This module never invokes a command and never invents a
   value it was not handed: every caption it shows was passed in by the caller.
   ========================================================================== */
(function () {
  "use strict";

  const stages = Array.prototype.slice.call(document.querySelectorAll("[data-companion]"));
  const reduced = matchMedia("(prefers-reduced-motion: reduce)");
  let paused = false;
  try {
    paused = localStorage.getItem("cure-companion-paused") === "true";
  } catch (_) {
    /* preference storage is optional */
  }

  /* Twelve graduations around the bezel, every third one longer. Reads as an
     instrument scale rather than decoration. */
  function ticks() {
    let out = "";
    for (let i = 0; i < 12; i++) {
      const a = (i * 30 * Math.PI) / 180;
      const c = Math.cos(a);
      const s = Math.sin(a);
      const inner = 44.5;
      const outer = i % 3 === 0 ? 49 : 47.4;
      out +=
        '<line x1="' + (50 + c * inner).toFixed(2) +
        '" y1="' + (50 + s * inner).toFixed(2) +
        '" x2="' + (50 + c * outer).toFixed(2) +
        '" y2="' + (50 + s * outer).toFixed(2) + '"/>';
    }
    return out;
  }

  /* pathLength normalises every ring to 100 units so the dash figures below
     read as percentages and cannot drift when a radius changes. */
  const NODE =
    '<svg class="companion-art luma-node" viewBox="0 0 100 100" ' +
    'aria-hidden="true" focusable="false">' +
    '<g class="ln-ticks">' + ticks() + "</g>" +
    '<circle class="ln-ring" cx="50" cy="50" r="40" pathLength="100"/>' +
    '<circle class="ln-sweep" cx="50" cy="50" r="40" pathLength="100"/>' +
    '<circle class="ln-segs" cx="50" cy="50" r="33" pathLength="100"/>' +
    '<circle class="ln-body" cx="50" cy="50" r="25"/>' +
    '<path class="ln-bevel" d="M28.35 37.5a25 25 0 0 0 34.3 0"/>' +
    '<path class="ln-rim" d="M71.65 62.5a25 25 0 0 1-43.3 0"/>' +
    '<circle class="ln-core" cx="50" cy="50" r="3.4"/>' +
    "</svg>";

  const KIND_LABEL = {
    idle: "Luma · standby",
    scan: "Luma · node",
    cleanup: "Luma · node",
    review: "Luma · node",
    success: "Luma · node",
    seal: "Luma · node",
  };

  /* Deterministic rotation: consecutive sessions advance through the action
     sets rather than replaying one loop, and captures stay reproducible. */
  const SEQUENCE = {
    scan: ["scan-console", "scan-scanner", "scan-lens"],
    cleanup: ["clean-sweep", "clean-sort", "clean-recycle"],
  };
  const cursors = { scan: 0, cleanup: 0 };
  const currentVariant = { scan: null, cleanup: null };

  function nextVariant(kind) {
    const list = SEQUENCE[kind];
    if (!list) return null;
    const value = list[cursors[kind] % list.length];
    cursors[kind] += 1;
    currentVariant[kind] = value;
    return value;
  }

  function stageFor(key) {
    return stages.find((e) => e.dataset.companion === key) || null;
  }

  function apply(stage, state, title, detail, variant) {
    if (!stage) return;
    stage.dataset.state = state;
    if (variant) stage.dataset.variant = variant;
    else delete stage.dataset.variant;
    const t = stage.querySelector(".companion-title");
    const d = stage.querySelector(".companion-detail");
    const k = stage.querySelector(".companion-kind");
    if (t && title !== undefined && title !== null) t.textContent = title;
    if (d && detail !== undefined) d.textContent = detail;
    if (k) k.textContent = KIND_LABEL[state] || KIND_LABEL.node;
  }

  function syncMotion() {
    /* Pausing is an app-wide preference, so it is published on the root as
       well as on each stage: style.css mirrors the reduced-motion contract
       against it and no loop or transition is left running anywhere. */
    const off = paused || reduced.matches || document.hidden;
    document.documentElement.dataset.paused = String(off);
    stages.forEach((stage) => {
      stage.dataset.paused = String(off);
      const button = stage.querySelector(".companion-motion");
      if (!button) return;
      button.textContent = paused ? "Resume motion" : "Pause motion";
      button.setAttribute("aria-label", button.textContent + " for Luma");
      button.setAttribute("aria-pressed", String(paused));
    });
  }

  stages.forEach((stage) => {
    /* Only the node shell is markup. Backend strings are always written with
       textContent, never interpolated into HTML. */
    stage.innerHTML =
      '<figure class="companion-figure">' +
      NODE +
      "</figure>" +
      '<div class="companion-copy">' +
      '<figcaption class="companion-kind">Luma</figcaption>' +
      '<div class="companion-title"></div>' +
      '<p class="companion-detail"></p>' +
      '<span class="companion-reduced">Reduced motion — static state</span>' +
      "</div>" +
      '<button class="ghost-btn companion-motion" type="button" aria-pressed="false">Pause motion</button>';

    const button = stage.querySelector("button");
    button.addEventListener("click", () => {
      paused = !paused;
      try {
        localStorage.setItem("cure-companion-paused", String(paused));
      } catch (_) {
        /* optional */
      }
      syncMotion();
      /* One gesture governs the whole presentation: the scan field and the
         storage core honour the same preference. */
      if (window.CureSweep) window.CureSweep.motion(paused);
      if (window.CureStorage) window.CureStorage.motion(paused);
    });
  });

  window.CureCompanion = {
    beginSession(kind) {
      const variant = nextVariant(kind);
      if (kind === "scan") this.scan("scan", "Awaiting collection", 0, variant);
      if (kind === "cleanup") this.cleanup("scan", "Measuring candidates", variant);
      return variant;
    },
    variant(kind) {
      return currentVariant[kind] || null;
    },
    overview(state, title) {
      apply(stageFor("overview"), state, title || "Ready when you are", "");
    },
    scan(state, detail, sourceIndex, variant) {
      const title =
        state === "review"
          ? "Review the evidence"
          : state === "idle"
            ? "Collection ended"
            : state === "success"
              ? "Nothing found in collected evidence"
              : variant === "scan-scanner"
                ? "Tracing evidence points"
                : variant === "scan-lens"
                  ? "Analysing the layers"
                  : "Operating the scanner console";
      apply(stageFor("scan"), state, title, detail, variant || currentVariant.scan);
      const stage = stageFor("scan");
      if (!stage) return;
      /* A restrained lean so the node reads as tracking the active collector
         rather than sitting inert beside it. */
      const look = Math.max(-1, Math.min(1, (sourceIndex || 0) / 4 - 0.5));
      stage.style.setProperty("--look-x", (look * 2).toFixed(2) + "px");
    },
    result(state, title, detail) {
      apply(stageFor("result"), state, title, detail);
    },
    cleanup(state, title, detail, variant) {
      apply(stageFor("cleanup"), state, title, detail, variant || currentVariant.cleanup);
    },
    seal(detail) {
      apply(stageFor("result"), "seal", "Item sealed in quarantine", detail);
    },
  };

  apply(stageFor("overview"), "idle", "Ready when you are", "");
  apply(stageFor("scan"), "scan", "Awaiting collection", "");
  apply(stageFor("cleanup"), "idle", "Ready to measure", "");
  apply(stageFor("result"), "idle", "Evidence first", "");

  reduced.addEventListener("change", syncMotion);
  document.addEventListener("visibilitychange", syncMotion);
  syncMotion();
})();