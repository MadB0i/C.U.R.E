/* ==========================================================================
   LUMA V2 — The C.U.R.E Mascot & Cyber-Companion.
   Original Japanese anime-inspired feline character.

   Layered Vector Puppet Architecture:
     root
     ├ ground (ambient drop shadow + sleeping loaf shadow)
     ├ tailBase → tail1 → tail2 → tailTip (fluffy segmented anime tail with tabby rings)
     └ body
        ├ footHindL / footHindR (soft haunches with white pads & toes)
        ├ forelegL / forelegR   (white mitten paws with pink toe beans)
        ├ collar                (cybernetic violet smart collar with glowing C.U.R.E gem)
        └ head
           ├ earL, earR         (curved anime feline ears with soft pink canal & inner ear fluff)
           ├ skull              (chibi anime kitten silhouette with distinct fluffy cheeks)
           ├ browL, browR       (expressive feline brow arches)
           ├ eyes               (multi-layered glowing anime iris, catchlights & winged liner)
           ├ happyEyes          (celebration anime smile arcs "^ . ^")
           ├ muzzle (nose/mouth)(sculpted "ω" muzzle, pink nose, morphing mouth & tongue)
           ├ cheekL, cheekR     (rosy anime blush pads & angled manga hatching ticks)
           └ whiskers           (graceful curved anime whiskers)

   Animation Model:
     * 7 Distinct Visibly Choreographed States:
       1. IDLE: Natural breathing, relaxed tail wave, blinks, alternating ear twitches.
       2. SCANNING: Focused crouch, forward radar ears, contracted pupils, holographic cyber-HUD!
       3. CURIOUS: Deep 16° anime head tilt, asymmetric ears, wide sparkling eyes, raised paw!
       4. EXCITED: Joyful squash-and-stretch hop, open cheerful mouth, star sparkle bursts!
       5. WARNING: Airplane ears, furrowed brows, tense wavy mouth, flashing alert collar LED!
       6. SUCCESS: Curved anime smile eyes ("^ . ^"), joyful victory hop, emerald collar aura!
       7. SLEEPY: Curled cozy cat loaf, slow deep breathing, wrapped tail blanket, drifting "Zzz".
     * Interactive personality: Hover perking, click/tap happy purr squint with floating heart.
     * Full OS prefers-reduced-motion honor: cleanly pauses all active tracks at base pose.
   ========================================================================== */
(function () {
  "use strict";

  const NS = "http://www.w3.org/2000/svg";

  /* ---------- tiny builders --------------------------------------------- */

  function el(name, attrs, parent) {
    const node = document.createElementNS(NS, name);
    if (attrs) {
      for (const k in attrs) {
        if (attrs[k] === null || attrs[k] === undefined) continue;
        node.setAttribute(k, String(attrs[k]));
      }
    }
    if (parent) parent.appendChild(node);
    return node;
  }

  function P(d, cls, parent, extra) {
    if (extra === undefined) extra = {};
    const a = { d, class: cls };
    for (const k in extra) a[k] = extra[k];
    return el("path", a, parent);
  }

  /* ---------- palette --------------------------------------------------- */

  function token(name, fallback) {
    const css = getComputedStyle(document.documentElement);
    return (css.getPropertyValue(name) || "").trim() || fallback;
  }

  /* ---------- geometry constants ---------------------------------------- */

  const HEAD = { cx: 76, top: 18, bottom: 66.5, eyeY: 45 };
  const EYE  = { dx: 12.5, r: 7.4 };
  const TAIL = {
    base: [104, 103],
    a:    [121,  87],
    b:    [112,  70],
    tip:  [102.5, 56],
  };

  /* ---------- per-mount gradient suffix ---------------------------------- */

  let _mountSerial = 0;

  /* ---------- prop library (enhanced with futuristic cybernetic effects) - */

  const PROPS = {
    scan: function (g) {
      /* Floating Holographic Companion Scan Drone / Reticle
         Hovers gracefully beside LUMA (upper-right shoulder/ear),
         leaving face, eyes, chest, and paws 100% unobstructed. */
      const hud = el("g", { class: "lm-prop-scan", transform: "translate(40, -56)" }, g);
      /* Dual floating holographic radar rings */
      el("circle", { cx: 0, cy: 0, r: 13, class: "lm-holo-radar-ring" }, hud);
      el("circle", { cx: 0, cy: 0, r: 8,  class: "lm-holo-radar-inner" }, hud);
      /* Tactical targeting crosshairs */
      P("M-17 0 H-12 M12 0 H17 M0 -17 V-12 M0 12 V17", "lm-holo-cross", hud);
      /* Sweeping scan fan beam */
      P("M0 0 L10 -8 A13 13 0 0 1 13 0 Z", "lm-holo-radar-sweep", hud);
      /* Micro telemetry nodes */
      el("circle", { cx: 7,  cy: -6, r: 1.2, class: "lm-holo-node lm-node-1" }, hud);
      el("circle", { cx: -6, cy: 5,  r: 1.0, class: "lm-holo-node lm-node-2" }, hud);
      /* Core beacon spark */
      el("circle", { cx: 0, cy: 0, r: 2.0, class: "lm-holo-drone-core" }, hud);
    },
    discovering: function (g) {
      /* Radiant golden multi-point anime sparkle stars */
      const s = el("g", { class: "lm-prop-excited" }, g);
      P("M0 -16 L3.8 -3.8 L16 0 L3.8 3.8 L0 16 L-3.8 3.8 L-16 0 L-3.8 -3.8 Z", "lm-finding lm-star-main", s);
      P("M13 -13 L14.5 -9 L18.5 -8 L14.5 -7 L13 -3 L11.5 -7 L7.5 -8 L11.5 -9 Z", "lm-sparkle-sub lm-spark-1", s);
      P("M-14 9 L-13 6 L-10 5 L-13 4 L-14 1 L-15 4 L-18 5 L-15 6 Z", "lm-sparkle-sub lm-spark-2", s);
      el("circle", { cx: 0, cy: 0, r: 11, class: "lm-finding-ring" }, s);
    },
    major: function (g) {
      /* Warning / Alert: Holographic alert hazard shield */
      const s = el("g", { class: "lm-prop-major" }, g);
      el("polygon", { points: "0,-16 14,-8 14,8 0,16 -14,8 -14,-8", class: "lm-major-ring lm-warning-shield" }, s);
      P("M0 -14 L3.5 -3.5 L14 0 L3.5 3.5 L0 14 L-3.5 3.5 L-14 0 L-3.5 -3.5 Z", "lm-major-star lm-warning-star", s);
      P("M0 -7.5 V-1.5 M0 3.5 V4.8", "lm-warning-excl", s);
      el("circle", { cx: 0, cy: 0, r: 5.5, class: "lm-major-core" }, s);
    },
    curious: function (g) {
      /* Curious: Floating holographic question spark with pulse ripple */
      const s = el("g", { class: "lm-prop-curious" }, g);
      P("M-2.5 -8 a3.6 3.6 0 1 1 3.6 3.6 V-2.2", "lm-curious-hook", s);
      el("circle", { cx: 1.1, cy: 2.4, r: 1.3, class: "lm-curious-dot" }, s);
      el("circle", { cx: 1.1, cy: -2, r: 9.5, class: "lm-curious-ring" }, s);
      P("M8 -9 L9 -6.5 L11.5 -5.5 L9 -4.5 L8 -2 L7 -4.5 L4.5 -5.5 L7 -6.5 Z", "lm-curious-spark", s);
    },
    thinking: function (g) {
      /* Thinking bubbles */
      const s = el("g", { class: "lm-prop-think" }, g);
      el("circle", { cx: -2, cy: 0,   r: 2.4, class: "lm-think-bubble lm-tb-1" }, s);
      el("circle", { cx:  2, cy: -6,  r: 3.2, class: "lm-think-bubble lm-tb-2" }, s);
      el("circle", { cx: -1, cy: -13, r: 4.4, class: "lm-think-bubble lm-tb-3" }, s);
    },
    review: function (g) {
      /* Review prism */
      const s = el("g", { class: "lm-prop-review" }, g);
      P("M0 -11 L8.5 -3.5 L0 11 L-8.5 -3.5 Z", "lm-review-stone", s);
      P("M0 -5.5 L4.8 -2.2 L0 4.8 L-4.8 -2.2 Z", "lm-review-stone-core", s);
      el("circle", { cx: 0, cy: -10, r: 1.25, class: "lm-review-glint" }, s);
    },
    cleanup: function (g) {
      /* Victory celebration starburst & emerald sparkles */
      const s = el("g", { class: "lm-prop-sparkle lm-prop-success" }, g);
      P("M0 -16 L3.2 -3.8 L16 0 L3.2 3.8 L0 16 L-3.2 3.8 L-16 0 L-3.2 -3.8 Z", "lm-spark lm-spark-a", s);
      P("M12 -12 L13.4 -8.5 L17 -7.5 L13.4 -6.5 L12 -3 L10.6 -6.5 L7 -7.5 L10.6 -8.5 Z", "lm-spark lm-spark-b", s);
      P("M-13 -10 L-12 -7 L-9 -6 L-12 -5 L-13 -2 L-14 -5 L-17 -6 L-14 -7 Z", "lm-spark lm-spark-c", s);
      el("circle", { cx: 12, cy: 8,   r: 1.5, class: "lm-celebrate-dot lm-cdot-1" }, s);
      el("circle", { cx: -11, cy: -12, r: 1.3, class: "lm-celebrate-dot lm-cdot-2" }, s);
      el("circle", { cx: 7,   cy: -15, r: 1.1, class: "lm-celebrate-dot lm-cdot-3" }, s);
    },
    quarantine: function (g) {
      /* High-tech seal capsule */
      const s = el("g", { class: "lm-prop-seal" }, g);
      el("rect", { x: -12.5, y: -11.5, width: 25, height: 23, rx: 6, class: "lm-seal-cap" }, s);
      el("circle", { cx: 0, cy: 0, r: 6.8, class: "lm-seal-disc" }, s);
      P("M-3.2 0.2 L-0.8 2.8 L3.5 -2.6", "lm-seal-tick", s);
      el("ellipse", { cx: 0, cy: 0, rx: 6.8, ry: 6.8, class: "lm-seal-ring" }, s);
    },
    success: function (g) {
      PROPS.cleanup(g);
    },
    failure: function (g) {
      const s = el("g", { class: "lm-prop-fail" }, g);
      P("M-4.5 -10 a5.4 5.4 0 1 1 5.4 5.4 V-2", "lm-quest", s);
      el("circle", { cx: 0.9, cy: 3.6, r: 1.8, class: "lm-quest-dot" }, s);
    },
    cancelled: function (g) {
      const s = el("g", { class: "lm-prop-pause" }, g);
      el("circle", { cx: 0, cy: -1, r: 9, class: "lm-pause-ring" }, s);
      el("rect", { x: -3.2, y: -7, width: 2.6, height: 9.2, rx: 1.2, class: "lm-pause-bar" }, s);
      el("rect", { x: 0.8,  y: -7, width: 2.6, height: 9.2, rx: 1.2, class: "lm-pause-bar" }, s);
    },
    sleeping: function (g) {
      /* Floating anime Zzz strokes */
      const z = el("g", { class: "lm-zzz" }, g);
      const draw = (x, y, s, per) => {
        P("M" + (x - 4.5*s) + " " + (y - 4.5*s) + " h" + 9*s + " l" + -9*s + " " + 9*s + " h" + 9*s, "lm-z", z);
        z.lastElementChild.style.animationDelay = per;
      };
      draw(-11, -7,  0.65, "-2.6s");
      draw(-2,  -14, 0.85, "-1.3s");
      draw( 8,  -22, 1.05, "0s");
    },
  };

  /* Toe marks helper */
  const toeMarks = (x, y, parent, cls) =>
    P("M" + (x - 2.2) + " " + y + " v3.4 M" + (x + 1.2) + " " + (y + 0.2) + " v3.4",
      cls || "lm-toes", parent);

  /* ---------- labels & copy --------------------------------------------- */

  const KIND_LABEL = {
    idle:        "Idle",
    scan:        "Scanning",
    discovering: "Agent Discovered",
    major:       "Warning Alert",
    curious:     "Curious",
    thinking:    "Analyzing",
    review:      "Review Lead",
    cleanup:     "Celebration",
    quarantine:  "Sealing",
    success:     "Operation Complete",
    failure:     "Needs Inspection",
    cancelled:   "Paused",
    sleeping:    "Sleeping Loaf",
  };
  const STATE_LABEL = {
    ...KIND_LABEL,
    scan:        "Scanning system processes…",
    discovering: "Agent discovered in workspace!",
    curious:     "Investigating activity…",
    major:       "Security anomaly detected!",
    cleanup:     "All changes verified clean!",
    success:     "All operations successful!",
    sleeping:    "LUMA is resting peacefully.",
  };

  /* ---------- mount state tracking -------------------------------------- */

  const hosts     = [];
  const instances = [];
  let paused  = false;
  let asleep  = false;
  let visible = true;
  let current = "idle";

  try {
    paused = localStorage.getItem("cure-companion-paused") === "true";
  } catch (_) { }

  const reducedMedia = matchMedia("(prefers-reduced-motion: reduce)");

  /* ---------- vector puppet builder ------------------------------------- */

  function buildRig(suffix) {
    const svg = el("svg", {
      viewBox:       "0 0 152 136",
      class:         "luma-svg",
      focusable:     "false",
      "aria-hidden": "true",
    });
    const defs = el("defs", null, svg);

    /* Fur Gradient: Warm ginger tabby with soft ambient radiance */
    const furGrad = el("radialGradient", {
      id: "dfur" + suffix,
      gradientUnits: "userSpaceOnUse",
      cx: "62", cy: "26", r: "130",
    }, defs);
    el("stop", { offset: "0%",   "stop-color": "var(--luma-fur-lit)"  }, furGrad);
    el("stop", { offset: "48%",  "stop-color": "var(--luma-fur)"      }, furGrad);
    el("stop", { offset: "85%",  "stop-color": "var(--luma-fur-deep)" }, furGrad);
    el("stop", { offset: "100%", "stop-color": "var(--luma-fur-edge)" }, furGrad);

    /* Rim Light Gradient: Back edge anime contour */
    const rimGrad = el("linearGradient", {
      id: "drim" + suffix,
      gradientUnits: "userSpaceOnUse",
      x1: "104", y1: "56", x2: "112", y2: "128",
    }, defs);
    el("stop", { offset: "0%",   "stop-color": "var(--luma-fur-rim)", "stop-opacity": "0.95" }, rimGrad);
    el("stop", { offset: "65%",  "stop-color": "var(--luma-fur-rim)", "stop-opacity": "0.35" }, rimGrad);
    el("stop", { offset: "100%", "stop-color": "var(--luma-fur-rim)", "stop-opacity": "0"    }, rimGrad);

    /* Iris Gradient: Rich luminous golden amber to honey cognac bronze */
    const irisGrad = el("radialGradient", { id: "diris" + suffix, cx: "45%", cy: "30%", r: "80%" }, defs);
    el("stop", { offset: "0%",   "stop-color": "#ffe49e" }, irisGrad);
    el("stop", { offset: "38%",  "stop-color": "var(--luma-eye)" }, irisGrad);
    el("stop", { offset: "78%",  "stop-color": "var(--luma-eye-deep)" }, irisGrad);
    el("stop", { offset: "100%", "stop-color": "#743206" }, irisGrad);

    /* Cybernetic Smart Collar Gradient */
    const collarGrad = el("linearGradient", {
      id: "dcollar" + suffix,
      gradientUnits: "userSpaceOnUse",
      x1: "58", y1: "63", x2: "94", y2: "69",
    }, defs);
    el("stop", { offset: "0%",   "stop-color": "#9b7ef8" }, collarGrad);
    el("stop", { offset: "50%",  "stop-color": "var(--luma-collar)" }, collarGrad);
    el("stop", { offset: "100%", "stop-color": "var(--luma-collar-deep)" }, collarGrad);

    /* Anime Cat Head Clip Contour (with sculpted fluffy cheek tufts) */
    const headClip = el("clipPath", { id: "dhead" + suffix }, defs);
    el("path", {
      d: "M76 18 C 62 18, 52 23, 46 32 C 43 36, 38 41, 39 46 C 40.5 48, 43 49, 45 48.5 C 41 53, 42 58, 47 61 C 52 64, 62 66.5, 76 66.5 C 90 66.5, 100 64, 105 61 C 110 58, 111 53, 107 48.5 C 109 49, 111.5 48, 113 46 C 114 41, 109 36, 106 32 C 100 23, 90 18, 76 18 Z",
    }, headClip);

    /* Ground Shadow */
    const ground = el("g", { class: "lm-ground" }, svg);
    el("ellipse", { cx: 76, cy: 125.4, rx: 33, ry: 4.2, class: "lm-shadow" }, ground);
    el("ellipse", { cx: 76, cy: 125.4, rx: 42, ry: 4.6, class: "lm-shadow lm-shadow-sleep" }, ground);

    const root = el("g", { class: "lm-root" }, svg);
    const awake = el("g", { class: "lm-awake" }, root);

    /* Tail Assembly (4 joints tapering with tabby rings and fluffy cream tip) */
    const tailBase = el("g", { class: "lm-j lm-tail-base" }, awake);
    el("path", {
      d: "M104 103 C 113 104, 122 96, 121 87",
      class: "lm-tail-seg lm-tailseg-0",
      "stroke-width": 11.4, "stroke-linecap": "round", fill: "none",
    }, tailBase);
    el("path", { d: "M113.8 96.2 L118.6 101.4", class: "lm-tail-ring lm-ring-0" }, tailBase);

    const tail1 = el("g", { class: "lm-j lm-tail-1" }, tailBase);
    el("path", {
      d: "M121 87 C 121.5 78, 118.5 73, 112 70",
      class: "lm-tail-seg lm-tailseg-1",
      "stroke-width": 9.2, "stroke-linecap": "round", fill: "none",
    }, tail1);
    el("path", { d: "M116.6 77.2 L121.8 74.6", class: "lm-tail-ring lm-ring-1" }, tail1);

    const tail2 = el("g", { class: "lm-j lm-tail-2" }, tail1);
    el("path", {
      d: "M112 70 C 106.5 66.5, 102 63, 102.5 56",
      class: "lm-tail-seg lm-tailseg-2",
      "stroke-width": 7.2, "stroke-linecap": "round", fill: "none",
    }, tail2);
    el("path", { d: "M106.2 64.6 L110.8 63.4", class: "lm-tail-ring lm-ring-2" }, tail2);

    const tailTip = el("g", { class: "lm-j lm-tail-tip" }, tail2);
    P("M101.3 47.4 q 3.6 -1.8 4.8 2 l -2.4 -0.6 l -0.8 3.2 l -2.6 -1.6 l -3 2.2 Z", "lm-tail-tuft", tailTip);
    el("circle", { cx: TAIL.tip[0], cy: TAIL.tip[1], r: 5.6, class: "lm-tail-cap" }, tailTip);

    /* Body Assembly: Pear haunches, cream chest bib, tiger flank stripes */
    const body = el("g", { class: "lm-j lm-body" }, awake);
    el("path", {
      d: "M76 61 C 63 61, 55 63.5, 51 70 C 45 79.5, 43 94, 46 106.5 C 49 118.5, 60.5 124.5, 76 124.5 C 91.5 124.5, 103 118.5, 106 106.5 C 109 94, 107 79.5, 101 70 C 97 63.5, 89 61, 76 61 Z",
      fill: "url(#dfur" + suffix + ")",
      class: "lm-body-shape",
    }, body);

    el("path", {
      d: "M76 61 C 89 61, 97 63.5, 101 70 C 107 79.5, 109 94, 106 106.5 C 103 118.5, 91.5 124.5, 76 124.5",
      fill: "none",
      stroke: "url(#drim" + suffix + ")",
      "stroke-width": "3.4",
      "stroke-linecap": "round",
      class: "lm-body-rim",
    }, body);

    /* Cream belly */
    P("M76 95 C 67.5 95, 62.5 107.5, 64 118 C 66 123, 70.8 124.5, 76 124.5 C 81.2 124.5, 86 123, 88 118 C 89.5 107.5, 84.5 95, 76 95 Z", "lm-belly", body);

    /* Fluffy white anime chest ascot bib */
    P("M76 61 C 70 61, 65.5 63.2, 64.2 68 C 63 74, 65 82, 69 88 C 72 91.5, 74.5 93, 76 94 C 77.5 93, 80 91.5, 83 88 C 87 82, 89 74, 87.8 68 C 86.5 63.2, 82 61, 76 61 Z", "lm-bib", body);

    /* Flank tabby markings */
    const flank = el("g", { class: "lm-stripes" }, body);
    [
      "M48.6 75.5 C 52.4 77.6, 55.6 79.4, 58 81.6",
      "M46.4 87.5 C 50.4 89.6, 53.6 91.4, 56 93.6",
      "M47.2 99.5 C 51 101.6, 54 103.4, 56.4 105.4",
      "M103.4 75.5 C 99.6 77.6, 96.4 79.4, 94 81.6",
      "M105.6 87.5 C 101.6 89.6, 98.4 91.4, 96 93.6",
      "M104.8 99.5 C 101 101.6, 98 103.4, 95.6 105.4",
    ].forEach((d) => P(d, "lm-stripe", flank));

    /* Lit fur accents */
    const coatDetail = el("g", { class: "lm-coat-detail" }, body);
    [
      "M51.2 109.6 C 54.2 111.4, 57.4 112.6, 60.6 113",
      "M92.4 108.2 C 89.8 110.4, 86.6 112, 83.2 112.8",
      "M58 116.4 C 61.2 118, 65.4 119, 69.4 119.2",
      "M85.8 117.6 C 82.8 119, 79.2 119.6, 75.6 119.8",
    ].forEach((d) => P(d, "lm-fur-tuft", coatDetail));

    /* Hind feet */
    const footL = el("g", { class: "lm-foot lm-foot-hind-l" }, body);
    el("ellipse", { cx: 52, cy: 118.5, rx: 8.5, ry: 5.8, class: "lm-hindpad" }, footL);
    toeMarks(51.2, 115.5, footL);

    const footR = el("g", { class: "lm-foot lm-foot-hind-r" }, body);
    el("ellipse", { cx: 100, cy: 118.5, rx: 8.5, ry: 5.8, class: "lm-hindpad" }, footR);
    toeMarks(99.2, 115.5, footR);

    /* Forelegs & rounded white paws with pink toe beans */
    const pawL = el("g", { class: "lm-j lm-paw lm-paw-l" }, body);
    el("rect", { x: 54.5, y: 85, width: 10.2, height: 21, rx: 5.1, class: "lm-leg" }, pawL);
    el("rect", { x: 54.5, y: 98.5, width: 10.2, height: 10, rx: 4.8, class: "lm-sock" }, pawL);
    P("M55.5 99.8 C 57.2 98.2, 62 98.2, 63.7 99.8", "lm-sock-cuff", pawL);
    el("ellipse", { cx: 59.6, cy: 107.6, rx: 8.2, ry: 5.6, class: "lm-paw-pad" }, pawL);
    toeMarks(58.4, 105.2, pawL);
    el("ellipse", { cx: 59.6, cy: 107.6, rx: 2.4, ry: 1.6, class: "lm-toe-beans" }, pawL);

    const pawR = el("g", { class: "lm-j lm-paw lm-paw-r" }, body);
    el("rect", { x: 87.3, y: 85, width: 10.2, height: 21, rx: 5.1, class: "lm-leg" }, pawR);
    el("rect", { x: 87.3, y: 98.5, width: 10.2, height: 10, rx: 4.8, class: "lm-sock" }, pawR);
    P("M88.3 99.8 C 90 98.2, 94.8 98.2, 96.5 99.8", "lm-sock-cuff", pawR);
    el("ellipse", { cx: 92.4, cy: 107.6, rx: 8.2, ry: 5.6, class: "lm-paw-pad" }, pawR);
    toeMarks(91.2, 105.2, pawR);
    el("ellipse", { cx: 92.4, cy: 107.6, rx: 2.4, ry: 1.6, class: "lm-toe-beans" }, pawR);

    /* Cybernetic Smart Collar */
    const collar = el("g", { class: "lm-j lm-collar" }, body);
    P("M58 63.5 C 63.8 66.8, 88.2 66.8, 94 63.5 C 96.2 66.5, 93 69.8, 76 69.8 C 59 69.8, 55.8 66.5, 58 63.5 Z", "lm-collar-band", collar);
    P("M59.4 64.7 C 64.8 67.2, 87.2 67.2, 92.6 64.7", "lm-collar-sheen", collar);
    el("polygon", { points: "73.2,69.5 76,66 78.8,69.5 76,73", class: "lm-collar-tag" }, collar);
    el("circle", { cx: 76, cy: 69.5, r: 1.6, class: "lm-collar-led" }, collar);
    el("circle", { cx: 76, cy: 69.5, r: 3.4, class: "lm-collar-glow" }, collar);

    /* Head Assembly */
    const head = el("g", { class: "lm-j lm-head" }, awake);

    /* Ears: Large, expressive, curved feline ears with inner pink fold & ear fluff */
    const earL = el("g", { class: "lm-j lm-ear lm-ear-l" }, head);
    P("M61 27 Q 52 11 49.5 6.5 Q 62 11.5 72 23 Z", "lm-ear-outer", earL);
    P("M59.5 24 Q 54.5 13.5 53 10.5 Q 61 14.5 67 21.5 Z", "lm-ear-inner", earL);
    P("M57.5 25 Q 62 21.5 64.5 23 Q 60.5 26.5 57.5 25 Z", "lm-ear-fluff", earL);

    const earR = el("g", { class: "lm-j lm-ear lm-ear-r" }, head);
    P("M91 27 Q 100 11 102.5 6.5 Q 90 11.5 80 23 Z", "lm-ear-outer", earR);
    P("M92.5 24 Q 97.5 13.5 99 10.5 Q 91 14.5 85 21.5 Z", "lm-ear-inner", earR);
    P("M94.5 25 Q 90 21.5 87.5 23 Q 91.5 26.5 94.5 25 Z", "lm-ear-fluff", earR);

    /* Skull: Chibi anime kitten face with sculpted fluffy cheeks */
    el("path", {
      d: "M76 18 C 62 18, 52 23, 46 32 C 43 36, 38 41, 39 46 C 40.5 48, 43 49, 45 48.5 C 41 53, 42 58, 47 61 C 52 64, 62 66.5, 76 66.5 C 90 66.5, 100 64, 105 61 C 110 58, 111 53, 107 48.5 C 109 49, 111.5 48, 113 46 C 114 41, 109 36, 106 32 C 100 23, 90 18, 76 18 Z",
      fill: "url(#dfur" + suffix + ")",
      class: "lm-skull",
    }, head);

    el("path", {
      d: "M76 18 C 90 18, 100 23, 106 32 C 109 36, 114 41, 113 46 C 111.5 48, 109 49, 107 48.5 C 111 53, 110 58, 105 61 C 100 64, 90 66.5, 76 66.5",
      fill: "none",
      stroke: "url(#drim" + suffix + ")",
      "stroke-width": "2.8",
      "stroke-linecap": "round",
      class: "lm-skull-rim",
    }, head);

    /* Crown highlight & cream blaze */
    P("M62 22 C 66 19.8, 71 18.8, 76 18.8 C 81 18.8, 86 19.8, 90 22 C 85 23.4, 80.5 24.2, 76 24.2 C 71.5 24.2, 67 23.4, 62 22 Z", "lm-crown", head);
    P("M71.4 24 C 74.4 29.5, 74.2 35.5, 75.1 41.5 C 72.5 37, 71 31, 71.4 24 Z", "lm-blaze", head);

    /* Forehead Tabby Chevron Crest ("M" marking) & Cheek Marks */
    const headMarks = el("g", { class: "lm-stripes" }, head);
    [
      "M69.4 20.8 C 70.6 25.2, 71.1 29.2, 70.5 32.8",
      "M76 19.4 C 76 24.4, 76 28.8, 76 32.6",
      "M82.6 20.8 C 81.4 25.2, 80.9 29.2, 81.5 32.8",
      "M62.6 24.6 C 65.8 27.2, 67.4 30.4, 67.9 34",
      "M89.4 24.6 C 86.2 27.2, 84.6 30.4, 84.1 34",
      "M53.4 35.8 C 55.8 37.2, 57.6 38.8, 58.8 40.6",
      "M52.8 42.2 C 55.2 43.6, 56.8 44.8, 57.8 46.4",
      "M98.6 35.8 C 96.2 37.2, 94.4 38.8, 93.2 40.6",
      "M99.2 42.2 C 96.8 43.6, 95.2 44.8, 94.2 46.4",
    ].forEach((d) => P(d, "lm-stripe", headMarks));

    /* Eye sockets */
    el("ellipse", { cx: 63.5, cy: 45, rx: 10.8, ry: 11.6, class: "lm-socket" }, head);
    el("ellipse", { cx: 88.5, cy: 45, rx: 10.8, ry: 11.6, class: "lm-socket" }, head);

    /* Anime Cheek Blush Pads & Manga Ticks */
    el("ellipse", { cx: 56.5, cy: 56.5, rx: 5.4, ry: 3.2, class: "lm-blush lm-blush-l" }, head);
    el("ellipse", { cx: 95.5, cy: 56.5, rx: 5.4, ry: 3.2, class: "lm-blush lm-blush-r" }, head);
    P("M53.5 54.5 L55.8 58.5 M57.2 54.5 L59.5 58.5", "lm-blush-tick", head);
    P("M92.8 54.5 L95.1 58.5 M96.5 54.5 L98.8 58.5", "lm-blush-tick", head);

    /* Eyes (clipped to head) */
    const eyes = el("g", { class: "lm-eyes", "clip-path": "url(#dhead" + suffix + ")" }, head);
    const mkEye = (sign) => {
      const g = el("g", {
        class: "lm-eye",
        transform: "translate(" + (76 + sign*EYE.dx) + " " + HEAD.eyeY + ")",
      }, eyes);

      el("ellipse", { cx: 0, cy: 0, rx: EYE.r+1.9, ry: EYE.r+3.1, class: "lm-eye-rim" }, g);
      P("M-9.2 -1.5 C -6.5 -7.2, 5.5 -7.2, 9.8 -1.2", "lm-eye-lash", g);

      const iris = el("g", { class: "lm-iris" }, g);
      el("ellipse", { cx: 0, cy: 0.2, rx: EYE.r, ry: EYE.r+1.3, fill: "url(#diris" + suffix + ")", class: "lm-iris-fill" }, iris);
      el("ellipse", { cx: 0, cy: 0.2, rx: EYE.r, ry: EYE.r+1.3, class: "lm-iris-ring" }, iris);
      P("M-6 3.2 C -3.6 7.4, 3.6 7.4, 6 3.2", "lm-iris-glow", iris);

      const pupilG = el("g", { class: "lm-pupil-g" }, iris);
      el("ellipse", { cx: 0, cy: 0.4, rx: 2.8, ry: 5.2, class: "lm-pupil" }, pupilG);

      /* Triple Catchlights */
      el("circle", { cx: -2.5, cy: -3.0, r: 2.2, class: "lm-glint" }, iris);
      el("circle", { cx:  2.4, cy:  3.4, r: 1.1, class: "lm-glint lm-glint-rim" }, iris);
      el("circle", { cx: -0.6, cy:  2.9, r: 0.85, class: "lm-glint lm-glint-sub" }, iris);
      P("M-3.6 5.5 Q 0 7.2 3.6 5.5", "lm-eye-wet", iris);

      /* Natural Eyelid */
      const lid = el("g", { class: "lm-lid" }, g);
      el("path", {
        d: "M-10.5 -44 L 10.5 -44 L 10.5 3.8 C 10.5 8.8, 5.6 11.6, 0 11.6 C -5.6 11.6, -10.5 8.8, -10.5 3.8 Z",
        class: "lm-lid-fill",
      }, lid);
      P("M-9.2 2.6 C -4.8 7.8, 4.8 7.8, 9.2 2.6", "lm-lid-line", lid);

      /* Anime Celebration Smile Eyes (^ . ^) */
      P("M-7.2 1.5 C -3.6 -4.8, 3.6 -4.8, 7.2 1.5", "lm-happy-eyes", g);

      return g;
    };
    mkEye(-1);
    mkEye(1);

    /* Brows */
    const browL = el("g", { class: "lm-j lm-brow lm-brow-l" }, head);
    el("path", { d: "M55.8 33.6 q 4.8 -2.6 8.8 -0.6 q -0.2 1.1 -0.7 1.5 q -4 -1.8 -7.6 0.6 Z", class: "lm-brow-shape" }, browL);
    const browR = el("g", { class: "lm-j lm-brow lm-brow-r" }, head);
    el("path", { d: "M87.4 33 q 4 -2.4 8.8 0.2 q -0.5 0.7 -0.7 1.3 q -4.6 -2.4 -8.6 -0.4 Z", class: "lm-brow-shape" }, browR);

    /* Muzzle Assembly */
    const muzzle = el("g", { class: "lm-j lm-muzzle" }, head);
    P("M76 46.2 C 69.5 46.2, 65.8 48.6, 65.8 52.8 C 65.8 58.8, 70.2 63.6, 76 63.6 C 81.8 63.6, 86.2 58.8, 86.2 52.8 C 86.2 48.6, 82.5 46.2, 76 46.2 Z", "lm-muzzle-pad", muzzle);

    const nose = el("g", { class: "lm-j lm-nose" }, muzzle);
    P("M73.4 49 C 72.4 49.2, 72.1 50, 72.4 50.8 C 72.9 52.5, 74.4 53.8, 76 53.8 C 77.6 53.8, 79.1 52.5, 79.6 50.8 C 79.9 50, 79.6 49.2, 78.6 49 C 77.2 48.7, 74.8 48.7, 73.4 49 Z", "lm-nose-shape", nose);
    el("ellipse", { cx: 74.7, cy: 50.1, rx: 1.3, ry: 0.8, class: "lm-nose-shine" }, nose);
    el("ellipse", { cx: 76, cy: 61, rx: 5.6, ry: 2.3, class: "lm-chin-lift" }, muzzle);

    P("M76 53.8 L76 55.2", "lm-philtrum", muzzle);
    P("M76 55.2 v1.6 M72.2 57.1 C 73.2 58.9, 74.9 59, 76 57.5 C 77.1 59, 78.8 58.9, 79.8 57.1", "lm-mouth", muzzle);
    P("M72.4 57.2 C 73.3 60.8, 78.7 60.8, 79.6 57.2 C 77.3 55.8, 74.7 55.8, 72.4 57.2 Z", "lm-mouth-fill", muzzle);
    P("M74.2 58.6 C 74.9 60.3, 77.1 60.3, 77.8 58.6 C 77.1 59.5, 74.9 59.5, 74.2 58.6 Z", "lm-tongue", muzzle);

    /* Whiskers: Gracefully curved feline whiskers */

    /* Scanning state keeps eyes, nose, mouth and facial expressions
       100% visible — holographic HUD floats beside the cat as a companion drone. */

    const whiskers = el("g", { class: "lm-whiskers" }, head);
    [
      [62.5, 51.5, -16, -2.6],
      [62,   54.6, -17,  0.4],
      [63,   57.5, -14.5, 3.2],
      [89.5, 51.5,  16, -2.6],
      [90,   54.6,  17,  0.4],
      [89,   57.5,  14.5, 3.2],
    ].forEach(([x, y, dx, dy]) => {
      P("M" + x + " " + y + " q " + dx/2 + " " + (dy/2 - 1.2) + " " + dx + " " + dy, "lm-whisker", whiskers);
    });

    /* Prop Slot: floating effects slot */
    const prop = el("g", { class: "lm-prop", transform: "translate(76 92)" }, root);

    /* Petting Heart Sparkle Particle */
    const petHeart = el("g", { class: "lm-pet-heart-group", transform: "translate(76 36)" }, root);
    P("M0 -4 C -2.5 -9, -8 -6, -8 -1.5 C -8 3, -1.5 7.5, 0 9.5 C 1.5 7.5, 8 3, 8 -1.5 C 8 -6, 2.5 -9, 0 -4 Z", "lm-pet-heart", petHeart);

    /* Curled Sleeping Cat Loaf */
    const sleep = el("g", { class: "lm-asleep" }, root);
    el("path", {
      d: "M38 106 C 38 88, 50 74, 70 73 C 92 72, 108 79, 114 94 C 119 107, 106 120, 84 121 C 60 122, 38 120, 38 106 Z",
      fill: "url(#dfur" + suffix + ")",
      class: "lm-sleep-body",
    }, sleep);

    el("path", {
      d: "M40 103 C 41 87, 52 76, 70 75 C 91 74, 107 80, 113 93",
      fill: "none",
      stroke: "url(#drim" + suffix + ")",
      "stroke-width": "2.8",
      "stroke-linecap": "round",
      class: "lm-sleep-rim",
    }, sleep);

    P("M43 100 C 45 92, 55 88, 64 90 C 70 92, 72 100, 68 106 C 62 112, 50 112, 45 107 Z", "lm-sleep-belly", sleep);
    [
      "M58 74.4 C 60 79, 61 83, 61 87",
      "M74 72.6 C 76 77, 77 81, 77 85",
      "M90 76 C 92 80, 93 84, 93 88",
    ].forEach((d) => P(d, "lm-stripe", sleep));

    const tailPath = "M104 82 C 115 97, 105 114, 86 118 C 70 120, 56 117, 48 110";
    el("path", { d: tailPath, fill: "none", stroke: "var(--luma-fur)", "stroke-width": "8.8", "stroke-linecap": "round", class: "lm-sleep-tail" }, sleep);
    el("path", { d: "M101 81 C 111 95, 101 110, 84 114.6", fill: "none", stroke: "var(--luma-fur-lit)", "stroke-width": "2.4", "stroke-linecap": "round", class: "lm-sleep-tail-hi" }, sleep);
    el("path", { d: "M50.4 113.2 C 48 109.8, 46.6 106.2, 46.4 102.6 C 49.6 104.2, 52.2 106.6, 53.8 109.6 C 53 111.2, 51.8 112.4, 50.4 113.2 Z", class: "lm-tail-cap" }, sleep);

    el("ellipse", { cx: 96, cy: 96, rx: 19, ry: 18, class: "lm-sleep-haunch" }, sleep);
    el("ellipse", { cx: 66, cy: 100, rx: 24, ry: 15, class: "lm-sleep-occlusion" }, sleep);

    /* Sleeping Head */
    const sleepHead = el("g", { class: "lm-sleep-head", transform: "rotate(-14 54 96)" }, sleep);
    P("M38 76 Q 28 66, 26.6 60 Q 36 63, 44 72 Z", "lm-ear-outer", sleepHead);
    P("M37.2 74 Q 31.6 67, 31 63 Q 36.8 65, 40.4 71 Z", "lm-ear-inner", sleepHead);
    el("path", {
      d: "M48 68 C 34 68, 26 77.6, 26.6 89 C 27.2 99.6, 36 107.4, 48.4 107.4 C 60.8 107.4, 69.2 99.6, 69.8 89 C 70.4 77.6, 62 68, 48 68 Z",
      fill: "url(#dfur" + suffix + ")",
      class: "lm-skull",
    }, sleepHead);
    P("M45.4 71.2 C 47.4 75.2, 47.6 80, 47.2 84.2 C 45.4 80.2, 44.8 75.6, 45.4 71.2 Z", "lm-blaze", sleepHead);
    P("M31.6 87 C 34.6 91.6, 41.4 91.6, 44.4 87", "lm-lid-line", sleepHead);
    P("M42.6 95 C 38.6 95, 36.2 96.8, 36.2 98.8 C 36.2 101.2, 38.4 102.8, 42.6 102.8 C 46.8 102.8, 49 101.2, 49 98.8 C 49 96.8, 46.6 95, 42.6 95 Z", "lm-muzzle-pad", sleepHead);
    P("M40.2 96.2 C 39.4 96.4, 39.2 97.2, 39.5 97.9 C 39.9 99.2, 41 100, 42.6 100 C 44.2 100, 45.3 99.2, 45.7 97.9 C 46 97.2, 45.8 96.4, 45 96.2 C 43.8 95.9, 41.4 95.9, 40.2 96.2 Z", "lm-nose-shape", sleepHead);
    el("ellipse", { cx: 41.2, cy: 97.1, rx: 1.05, ry: 0.62, class: "lm-nose-shine" }, sleepHead);
    P("M62 84.6 C 65.4 88, 68.4 89.6, 71.6 89.6", "lm-collar-band", sleepHead);
    el("circle", { cx: 72.6, cy: 89.8, r: 1.3, class: "lm-collar-led" }, sleepHead);

    const sleepPawL = el("g", { class: "lm-sleep-paw-grip" }, sleep);
    el("ellipse", { cx: 54.4, cy: 110, rx: 8.6, ry: 5, class: "lm-sleep-wrist" }, sleepPawL);
    el("ellipse", { cx: 56, cy: 111.4, rx: 7.6, ry: 4.4, class: "lm-sleep-paw" }, sleepPawL);
    toeMarks(54.4, 108.4, sleepPawL);

    return {
      svg, root, body, head, awake, sleep,
      earL, earR, browL, browR,
      eyes, muzzle, whiskers,
      pawL, pawR, footL, footR, collar,
      tailBase, tail1, tail2, tailTip,
      prop,
    };
  }

  function mount(host) {
    const figure = document.createElement("figure");
    figure.className = "luma-figure";

    const stage = document.createElement("div");
    stage.className = "luma-stage";

    const suffix = "_" + (++_mountSerial);
    const rig = buildRig(suffix);
    stage.appendChild(rig.svg);

    const copy = document.createElement("figcaption");
    copy.className = "luma-copy";
    const kind   = document.createElement("span"); kind.className   = "luma-kind";
    const title  = document.createElement("span"); title.className  = "luma-title";
    const detail = document.createElement("span"); detail.className = "luma-detail";
    const reducedNote = document.createElement("span");
    reducedNote.className = "luma-reduced";
    reducedNote.textContent = "Reduced motion: LUMA holds a still pose.";
    copy.append(kind, title, detail, reducedNote);

    const controls = document.createElement("div");
    controls.className = "luma-controls";
    const hideBtn = document.createElement("button");
    hideBtn.type = "button";
    hideBtn.className = "luma-hide";
    hideBtn.textContent = "Hide";
    hideBtn.setAttribute("aria-label", "Hide LUMA");
    controls.appendChild(hideBtn);

    stage.setAttribute("role", "button");
    stage.setAttribute("tabindex", "0");
    stage.setAttribute("aria-label", "Interact with LUMA");

    stage.addEventListener("click", (ev) => {
      if (paused || reducedMedia.matches) return;
      if (ev.target === rig.svg) return;
      pet(inst);
    });
    stage.addEventListener("keydown", (ev) => {
      if (ev.key !== "Enter" && ev.key !== " ") return;
      ev.preventDefault();
      if (paused || reducedMedia.matches) return;
      pet(inst);
    });

    figure.append(stage, copy, controls);
    host.replaceChildren(figure);
    host.classList.add("luma-host");

    const inst = {
      host, figure, stage, copy, kind, title, detail,
      hideBtn,
      svg: rig.svg,
      tracks: [],
      timer: 0,
      pulseTimer: 0,
      rng: makeRng(hostIndexSeed()),
      state: "idle",
      scoped: false,
      listeners: [],
    };
    inst.host.hidden = !visible;
    inst.host.setAttribute("data-hidden", String(!visible));
    instances.push(inst);
    return inst;
  }

  let mountCount = 0;
  function hostIndexSeed() {
    return (mountCount++ * 2654435761 + 11) >>> 0;
  }

  function makeRng(seed) {
    let s = seed || 1;
    return function () {
      s = (s * 1664525 + 1013904223) >>> 0;
      return s / 4294967296;
    };
  }

  /* ---------- motion governance ------------------------------------------ */

  let unfocused = false;

  function motionAllowed() {
    return !paused && !reducedMedia.matches && !document.hidden && !unfocused;
  }

  function syncMotion() {
    const off = paused || reducedMedia.matches || document.hidden || unfocused;
    document.documentElement.setAttribute("data-paused", String(off));
    for (const inst of instances) {
      inst.stage.setAttribute("data-paused", String(off));
      if (off) park(inst);
      else resume(inst);
    }
  }

  function park(inst) {
    for (const t of inst.tracks) t.cancel();
    inst.tracks = [];
    if (inst.timer) { clearTimeout(inst.timer); inst.timer = 0; }
  }

  function resume(inst) {
    if (inst.timer) clearTimeout(inst.timer);
    inst.timer = 0;
    scheduleNext(inst);
  }

  /* ---------- beat scheduler --------------------------------------------- */

  function beat(inst, tracks) {
    inst.tracks = inst.tracks.filter((track) => {
      const state = track.playState;
      if (state === "finished" || state === "idle" || state === "aborted") {
        track.cancel();
        return false;
      }
      return true;
    });
    for (const t of tracks) {
      const node = t.sel ? inst.svg.querySelector(t.sel) : null;
      if (!node) continue;
      inst.tracks = inst.tracks.filter((track) => {
        if (track.effect && track.effect.target === node) {
          track.cancel(); return false;
        }
        return true;
      });
      const anim = node.animate(t.keyframes, t.opts);
      inst.tracks.push(anim);
    }
  }

  const flickL = () => [
    { sel: ".lm-ear-l", keyframes: [
      { transform: "rotate(0deg)", offset: 0 },
      { transform: "rotate(-14deg) translateY(-1.2px)", offset: 0.28, easing: "cubic-bezier(.34,1.56,.64,1)" },
      { transform: "rotate(4deg)", offset: 0.6 },
      { transform: "rotate(0deg)", offset: 1 },
    ], opts: { duration: 640, easing: "ease-out" } },
  ];

  const flickR = () => [
    { sel: ".lm-ear-r", keyframes: [
      { transform: "rotate(0deg)", offset: 0 },
      { transform: "rotate(14deg) translateY(-1.2px)", offset: 0.28, easing: "cubic-bezier(.34,1.56,.64,1)" },
      { transform: "rotate(-4deg)", offset: 0.6 },
      { transform: "rotate(0deg)", offset: 1 },
    ], opts: { duration: 640, easing: "ease-out" } },
  ];

  const glance = (dir) => {
    const dx = dir * 2.8;
    return [
      { sel: ".lm-eyes", keyframes: [
        { transform: "translateX(0px)", offset: 0 },
        { transform: "translateX(" + dx + "px)", offset: 0.35, easing: "cubic-bezier(.3,.9,.4,1)" },
        { transform: "translateX(" + dx + "px)", offset: 0.75 },
        { transform: "translateX(0px)", offset: 1 },
      ], opts: { duration: 1500, easing: "ease-out" } },
      { sel: ".lm-head", keyframes: [
        { transform: "rotate(0deg)", offset: 0 },
        { transform: "rotate(" + dir*3.4 + "deg)", offset: 0.4, easing: "cubic-bezier(.25,.8,.35,1)" },
        { transform: "rotate(" + dir*3.4 + "deg)", offset: 0.8 },
        { transform: "rotate(0deg)", offset: 1 },
      ], opts: { duration: 1900, easing: "ease-in-out" } },
    ];
  };

  const tailFlick = () => [
    { sel: ".lm-tail-1", keyframes: [
      { transform: "rotate(0deg)", offset: 0 },
      { transform: "rotate(-14deg)", offset: 0.3, easing: "cubic-bezier(.3,.7,.4,1)" },
      { transform: "rotate(6deg)", offset: 0.65 },
      { transform: "rotate(0deg)", offset: 1 },
    ], opts: { duration: 1050, easing: "ease-in-out" } },
    { sel: ".lm-tail-tip", keyframes: [
      { transform: "rotate(0deg)", offset: 0 },
      { transform: "rotate(16deg)", offset: 0.45, easing: "ease-out" },
      { transform: "rotate(-4deg)", offset: 0.75 },
      { transform: "rotate(0deg)", offset: 1 },
    ], opts: { duration: 1200, easing: "ease-in-out" } },
  ];

  const doubleBlink = () => [
    { sel: ".lm-eyes .lm-eye:first-child .lm-lid", keyframes: [
      { transform: "translateY(-38px)", offset: 0 },
      { transform: "translateY(0px)", offset: 0.14 },
      { transform: "translateY(-38px)", offset: 0.34 },
      { transform: "translateY(0px)", offset: 0.5 },
      { transform: "translateY(-38px)", offset: 0.72 },
      { transform: "translateY(-38px)", offset: 1 },
    ], opts: { duration: 860, easing: "ease-in-out" } },
    { sel: ".lm-eyes .lm-eye:last-child .lm-lid", keyframes: [
      { transform: "translateY(-38px)", offset: 0 },
      { transform: "translateY(0px)", offset: 0.15 },
      { transform: "translateY(-38px)", offset: 0.34 },
      { transform: "translateY(0px)", offset: 0.51 },
      { transform: "translateY(-38px)", offset: 0.72 },
      { transform: "translateY(-38px)", offset: 1 },
    ], opts: { duration: 860, easing: "ease-in-out" } },
  ];

  const BEATS = {
    idle: [
      [flickL, 2], [flickR, 2], [() => glance(-1), 2], [() => glance(1), 2],
      [tailFlick, 1], [doubleBlink, 1]
    ],
    scan: [
      [doubleBlink, 1], [flickR, 1]
    ],
    discovering: [
      [doubleBlink, 1], [tailFlick, 2]
    ],
    curious: [
      [flickL, 1], [doubleBlink, 1]
    ],
  };

  function pickBeat(inst) {
    const lib = BEATS[inst.state] || BEATS.idle;
    const play = (b) => (Array.isArray(b) ? b : b());
    let total = 0;
    for (const [, w] of lib) total += w;
    let roll = inst.rng() * total;
    for (const [b, w] of lib) {
      roll -= w;
      if (roll <= 0) return play(b);
    }
    return play(lib[0][0]);
  }

  function scheduleNext(inst) {
    if (!motionAllowed()) return;
    if (inst.timer) clearTimeout(inst.timer);
    const delay = 3500 + inst.rng() * 5000;
    inst.timer = setTimeout(() => {
      inst.timer = 0;
      if (!motionAllowed()) return;
      beat(inst, pickBeat(inst));
      scheduleNext(inst);
    }, delay);
  }

  /* ---------- state transitions ----------------------------------------- */

  function clearTracks(inst) {
    for (const t of inst.tracks) t.cancel();
    inst.tracks = [];
    if (inst.timer) { clearTimeout(inst.timer); inst.timer = 0; }
  }

  function applyState(inst, stateName) {
    const was = inst.state;
    inst.state = stateName;
    clearTracks(inst);
    const svg = inst.svg;
    svg.setAttribute("data-state", stateName);

    if (stateName === "sleeping") {
      if (was !== "sleeping") svg.setAttribute("data-sleep", "down");
    } else if (was === "sleeping") {
      svg.setAttribute("data-sleep", "up");
    } else {
      svg.removeAttribute("data-sleep");
    }

    const prop = svg.querySelector(".lm-prop");
    if (prop) {
      prop.replaceChildren();
      const make = PROPS[stateName];
      if (make) {
        make(prop);
        prop.style.animation = "none";
        prop.style.opacity   = "1";
        prop.style.scale     = "1";
        void prop.getBoundingClientRect();
        prop.style.animation = "";
        prop.style.opacity   = "";
        prop.style.scale     = "";
      }
    }
    if (motionAllowed()) scheduleNext(inst);
  }

  function pet(inst) {
    if (!inst) return;
    const svg = inst.svg;
    if (!svg) return;
    clearTracks(inst);
    svg.classList.remove("lm-pulse");
    void svg.getBoundingClientRect();
    svg.classList.add("lm-pulse");
    if (inst.pulseTimer) clearTimeout(inst.pulseTimer);
    inst.pulseTimer = setTimeout(() => {
      inst.pulseTimer = 0;
      svg.classList.remove("lm-pulse");
      if (motionAllowed()) scheduleNext(inst);
    }, 1200);
  }

  function setVisible(next) {
    visible = !!next;
    try {
      localStorage.setItem("cure-companion-visible", String(visible));
    } catch (_) { }
    for (const inst of instances) {
      inst.host.hidden = !visible;
      inst.host.setAttribute("data-hidden", String(!visible));
      if (!visible) {
        if (inst.timer) { clearTimeout(inst.timer); inst.timer = 0; }
        for (const track of inst.tracks) track.cancel();
        inst.tracks = [];
        if (inst.pulseTimer) { clearTimeout(inst.pulseTimer); inst.pulseTimer = 0; }
      } else if (motionAllowed()) {
        scheduleNext(inst);
      }
    }
    window.dispatchEvent(new CustomEvent("cure-companion-visibility", { detail: { visible } }));
  }

  function setMotionPaused(next) {
    paused = !!next;
    try {
      localStorage.setItem("cure-companion-paused", String(paused));
    } catch (_) { }
    syncMotion();
  }

  /* ---------- public API ------------------------------------------------ */

  function writeCopy(inst, label, title, detail) {
    inst.kind.textContent   = label;
    inst.title.textContent  = title  || "";
    inst.detail.textContent = detail || "";
  }

  function apply(stateName, title, detail) {
    current = stateName;
    const label = KIND_LABEL[stateName] || "Idle";
    const text  = STATE_LABEL[stateName] || stateName;
    for (const inst of instances) {
      if (inst.scoped) continue;
      writeCopy(inst, label, title || text, detail || null);
      inst.host.setAttribute("data-state", stateName);
      inst.copy.setAttribute("role", "status");
      applyState(inst, stateName);
    }
  }

  const CompanionAPI = {
    idle: function (detail) {
      asleep = false;
      apply("idle", null, detail || null);
    },
    discovering: function (count, detail) {
      if (!count) { apply("idle", null, detail || null); return; }
      asleep = false;
      apply("discovering", count + (count === 1 ? " finding" : " findings"), detail || null);
    },
    notice: function (detail) {
      asleep = false;
      apply("discovering", "New clue discovered", detail || null);
    },
    major: function (detail) {
      asleep = false;
      apply("major", "Security anomaly flagged", detail || null);
    },
    curious: function (detail) {
      asleep = false;
      apply("curious", null, detail || null);
    },
    thinking: function (detail) {
      asleep = false;
      apply("thinking", null, detail || null);
    },
    scan: function (stateName, detail, variantIndex) {
      asleep = false;
      apply(stateName || "scan", null, detail || null);
      if (variantIndex !== undefined && variantIndex !== null) {
        for (const inst of instances) {
          inst.stage.style.setProperty("--luma-lean", String(variantIndex));
        }
      }
    },
    review: function (count, detail) {
      asleep = false;
      apply("review", count ? count + " to review" : "Review", detail || null);
    },
    cleanup: function (stateName, title, detail) {
      asleep = false;
      apply(stateName || "cleanup", title || null, detail || null);
    },
    seal: function (detail) {
      asleep = false;
      apply("quarantine", null, detail || null);
    },
    failed: function (detail) {
      asleep = false;
      apply("failure", null, detail || null);
    },
    cancelled: function (detail) {
      asleep = false;
      apply("cancelled", null, detail || null);
    },
    sleep: function () {
      asleep = true;
      apply("sleeping", null, null);
    },
    sleepy: function () {
      asleep = true;
      apply("sleeping", null, null);
    },
    scanning: function (detail) {
      asleep = false;
      apply("scan", null, detail || null);
    },
    excited: function (detail) {
      asleep = false;
      apply("discovering", "Agent discovered", detail || null);
    },
    warning: function (detail) {
      asleep = false;
      apply("major", "Warning", detail || null);
    },
    success: function (detail) {
      asleep = false;
      apply("cleanup", "Success", detail || null);
    },
    wake: function () {
      asleep = false;
      apply("idle", null, null);
    },
    isAsleep: function ()  { return asleep; },
    isVisible: function () { return visible; },
    setVisible: setVisible,
    isMotionPaused: function () { return paused; },
    setMotionPaused: setMotionPaused,
    destroy: function (hostName) {
      const inst = hostName
        ? instances.find((i) => i.host.getAttribute("data-companion") === hostName)
        : instances[0];
      destroy(inst);
    },
    instances: function () { return instances.slice(); },
    state:    function ()  { return current; },
    isPaused: function ()  { return paused; },
    setState: function (name, detail) {
      if (typeof this[name] === "function") { this[name](detail); } else { apply(name, null, detail || null); }
    },
    onHost: function (hostName) {
      const inst = instances.find(i => i.host.getAttribute("data-companion") === hostName);
      if (!inst) throw new Error("no companion host named " + hostName);
      inst.scoped = true;
      const scopedApply = (stateName, title, detail) => {
        current = stateName;
        const label = KIND_LABEL[stateName] || "Idle";
        const text  = STATE_LABEL[stateName] || stateName;
        writeCopy(inst, label, title || text, detail || null);
        inst.host.setAttribute("data-state", stateName);
        applyState(inst, stateName);
      };
      return {
        idle:        (detail)            => scopedApply("idle", null, detail || null),
        scan:        (stateName, detail) => scopedApply(stateName || "scan", null, detail || null),
        curious:     (detail)            => scopedApply("curious", null, detail || null),
        thinking:    (detail)            => scopedApply("thinking", null, detail || null),
        discovering: (count, detail)     => {
          if (!count) { scopedApply("idle", null, detail || null); return; }
          scopedApply("discovering", count + (count === 1 ? " finding" : " findings"), detail || null);
        },
        notice:      (detail)            => scopedApply("discovering", "New clue discovered", detail || null),
        major:       (detail)            => scopedApply("major", "Large concentration", detail || null),
        review:      (count, detail)     => scopedApply("review", count ? count + " to review" : "Review", detail || null),
        cleanup:     (stateName, title, detail) => scopedApply(stateName || "cleanup", title || null, detail || null),
        seal:        (detail)            => scopedApply("quarantine", null, detail || null),
        failed:      (detail)            => scopedApply("failure", null, detail || null),
        cancelled:   (detail)            => scopedApply("cancelled", null, detail || null),
        sleep:       ()                  => scopedApply("sleeping", null, null),
      };
    },
  };

  window.CureCompanion = CompanionAPI;
  window.Luma = CompanionAPI;

  function destroy(inst) {
    if (!inst) return;
    clearTracks(inst);
    if (inst.timer) { clearTimeout(inst.timer); inst.timer = 0; }
    if (inst.pulseTimer) { clearTimeout(inst.pulseTimer); inst.pulseTimer = 0; }
    for (const off of inst.listeners || []) {
      try { off(); } catch (_) { }
    }
    inst.listeners = [];
    const at = instances.indexOf(inst);
    if (at >= 0) instances.splice(at, 1);
    try { inst.host.replaceChildren(); } catch (_) { }
  }

  function wire() {
    for (const hostEl of document.querySelectorAll("[data-companion]")) {
      hosts.push(hostEl);
    }
    for (const host of hosts) {
      const inst = mount(host);
      inst.hideBtn.addEventListener("click", () => setVisible(false));
    }

    reducedMedia.addEventListener("change", () => syncMotion());
    document.addEventListener("visibilitychange", syncMotion);
    window.addEventListener("blur",    () => { unfocused = true;  syncMotion(); });
    window.addEventListener("focus",   () => { unfocused = false; syncMotion(); });
    window.addEventListener("storage", syncMotion);

    syncMotion();
    apply("idle", null, null);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", wire);
  } else {
    wire();
  }
})();
