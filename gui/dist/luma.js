/* ==========================================================================
   LUMA — the C.U.R.E companion. Second generation, character overhaul.

   An original character, hand-authored as a vector puppet: one silhouette
   built from bezier fur, shaded with layered fills and two rim tones, rigged
   as named joints and animated by a hybrid director.

   Rig (every joint is a <g> so motion pivots at real anatomy)
   ---
     root                          (settles, squashes, hops, petting lean)
     ├ tailBase → tail1 → tail2 → tailTip     (4-joint tapering tail)
     └ body
        ├ footHindL / footHindR    (still, haunches)
        ├ forelegL / forelegR      (raise, point, hold, press)
        └ head
           ├ earL, earR            (independent rotate; inner fur)
           ├ browL, browR          (the whole emotional range)
           ├ eyeL, eyeR → iris → pupil → lid  (gaze, blink, pupil shape)
           ├ muzzle (nose, mouth)  (mouth morphs per mood)
           ├ cheekL, cheekR        (blush on petting)
           └ collar (tag, LED)     (violet tech collar)

   Animation model
   ---------------
   * CSS compositor keyframes carry everything that loops while a state is
     held (breathing, blinking, tail sway, scanning beam, sorting chips,
     zzz float). They run on the GPU backend of the browser, cost no script
     time, and are killed wholesale by the single `data-paused` switch.
   * A bounded deterministic scheduler (seeded LCG — no Math.random, no
     unbounded drift) fires short authored "beats" through the Web Animations
     API for the non-looping micro-motion: ear flicks, glances, saccades,
     tail flourishes. Beats are cancelled and cleared on every state change,
     so authored keyframes always start from the state's base pose.
   * State entry poses (head tilt, brow angle, muzzle, prop anchor) are plain
     CSS on `[data-state]`, so a transition is a style recalc, not script.
   * Gradient ids are suffixed per mount so cloned SVGs never share defs.

   Character rules
   ---------------
   * LUMA only ever reacts to state the engine actually produced. `discover`
     fires when a real finding count arrives; `success` fires when a real byte
     count arrives. A zero count can never produce a discovery pose.
   * She never covers evidence: every prop renders inside her own viewBox.
   * No <text>, no <image>, no filters — the figure stays out of the a11y
     tree and never adds a post-processing pass.
   * Asleep she curls up and breathes slowly; the copy beside her carries the
     state for screen readers.
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

  const FUR_DEEP  = token("--luma-fur-deep",    "#a3511d");
  const FUR       = token("--luma-fur",          "#d97b32");
  const FUR_LIT   = token("--luma-fur-lit",      "#f2a95c");
  const FUR_EDGE  = token("--luma-fur-edge",     "#7d3d12");
  const FUR_RIM   = token("--luma-fur-rim",      "#ffd7a0"); /* back-rim light */
  const STRIPE    = token("--luma-stripe",       "#a8501c"); /* tabby markings */
  const MANE      = token("--luma-mane",         "#f7e2c4");
  const INNER_EAR = token("--luma-inner-ear",    "#f4a3a8");
  const CREAM     = token("--luma-cream",        "#fdf3e4");
  const SOCK      = token("--luma-sock",         "#fffaf2");
  const AMBER     = token("--luma-eye",          "#ffc264");
  const AMBER_DEEP= token("--luma-eye-deep",     "#d97d1c");

  const COLLAR    = token("--luma-collar",       "#7d63d8");
  const COLLAR_DEEP=token("--luma-collar-deep",  "#5c49b2");
  const LED       = token("--luma-collar-led",   "#8ef0c8");  // eslint-disable-line
  const NOSE      = token("--luma-nose",         "#f08ca8");
  const MOUTH_INK = token("--luma-mouth",        "#5a2a18");  // eslint-disable-line
  const MOUTH_FILL= token("--luma-mouth-fill",   "#7d3a24");  // eslint-disable-line
  const TONGUE    = token("--luma-tongue",       "#f0909f");  // eslint-disable-line

  /* ---------- geometry ---------------------------------------------------
     A 152x136 box. She sits low and centred; head is nearly half the height
     (chibi reading), with headroom for ears to flick and props to hover. */

  const HEAD = { cx: 76, top: 20, bottom: 66, eyeY: 45 };
  const EYE  = { dx: 12.5, r: 7.1 };
  const TAIL = {
    base: [104, 103],
    a:    [121,  87],
    b:    [112,  70],
    tip:  [102.5, 56],
  };

  /* ---------- per-mount gradient suffix ---------------------------------- */

  let _mountSerial = 0;

  function suffixDefs(svgNode, suffix) {
    /* Rewrite every id="dXXX" in <defs> and every fill/clip-path that
       references it, so cloned SVGs never share gradient definitions. */
    const IDS = ["dfur", "diris", "dhead"];
    IDS.forEach(id => {
      const node = svgNode.querySelector("#" + id);
      if (node) node.id = id + suffix;
    });
    /* Rewrite fill="url(#dXXX)" on body-shape, skull, lid-fill */
    svgNode.querySelectorAll("[fill]").forEach(n => {
      const f = n.getAttribute("fill");
      if (f && f.startsWith("url(#d")) {
        IDS.forEach(id => {
          if (f === "url(#" + id + ")") n.setAttribute("fill", "url(#" + id + suffix + ")");
        });
      }
    });
    svgNode.querySelectorAll("[clip-path]").forEach(n => {
      const cp = n.getAttribute("clip-path");
      if (cp && cp.startsWith("url(#d")) {
        IDS.forEach(id => {
          if (cp === "url(#" + id + ")") n.setAttribute("clip-path", "url(#" + id + suffix + ")");
        });
      }
    });
  }

  /* ---------- prop library -----------------------------------------------
     Every state gets exactly one prop slot (idle gets none). Props are
     anchored by CSS per state so they sit where she is holding them, not
     floating above her head. */

  const fileTapPath = (s, cls, parent) =>
    P(
      "M" + -6*s + " " + -13*s +
        " h" + 4*s + " l" + 4*s + " " + 4*s + " v" + (17*s) +
        " h" + -8*s + " Z M" + -2*s + " " + -13*s + " v" + 4*s + " h" + 4*s,
      cls, parent,
    );

  /* Toe marks. A single shallow curve under a paw reads as a closed eye at
     this size, which is why the earlier version looked like a face on every
     foot. Two short upright strokes read as toes. */
  const toeMarks = (x, y, parent, cls) =>
    P("M" + (x - 2.2) + " " + y + " v3.2 M" + (x + 1.2) + " " + (y + 0.2) + " v3.2",
      cls || "lm-toes", parent);

  const PROPS = {
    scan: function (g) {
      /* A slim holo-inspector she carries in both paws: a bezel, a lit field,
         a list of file marks, and a sweep running down the screen. It is
         sized and placed to be *held* — the grip paws close over its lower
         corners and the forearms come up behind it from the shoulders. */
      const device = el("g", { class: "lm-prop-scan" }, g);
      el("rect", { x: -13, y: -10, width: 26, height: 18, rx: 4.6, class: "lm-holo-body" }, device);
      el("rect", { x: -10.6, y: -7.6, width: 21.2, height: 13.2, rx: 2.6, class: "lm-holo-field" }, device);
      /* Three marks standing for files on the screen. */
      P("M-8.4 -1.8 H-1.6 M-8.4 0.4 H-2.6 M-8.4 2.6 H-4", "lm-holo-lines", device);
      /* One file, drawn as a sheet with a folded corner. */
      const sheet = el("g", { transform: "translate(5 0.4)" }, device);
      fileTapPath(0.5, "lm-holo-sheet", sheet);
      el("rect", { x: -10.6, y: -7.6, width: 21.2, height: 13.2, rx: 2.6, class: "lm-holo-sweep" }, device);
      el("circle", { cx: 9.4, cy: 6.2, r: 0.9, class: "lm-holo-led" }, device);
    },
    discovering: function (g) {
      /* The finding spark: a real count arrived. */
      P(
        "M0 -13 L3 -3 L13 0 L3 3 L0 13 L-3 3 L-13 0 L-3 -3 Z",
        "lm-finding", g,
      );
    },
    major: function (g) {
      /* A large concentration: the same spark, but larger and circled, so
         the reaction reads as "this one is big" without implying danger. */
      const s = el("g", { class: "lm-prop-major" }, g);
      el("circle", { cx: 0, cy: 0, r: 10.5, class: "lm-major-ring" }, s);
      P(
        "M0 -15 L3.6 -3.6 L15 0 L3.6 3.6 L0 15 L-3.6 3.6 L-15 0 L-3.6 -3.6 Z",
        "lm-major-star", s,
      );
      P("M10 -10 L11.2 -7.2 L14 -6 L11.2 -4.8 L10 -2 L8.8 -4.8 L6 -6 L8.8 -7.2 Z",
        "lm-major-spark", s);
    },
    curious: function (g) {
      /* A tiny floating question mark rendered as geometry, not text —
         smaller and warmer than the failure hook. */
      const s = el("g", { class: "lm-prop-curious" }, g);
      P("M-2.5 -7 a3.2 3.2 0 1 1 3.2 3.2 V-2", "lm-curious-hook", s);
      el("circle", { cx: 0.7, cy: 2.2, r: 1.2, class: "lm-curious-dot" }, s);
    },
    thinking: function (g) {
      /* Three rising thought bubbles — drawn as circles, language-neutral. */
      const s = el("g", { class: "lm-prop-think" }, g);
      el("circle", { cx: -2, cy: 0,   r: 2.2, class: "lm-think-bubble lm-tb-1" }, s);
      el("circle", { cx:  2, cy: -5,  r: 3.0, class: "lm-think-bubble lm-tb-2" }, s);
      el("circle", { cx: -1, cy: -11, r: 4.2, class: "lm-think-bubble lm-tb-3" }, s);
    },
    review: function (g) {
      /* Attention badge: a stone-shaped droplet with a soft inner glint. */
      const s = el("g", { class: "lm-prop-review" }, g);
      P("M0 -10 L8 -3 L0 10 L-8 -3 Z", "lm-review-stone", s);
      P("M0 -5 L4.4 -2 L0 4.4 L-4.4 -2 Z", "lm-review-stone-core", s);
      el("circle", { cx: 0, cy: -9.3, r: 1.15, class: "lm-review-glint" }, s);
    },
    cleanup: function (g) {
      /* Two files she tosses one by one into the bin in front of her. */
      const s = el("g", { class: "lm-prop-sort" }, g);
      fileTapPath(0.92, "lm-chip lm-chip-1", s);
      fileTapPath(0.92, "lm-chip lm-chip-2", s);
      P("M-10.5 8 h21 l-2.6 5.6 h-15.8 Z", "lm-bin", s);
    },
    quarantine: function (g) {
      /* A capsule she presses shut; the ping is a stroke circle, never a
         filter glow. */
      const s = el("g", { class: "lm-prop-seal" }, g);
      el("rect", { x: -12, y: -11, width: 24, height: 22, rx: 5.5, class: "lm-seal-cap" }, s);
      el("circle", { cx: 0, cy: 0, r: 6.4, class: "lm-seal-disc" }, s);
      P("M-3 0.2 L-0.8 2.6 L3.2 -2.4", "lm-seal-tick", s);
      el("ellipse", { cx: 0, cy: 0, rx: 6.4, ry: 6.4, class: "lm-seal-ring" }, s);
    },
    success: function (g) {
      const s = el("g", { class: "lm-prop-sparkle" }, g);
      P("M0 -13 L2.5 -3.3 L12.5 0 L2.5 3.3 L0 13 L-2.5 3.3 L-12.5 0 L-2.5 -3.3 Z", "lm-spark lm-spark-a", s);
      P("M11 -11 L12.2 -8 L15.4 -7 L12.2 -6 L11 -3 L9.8 -6 L6.6 -7 L9.8 -8 Z",    "lm-spark lm-spark-b", s);
      P("M-12 -9 L-11.2 -6.4 L-8.6 -5.6 L-11.2 -4.8 L-12 -2.2 L-12.8 -4.8 L-15.4 -5.6 L-12.8 -6.4 Z", "lm-spark lm-spark-c", s);
    },
    failure: function (g) {
      /* A hook question: confused first, helpful after. Drawn as geometry. */
      P("M-4.5 -9.5 a5.2 5.2 0 1 1 5.2 5.2 V-2", "lm-quest", g);
      el("circle", { cx: 0.7, cy: 3.4, r: 1.7, class: "lm-quest-dot" }, g);
    },
    cancelled: function (g) {
      const s = el("g", { class: "lm-prop-pause" }, g);
      el("circle", { cx: 0, cy: -1, r: 8.6, class: "lm-pause-ring" }, s);
      el("rect", { x: -3,  y: -6.6, width: 2.4, height: 8.6, rx: 1.1, class: "lm-pause-bar" }, s);
      el("rect", { x: 0.6, y: -6.6, width: 2.4, height: 8.6, rx: 1.1, class: "lm-pause-bar" }, s);
    },
    sleeping: function (g) {
      /* Three z's, drawn as zigzag strokes: language-neutral, no <text>. */
      const z = el("g", { class: "lm-zzz" }, g);
      const draw = (x, y, s, per) => {
        P(
          "M" + (x - 4.4*s) + " " + (y - 4.4*s) +
            " h" + 8.8*s + " l" + -8.8*s + " " + 8.8*s + " h" + 8.8*s,
          "lm-z", z,
        );
        z.lastElementChild.style.animationDelay = per;
      };
      draw(-11, -7,  0.6, "-2.6s");
      draw(-2,  -13, 0.8, "-1.3s");
      draw( 8,  -20, 1.0, "0s");
    },
  };

  /* ---------- copy ------------------------------------------------------- */

  const KIND_LABEL = {
    idle:        "Idle",
    scan:        "Scanning",
    discovering: "Found something",
    major:       "Major find",
    curious:     "Curious",
    thinking:    "Thinking",
    review:      "Review",
    cleanup:     "Sorting",
    quarantine:  "Sealing",
    success:     "Reclaimed",
    failure:     "Needs a look",
    cancelled:   "Stopped",
    sleeping:    "Asleep",
  };
  const STATE_LABEL = { ...KIND_LABEL, discovering: "Discovering", curious: "Curious…", thinking: "Thinking…" };

  /* ---------- mount ------------------------------------------------------ */

  const hosts     = [];
  const instances = [];
  let paused  = false;
  let asleep  = false;
  /* Whether the companion is shown at all. The preference is the
     operator's, so it is stored; the interface listens for the change so the
     Overview can rebalance when she is hidden. */
  let visible = true;
  let current = "idle";

  try {
    paused = localStorage.getItem("cure-companion-paused") === "true";
  } catch (_) { /* storage is optional */ }

  const reducedMedia = matchMedia("(prefers-reduced-motion: reduce)");

  /* Build one rig per host (not a shared template) so gradient ids can be
     suffixed without affecting other mounts. */
  function buildRig(suffix) {
    const svg = el("svg", {
      viewBox:      "0 0 152 136",
      class:        "luma-svg",
      focusable:    "false",
      "aria-hidden":"true",
    });
    const defs = el("defs", null, svg);

    /* Fur: main radial from top-front, plus a rim light on the back edge. */
    const furGrad = el("radialGradient", {
      id: "dfur" + suffix,
      gradientUnits: "userSpaceOnUse",
      cx: "62", cy: "28", r: "132",
    }, defs);
    /* The stops reference the theme's custom properties rather than a
       literal colour, so the whole coat follows the palette instead of
       only the parts that are painted from CSS. */
    el("stop", { offset: "0%",   "stop-color": "var(--luma-fur-lit)"  }, furGrad);
    el("stop", { offset: "52%",  "stop-color": "var(--luma-fur)"      }, furGrad);
    el("stop", { offset: "100%", "stop-color": "var(--luma-fur-deep)" }, furGrad);

    /* Rim light: a warm arc along the back edge. It is a *stroke* on the
       back curve, not a second fill — the earlier version traced a path out
       and back along the same curve, which has zero area and therefore
       rendered nothing at all. */
    const rimGrad = el("linearGradient", {
      id: "drim" + suffix,
      gradientUnits: "userSpaceOnUse",
      x1: "104", y1: "58", x2: "110", y2: "126",
    }, defs);
    el("stop", { offset: "0%",   "stop-color": "var(--luma-fur-rim)", "stop-opacity": "0.85" }, rimGrad);
    el("stop", { offset: "100%", "stop-color": "var(--luma-fur-rim)", "stop-opacity": "0"    }, rimGrad);

    const irisGrad = el("radialGradient", { id: "diris" + suffix, cx: "46%", cy: "30%", r: "78%" }, defs);
    el("stop", { offset: "0%",   "stop-color": "var(--luma-eye)"      }, irisGrad);
    el("stop", { offset: "100%", "stop-color": "var(--luma-eye-deep)" }, irisGrad);

    /* Form shadow: the cool occlusion where the head sits on the shoulders
       and where the body turns away from the light. One gradient, used by
       two shapes, so the two shadows agree about where the light is. */
    const shadeGrad = el("radialGradient", {
      id: "dshade" + suffix,
      gradientUnits: "userSpaceOnUse",
      cx: "76", cy: "68", r: "58",
    }, defs);
    el("stop", { offset: "0%",   "stop-color": "var(--luma-shadow)", "stop-opacity": "0.5"  }, shadeGrad);
    el("stop", { offset: "62%",  "stop-color": "var(--luma-shadow)", "stop-opacity": "0.16" }, shadeGrad);
    el("stop", { offset: "100%", "stop-color": "var(--luma-shadow)", "stop-opacity": "0"    }, shadeGrad);

    /* Belly light: the soft bounce that lifts the underside of the coat. */
    const bellyGrad = el("radialGradient", {
      id: "dbelly" + suffix,
      gradientUnits: "userSpaceOnUse",
      cx: "76", cy: "116", r: "34",
    }, defs);
    el("stop", { offset: "0%",   "stop-color": "var(--luma-cream)", "stop-opacity": "0.85" }, bellyGrad);
    el("stop", { offset: "100%", "stop-color": "var(--luma-cream)", "stop-opacity": "0"    }, bellyGrad);

    /* The face mask: parked lids blink away above the skull silhouette. */
    const headClip = el("clipPath", { id: "dhead" + suffix }, defs);
    el("path", {
      d: "M76 20 C 60 20, 49 29, 48.6 44 C 48.2 57.5, 55.5 66, 76 66 C 96.5 66, 103.8 57.5, 103.4 44 C 103 29, 92 20, 76 20 Z",
    }, headClip);

    /* Ground */
    const ground = el("g", { class: "lm-ground" }, svg);
    el("ellipse", { cx: 76, cy: 125.4, rx: 31, ry: 3.8, class: "lm-shadow" }, ground);
    /* The curled cat is wider on the floor than the standing one, so it
       gets its own shadow. */
    el("ellipse", { cx: 76, cy: 125.4, rx: 40, ry: 4.2, class: "lm-shadow lm-shadow-sleep" }, ground);

    const root = el("g", { class: "lm-root" }, svg);
    /* Two poses in one rig: the standing cat and a genuinely curled sleeping
       cat. CSS cross-fades between them, so "sleeping" is a curled body with
       tucked paws and a wrapped tail rather than a sitting cat with closed
       eyes. */
    const awake = el("g", { class: "lm-awake" }, root);

    /* Tail: behind the body. Stroke widths taper 10.5 → 6, so the chain
       reads as one fluffy limb rather than three pills. */
    const tailBase = el("g", { class: "lm-j lm-tail-base" }, awake);
    el("path", {
      d: "M" + TAIL.base[0] + " " + TAIL.base[1] +
         " C " + (TAIL.base[0]+9) + " " + (TAIL.base[1]+1) +
         " "   + (TAIL.a[0]+1)    + " " + (TAIL.a[1]+9) +
         " "   + TAIL.a[0]        + " " + TAIL.a[1],
      class: "lm-tail-seg lm-tailseg-0",
      "stroke-width": 10.6, "stroke-linecap": "round", fill: "none",
    }, tailBase);
    const tail1 = el("g", { class: "lm-j lm-tail-1" }, tailBase);
    el("path", {
      d: "M" + TAIL.a[0] + " " + TAIL.a[1] +
         " C " + (TAIL.a[0]+0.5) + " " + (TAIL.a[1]-9) +
         " "   + (TAIL.b[0]+6.5) + " " + (TAIL.b[1]+3) +
         " "   + TAIL.b[0]       + " " + TAIL.b[1],
      class: "lm-tail-seg lm-tailseg-1",
      "stroke-width": 8.4, "stroke-linecap": "round", fill: "none",
    }, tail1);
    const tail2 = el("g", { class: "lm-j lm-tail-2" }, tail1);
    el("path", {
      d: "M" + TAIL.b[0] + " " + TAIL.b[1] +
         " C " + (TAIL.b[0]-5.5) + " " + (TAIL.b[1]-3.5) +
         " "   + (TAIL.tip[0]-0.5) + " " + (TAIL.tip[1]+7) +
         " "   + TAIL.tip[0] + " " + TAIL.tip[1],
      class: "lm-tail-seg lm-tailseg-2",
      "stroke-width": 6.2, "stroke-linecap": "round", fill: "none",
    }, tail2);
    /* Tip tuft */
    const tailTip = el("g", { class: "lm-j lm-tail-tip" }, tail2);
    P(
      "M" + (TAIL.tip[0]-1.2) + " " + (TAIL.tip[1]-8.6) +
        " q 3.4 -1.6 4.6 1.8 l -2.2 -0.6 l -0.8 3 l -2.4 -1.4 l -2.8 2 Z",
      "lm-tail-tuft", tailTip,
    );
    el("circle", { cx: TAIL.tip[0], cy: TAIL.tip[1], r: 5.2, class: "lm-tail-cap" }, tailTip);
    /* Tabby rings across the tail. Each band is a short dash laid squarely
       across the segment it sits on, and narrower than the segment, so the
       tail keeps its roundness instead of sprouting square planks. */
    el("path", { d: "M114.1 96.4 L118.4 101.1", class: "lm-tail-ring lm-ring-0" }, tailBase);
    el("path", { d: "M116.8 77.5 L121.4 75", class: "lm-tail-ring lm-ring-1" }, tail1);
    el("path", { d: "M106.5 64.9 L110.7 63.8", class: "lm-tail-ring lm-ring-2" }, tail2);

    /* Body: pear with real hips; the coat gradient carries most of the form. */
    const body = el("g", { class: "lm-j lm-body" }, awake);
    el("path", {
      d: "M76 61 C 63.5 61, 56 63.5, 52 70 C 46 79.5, 44 94, 47 106.5 C 49.8 118.5, 61 124.5, 76 124.5 C 91 124.5, 102.2 118.5, 105 106.5 C 108 94, 106 79.5, 100 70 C 96 63.5, 88.5 61, 76 61 Z",
      fill: "url(#dfur" + suffix + ")",
      class: "lm-body-shape",
    }, body);
    /* Rim light along the back edge. */
    el("path", {
      d: "M76 61 C 88.5 61, 96 63.5, 100 70 C 106 79.5, 108 94, 105 106.5 C 102.2 118.5, 91 124.5, 76 124.5",
      fill: "none",
      stroke: "url(#drim" + suffix + ")",
      "stroke-width": "3.2",
      "stroke-linecap": "round",
      class: "lm-body-rim",
    }, body);
    /* Belly shading */
    P(
      "M76 96 C 68 96, 63 108, 64.5 118 C 66.5 123, 71 124.5, 76 124.5 C 81 124.5, 85.5 123, 87.5 118 C 89 108, 84 96, 76 96 Z",
      "lm-belly", body,
    );
    /* Chest bib */
    P(
      "M76 61.5 C 70.5 61.5, 66.5 63.5, 65.5 68 C 64.5 76, 68.5 92, 76 92 C 83.5 92, 87.5 76, 86.5 68 C 85.5 63.5, 81.5 61.5, 76 61.5 Z",
      "lm-bib", body,
    );
    /* Tabby markings on the flanks. */
    const flank = el("g", { class: "lm-stripes" }, body);
    [
      "M48.6 75.5 C 52.4 77.6, 55.6 79.4, 58 81.6",
      "M46.4 87.5 C 50.4 89.6, 53.6 91.4, 56 93.6",
      "M47.2 99.5 C 51 101.6, 54 103.4, 56.4 105.4",
      "M103.4 75.5 C 99.6 77.6, 96.4 79.4, 94 81.6",
      "M105.6 87.5 C 101.6 89.6, 98.4 91.4, 96 93.6",
      "M104.8 99.5 C 101 101.6, 98 103.4, 95.6 105.4",
    ].forEach((d) => P(d, "lm-stripe", flank));

    /* Hind feet */
    const footL = el("g", { class: "lm-foot lm-foot-hind-l" }, body);
    el("ellipse", { cx: 52.5, cy: 118, rx: 8, ry: 5.6, class: "lm-hindpad" }, footL);
    toeMarks(51.4, 115.2, footL);
    const footR = el("g", { class: "lm-foot lm-foot-hind-r" }, body);
    el("ellipse", { cx: 99.5, cy: 118, rx: 8, ry: 5.6, class: "lm-hindpad" }, footR);
    toeMarks(98.4, 115.2, footR);

    /* Forelegs */
    const pawL = el("g", { class: "lm-j lm-paw lm-paw-l" }, body);
    el("rect",    { x: 55.5, y: 86, width: 9.4, height: 20, rx: 4.7, class: "lm-leg" }, pawL);
    el("rect",    { x: 55.5, y: 99, width: 9.4, height: 9, rx: 4.2, class: "lm-sock" }, pawL);
    el("ellipse", { cx: 60.2, cy: 107.4, rx: 7.6, ry: 5.2, class: "lm-paw-pad" }, pawL);
    toeMarks(59, 105, pawL);
    const pawR = el("g", { class: "lm-j lm-paw lm-paw-r" }, body);
    el("rect",    { x: 87.1, y: 86, width: 9.4, height: 20, rx: 4.7, class: "lm-leg" }, pawR);
    el("rect",    { x: 87.1, y: 99, width: 9.4, height: 9, rx: 4.2, class: "lm-sock" }, pawR);
    el("ellipse", { cx: 91.8, cy: 107.4, rx: 7.6, ry: 5.2, class: "lm-paw-pad" }, pawR);
    toeMarks(90.6, 105, pawR);

    /* Collar */
    const collar = el("g", { class: "lm-j lm-collar" }, body);
    P(
      "M58.5 63.5 C 64 66.5, 88 66.5, 93.5 63.5 C 95.6 66.4, 92.6 69.6, 76 69.6 C 59.4 69.6, 56.4 66.4, 58.5 63.5 Z",
      "lm-collar-band", collar,
    );
    P("M73.2 69.4 l 2.8 -3.4 l 2.8 3.4 l -2.8 3.4 Z", "lm-collar-tag", collar);
    el("circle", { cx: 76, cy: 66.2, r: 1.25, class: "lm-collar-led" }, collar);

    /* Head assembly */
    const head = el("g", { class: "lm-j lm-head" }, awake);

    /* Ears behind the skull */
    const earL = el("g", { class: "lm-j lm-ear lm-ear-l" }, head);
    P("M61.5 28 Q 53.5 13 52.3 8.4 Q 62.5 12.5 71.5 23 Z", "lm-ear-outer", earL);
    P("M60 25 Q 56 15.5 55.4 13 Q 62 16 66.9 22 Z", "lm-ear-inner", earL);
    const earR = el("g", { class: "lm-j lm-ear lm-ear-r" }, head);
    P("M90.5 28 Q 98.5 13 99.7 8.4 Q 89.5 12.5 80.5 23 Z", "lm-ear-outer", earR);
    P("M92 25 Q 96 15.5 96.6 13 Q 90 16 85.1 22 Z", "lm-ear-inner", earR);

    /* Skull */
    el("path", {
      d: "M76 20 C 60 20, 49 29, 48.6 44 C 48.2 57.5, 55.5 66, 76 66 C 96.5 66, 103.8 57.5, 103.4 44 C 103 29, 92 20, 76 20 Z",
      fill: "url(#dfur" + suffix + ")",
      class: "lm-skull",
    }, head);
    /* Rim light along the back of the skull. */
    el("path", {
      d: "M76 20 C 92 20, 103 29, 103.4 44 C 103.8 57.5, 96.5 66, 76 66",
      fill: "none",
      stroke: "url(#drim" + suffix + ")",
      "stroke-width": "2.6",
      "stroke-linecap": "round",
      class: "lm-skull-rim",
    }, head);
    /* Crown highlight */
    P(
      "M63 23.5 C 66.5 21.5, 71 20.6, 76 20.6 C 81 20.6, 85.5 21.5, 89 23.5 C 84.5 24.8, 80 25.6, 76 25.6 C 72 25.6, 67.5 24.8, 63 23.5 Z",
      "lm-crown", head,
    );
    /* Tabby markings: the classic forehead M, and cheek stripes. */
    const headMarks = el("g", { class: "lm-stripes" }, head);
    [
      "M69.4 21.8 C 70.6 26.2, 71.1 30.2, 70.5 33.8",
      "M76 20.4 C 76 25.4, 76 29.8, 76 33.6",
      "M82.6 21.8 C 81.4 26.2, 80.9 30.2, 81.5 33.8",
      "M62.6 25.6 C 65.8 28.2, 67.4 31.4, 67.9 35",
      "M89.4 25.6 C 86.2 28.2, 84.6 31.4, 84.1 35",
      "M53.4 36.8 C 55.8 38.2, 57.6 39.8, 58.8 41.6",
      "M52.8 43.2 C 55.2 44.6, 56.8 45.8, 57.8 47.4",
      "M98.6 36.8 C 96.2 38.2, 94.4 39.8, 93.2 41.6",
      "M99.2 43.2 C 96.8 44.6, 95.2 45.8, 94.2 47.4",
    ].forEach((d) => P(d, "lm-stripe", headMarks));

    /* Cheek fur */
    P("M49.4 40.6 q -3.2 0.6 -4.1 3.4 q 2.9 1.6 4.7 -0.8 Z",    "lm-cheek lm-cheek-l1", head);
    P("M49 50.2 q -3.4 1.4 -4 4.2 q 3.1 0.6 4.9 -1.9 Z",        "lm-cheek lm-cheek-l2", head);
    P("M102.6 40.6 q 3.2 0.6 4.1 3.4 q -2.9 1.6 -4.7 -0.8 Z",   "lm-cheek lm-cheek-r1", head);
    P("M103 50.2 q 3.4 1.4 4 4.2 q -3.1 0.6 -4.9 -1.9 Z",       "lm-cheek lm-cheek-r2", head);

    /* Eye sockets */
    el("ellipse", { cx: 63.5, cy: 45, rx: 10.4, ry: 11.2, class: "lm-socket" }, head);
    el("ellipse", { cx: 88.5, cy: 45, rx: 10.4, ry: 11.2, class: "lm-socket" }, head);

    /* Blush pads */
    el("ellipse", { cx: 57.5, cy: 56.5, rx: 4.4, ry: 2.6, class: "lm-blush lm-blush-l" }, head);
    el("ellipse", { cx: 94.5, cy: 56.5, rx: 4.4, ry: 2.6, class: "lm-blush lm-blush-r" }, head);

    /* Eyes (clipped to the skull) */
    const eyes = el("g", { class: "lm-eyes", "clip-path": "url(#dhead" + suffix + ")" }, head);
    const mkEye = (sign) => {
      const g = el("g", {
        class: "lm-eye",
        transform: "translate(" + (76 + sign*EYE.dx) + " " + HEAD.eyeY + ")",
      }, eyes);
      el("ellipse", { cx: 0, cy: 0, rx: EYE.r+1.7, ry: EYE.r+2.9, class: "lm-eye-rim" }, g);
      const iris = el("g", { class: "lm-iris" }, g);
      el("ellipse", { cx: 0, cy: 0, rx: EYE.r, ry: EYE.r+1.3,
        fill: "url(#diris" + suffix + ")", class: "lm-iris-fill" }, iris);
      /* Pupil: a <g> so its scale can be animated independently */
      const pupilG = el("g", { class: "lm-pupil-g" }, iris);
      el("ellipse", { cx: 0, cy: 0.4, rx: 2.55, ry: 4.9, class: "lm-pupil" }, pupilG);
      el("circle",  { cx: -2.5, cy: -2.9, r: 2.15, class: "lm-glint" }, iris);
      el("circle",  { cx: 2.3,  cy:  3.3, r: 0.95, class: "lm-glint lm-glint-rim" }, iris);
      P("M-3.4 5.2 Q 0 6.8 3.4 5.2", "lm-eye-wet", iris);
      /* Lid curtain. The earlier version was a plain rectangle, which was
         wider than the eye and produced a black visor across the face when
         the eye closed. This is an eye-shaped lid: it follows the rim and
         ends in a soft curve, so a closed eye reads as an eyelid. */
      const lid = el("g", { class: "lm-lid" }, g);
      el("path", {
        d: "M-9.9 -44 L 9.9 -44 L 9.9 3.6 C 9.9 8.4, 5.4 11.2, 0 11.2 C -5.4 11.2, -9.9 8.4, -9.9 3.6 Z",
        class: "lm-lid-fill",
      }, lid);
      P("M-8.6 2.4 C -4.6 7.4, 4.6 7.4, 8.6 2.4", "lm-lid-line", lid);
      return g;
    };
    mkEye(-1);
    mkEye(1);

    /* Brows */
    const browL = el("g", { class: "lm-j lm-brow lm-brow-l" }, head);
    el("path", {
      d: "M56.4 34.2 q 4.6 -2.4 8.4 -0.6 q -0.2 1 -0.6 1.4 q -3.8 -1.6 -7.2 0.6 Z",
      class: "lm-brow-shape",
    }, browL);
    const browR = el("g", { class: "lm-j lm-brow lm-brow-r" }, head);
    el("path", {
      d: "M87.2 33.6 q 3.8 -2.2 8.4 0.2 q -0.4 0.6 -0.6 1.2 q -4.4 -2.2 -8.2 -0.4 Z",
      class: "lm-brow-shape",
    }, browR);

    /* Muzzle */
    const muzzle = el("g", { class: "lm-j lm-muzzle" }, head);
    P(
      "M76 46.4 C 70 46.4, 66.4 48.8, 66.4 52.8 C 66.4 58.6, 70.6 63.4, 76 63.4 C 81.4 63.4, 85.6 58.6, 85.6 52.8 C 85.6 48.8, 82 46.4, 76 46.4 Z",
      "lm-muzzle-pad", muzzle,
    );
    const nose = el("g", { class: "lm-j lm-nose" }, muzzle);
    P(
      "M73.6 49.2 C 72.6 49.4, 72.3 50.2, 72.6 51 C 73.1 52.6, 74.5 53.8, 76 53.8 C 77.5 53.8, 78.9 52.6, 79.4 51 C 79.7 50.2, 79.4 49.4, 78.4 49.2 C 77 48.9, 75 48.9, 73.6 49.2 Z",
      "lm-nose-shape", nose,
    );
    P("M76 53.8 L76 55.1", "lm-philtrum", muzzle);
    P("M76 55.1 v1.6 M72.4 57 C 73.3 58.8, 75 58.9, 76 57.4 C 77 58.9, 78.7 58.8, 79.6 57", "lm-mouth", muzzle);
    P("M72.6 57.2 C 73.4 60.6, 78.6 60.6, 79.4 57.2 C 77.2 55.9, 74.8 55.9, 72.6 57.2 Z", "lm-mouth-fill", muzzle);
    P("M74.4 58.6 C 75 60.2, 77 60.2, 77.6 58.6 C 77 59.4, 75 59.4, 74.4 58.6 Z", "lm-tongue", muzzle);

    /* Whiskers */
    const whiskers = el("g", { class: "lm-whiskers" }, head);
    const whiskerLayout = [
      [62.5, 51.5, -15.5, -2.6],
      [62,   54.6, -16.5,  0.2],
      [63,   57.5, -14,    3  ],
      [89.5, 51.5,  15.5, -2.6],
      [90,   54.6,  16.5,  0.2],
      [89,   57.5,  14,    3  ],
    ];
    whiskerLayout.forEach(([x, y, dx, dy]) => {
      P(
        "M" + x + " " + y + " q " + dx/2 + " " + (dy/2 - 1.2) + " " + dx + " " + dy,
        "lm-whisker", whiskers,
      );
    });

    /* ---- the arms and the grip ------------------------------------------
       A prop that floats in front of her is not being *held*. Three
       foreground layers make the difference, and they are drawn in this
       order on purpose:

         1. the forearms  — behind the device, rising from the shoulders
         2. the device    — the state prop
         3. the grip paws — over the device, closing on its lower corners

       Everything is hidden in the states that hold nothing, and the standing
       rig's own forelegs step aside so there are never four legs. */

    const arms = el("g", { class: "lm-arms" }, root);
    const armL = el("g", { class: "lm-arm lm-arm-l" }, arms);
    el("path", {
      d: "M56 70 C 49 78, 50 88, 66 92",
      class: "lm-arm-limb",
      fill: "none",
      "stroke-width": "9.4",
      "stroke-linecap": "round",
    }, armL);
    const armR = el("g", { class: "lm-arm lm-arm-r" }, arms);
    el("path", {
      d: "M96 70 C 103 78, 102 88, 86 92",
      class: "lm-arm-limb",
      fill: "none",
      "stroke-width": "9.4",
      "stroke-linecap": "round",
    }, armR);

    /* Prop slot */
    const prop = el("g", { class: "lm-prop", transform: "translate(76 92)" }, root);

    /* The grip. Drawn *after* the prop, so the paws sit over the device:
       she is holding the inspector between her paws rather than wearing it.
       Hidden in every state that has no prop. */
    const hold = el("g", { class: "lm-hold" }, root);
    const gripL = el("g", { class: "lm-grip-paw lm-grip-l" }, hold);
    el("ellipse", { cx: 68, cy: 93, rx: 6.2, ry: 4.8, class: "lm-hold-paw" }, gripL);
    el("ellipse", { cx: 68.4, cy: 94.9, rx: 4.4, ry: 2.6, class: "lm-hold-pad" }, gripL);
    toeMarks(67.6, 90.4, gripL, "lm-hold-toes");
    const gripR = el("g", { class: "lm-grip-paw lm-grip-r" }, hold);
    el("ellipse", { cx: 84, cy: 93, rx: 6.2, ry: 4.8, class: "lm-hold-paw" }, gripR);
    el("ellipse", { cx: 83.6, cy: 94.9, rx: 4.4, ry: 2.6, class: "lm-hold-pad" }, gripR);
    toeMarks(84.4, 90.4, gripR, "lm-hold-toes");

    /* The paw that presses the seal shut. A separate limb, because it comes
       from above rather than from the side, and a separate layer, because
       the grip paws belong to the inspector and this one belongs to the
       capsule. */
    const press = el("g", { class: "lm-press-paw" }, root);
    el("ellipse", { cx: 0, cy: 0, rx: 6, ry: 4.6, class: "lm-hold-paw" }, press);
    el("ellipse", { cx: 0, cy: 1.8, rx: 4.2, ry: 2.4, class: "lm-hold-pad" }, press);
    toeMarks(0, -2.4, press, "lm-hold-toes");

    /* ---- the curled sleeping cat ----------------------------------------
       A sleeping cat is read as one continuous body, not as a head sitting
       beside a lump. The drawing below is a loaf lying on its side: the
       haunch rises at the right, the back arcs over the top, the head drops
       down onto the front paws at the left, and the tail comes round the
       near side and lies along the front, under the chin. */

    const sleep = el("g", { class: "lm-asleep" }, root);

    /* The body: one loaf from the chest at the left to the haunch at the
       right, sitting on the ground. */
    el("path", {
      d: "M38 106 C 38 88, 50 74, 70 73 C 92 72, 108 79, 114 94 C 119 107, 106 120, 84 121 C 60 122, 38 120, 38 106 Z",
      fill: "url(#dfur" + suffix + ")",
      class: "lm-sleep-body",
    }, sleep);
    /* Warm rim along the top of the curled back. */
    el("path", {
      d: "M40 103 C 41 87, 52 76, 70 75 C 91 74, 107 80, 113 93",
      fill: "none",
      stroke: "url(#drim" + suffix + ")",
      "stroke-width": "2.6",
      "stroke-linecap": "round",
      class: "lm-sleep-rim",
    }, sleep);
    /* Cream chest, showing between the chin and the body. */
    P(
      "M43 100 C 45 92, 55 88, 64 90 C 70 92, 72 100, 68 106 C 62 112, 50 112, 45 107 Z",
      "lm-sleep-belly",
      sleep,
    );
    /* Tabby rings across the curled back. */
    [
      "M58 74.4 C 60 79, 61 83, 61 87",
      "M74 72.6 C 76 77, 77 81, 77 85",
      "M90 76 C 92 80, 93 84, 93 88",
    ].forEach((d) => P(d, "lm-stripe", sleep));

    /* The tail. It leaves the haunch, comes down the near side and lies
       along the front of the body, under the chin, the way a cat actually
       wraps itself. It is drawn in the lit coat tone so it reads as a
       separate limb rather than part of the body. */
    const tailPath = "M104 82 C 115 97, 105 114, 86 118 C 70 120, 56 117, 48 110";
    el("path", {
      d: tailPath,
      fill: "none",
      stroke: "var(--luma-fur-edge)",
      "stroke-opacity": "0.5",
      "stroke-width": "10.6",
      "stroke-linecap": "round",
      class: "lm-sleep-tail-edge",
    }, sleep);
    el("path", {
      d: tailPath,
      fill: "none",
      stroke: "var(--luma-fur)",
      "stroke-width": "8.4",
      "stroke-linecap": "round",
      class: "lm-sleep-tail",
    }, sleep);
    /* A thin lit line along the top of the wrapped tail, the same way the
       standing tail is lit from the back. */
    el("path", {
      d: "M101 81 C 111 95, 101 110, 84 114.6",
      fill: "none",
      stroke: "var(--luma-fur-lit)",
      "stroke-width": "2.2",
      "stroke-linecap": "round",
      "stroke-opacity": "0.85",
      class: "lm-sleep-tail-hi",
    }, sleep);
    /* The cream tip, resting against the paws. */
    el("path", {
      d: "M50.4 113.2 C 48 109.8, 46.6 106.2, 46.4 102.6 C 49.6 104.2, 52.2 106.6, 53.8 109.6 C 53 111.2, 51.8 112.4, 50.4 113.2 Z",
      class: "lm-tail-cap",
    }, sleep);

    /* The haunch is drawn over the tail, so the tail leaves the body
       instead of starting in the middle of it with a blunt end. */
    el("ellipse", { cx: 96, cy: 96, rx: 19, ry: 18, class: "lm-sleep-haunch" }, sleep);

    /* The head casts a soft shadow onto the body it is resting against, so
       the two forms read as separate limbs rather than one lump. */
    el("ellipse", { cx: 66, cy: 100, rx: 24, ry: 15, class: "lm-sleep-occlusion" }, sleep);

    /* Head: down and forward, the cheek on the floor and the chin near the
       paws. It is drawn large, because a sleeping cat is read by its face. */
    const sleepHead = el("g", { class: "lm-sleep-head", transform: "rotate(-14 54 96)" }, sleep);
    /* The ear, folded back along the head the way a sleeping cat's is. */
    P("M38 76 Q 28 66, 26.6 60 Q 36 63, 44 72 Z", "lm-ear-outer", sleepHead);
    P("M37.2 74 Q 31.6 67, 31 63 Q 36.8 65, 40.4 71 Z", "lm-ear-inner", sleepHead);
    el("path", {
      d: "M48 68 C 34 68, 26 77.6, 26.6 89 C 27.2 99.6, 36 107.4, 48.4 107.4 C 60.8 107.4, 69.2 99.6, 69.8 89 C 70.4 77.6, 62 68, 48 68 Z",
      fill: "url(#dfur" + suffix + ")",
      class: "lm-skull",
    }, sleepHead);
    P("M37.4 71.4 C 39.8 74.6, 41.2 77.8, 41.8 81", "lm-stripe", sleepHead);
    P("M48 69.6 C 49.4 73.2, 50 76.6, 50 79.4", "lm-stripe", sleepHead);
    /* Closed eye: a single relaxed curve, not a lid rectangle. */
    P("M31.6 87 C 34.6 91.6, 41.4 91.6, 44.4 87", "lm-lid-line", sleepHead);
    /* Muzzle and a small pink nose. */
    P("M42.6 95 C 38.6 95, 36.2 96.8, 36.2 98.8 C 36.2 101.2, 38.4 102.8, 42.6 102.8 C 46.8 102.8, 49 101.2, 49 98.8 C 49 96.8, 46.6 95, 42.6 95 Z", "lm-muzzle-pad", sleepHead);
    P("M40.2 96.2 C 39.4 96.4, 39.2 97.2, 39.5 97.9 C 39.9 99.2, 41 100, 42.6 100 C 44.2 100, 45.3 99.2, 45.7 97.9 C 46 97.2, 45.8 96.4, 45 96.2 C 43.8 95.9, 41.4 95.9, 40.2 96.2 Z", "lm-nose-shape", sleepHead);
    /* The tech collar, still on, catching the light. */
    P("M62 84.6 C 65.4 88, 68.4 89.6, 71.6 89.6", "lm-collar-band", sleepHead);
    el("circle", { cx: 72.6, cy: 89.8, r: 1.2, class: "lm-collar-led" }, sleepHead);

    /* The front paws, tucked under the chin and stretched forward. Each has
       a furred wrist behind the white pad, so it reads as a paw rather than
       as an egg lying on the body. */
    const sleepPawL = el("g", { class: "lm-sleep-paw-grip" }, sleep);
    el("ellipse", { cx: 54.4, cy: 110, rx: 8.6, ry: 5, class: "lm-sleep-wrist" }, sleepPawL);
    el("ellipse", { cx: 56, cy: 111.4, rx: 7.6, ry: 4.4, class: "lm-sleep-paw" }, sleepPawL);
    toeMarks(54.4, 108.4, sleepPawL);
    const sleepPawR = el("g", { class: "lm-sleep-paw-grip" }, sleep);
    el("ellipse", { cx: 66.4, cy: 106.6, rx: 8.2, ry: 4.8, class: "lm-sleep-wrist" }, sleepPawR);
    el("ellipse", { cx: 68, cy: 108, rx: 7.2, ry: 4.2, class: "lm-sleep-paw" }, sleepPawR);
    toeMarks(66.4, 105, sleepPawR);


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

    /* Each mount gets its own SVG with unique gradient ids */
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

    /* One control, and it is about the companion rather than about
       housekeeping. The motion preference and the reduced-motion behaviour
       live in the display preferences, where they belong; the character area
       carries the character. */
    const controls = document.createElement("div");
    controls.className = "luma-controls";

    const hideBtn = document.createElement("button");
    hideBtn.type = "button";
    hideBtn.className = "luma-hide";
    hideBtn.textContent = "Hide";
    hideBtn.setAttribute("aria-label", "Hide the companion");

    controls.appendChild(hideBtn);

    /* The stage is a real control: it takes focus, it is announced, and it
       can be operated from the keyboard. The artwork itself is the hit
       region — clicking an empty corner of the stage does not count as
       petting her. */
    stage.setAttribute("role", "button");
    stage.setAttribute("tabindex", "0");
    stage.setAttribute("aria-label", "Pet LUMA");

    stage.addEventListener("click", (ev) => {
      if (paused || reducedMedia.matches) return;
      /* A click on the canvas but not on a painted part of the cat is not a
         pet. The SVG element itself is the background. */
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
      timer:  0,
      pulseTimer: 0,
      rng:   makeRng(hostIndexSeed()),
      state: "idle",
      /* Set when a host asks for its own persona. A global event then
         leaves this companion alone: the storage workstation's cat must not
         be told about a rescue finding on a screen she is not on. */
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

  function motionAllowed() {
    /* `document.hidden` tracks the browser tab, not the window. In a
       desktop shell a minimised window still reports as visible, so the
       window's own focus state is used as well: an unfocused window is not
       being looked at, and its animations should not keep running. */
    return !paused && !reducedMedia.matches && !document.hidden && !unfocused;
  }

  let unfocused = false;

  function syncMotion() {
    const off = paused || reducedMedia.matches || document.hidden || unfocused;
    document.documentElement.setAttribute("data-paused", String(off));
    for (const inst of instances) {
      inst.stage.setAttribute("data-paused", String(off));
      if (off) park(inst);
      else resume(inst);
    }
    if (window.CureSweep) window.CureSweep.motion(paused);
    if (window.CureStorageWorkstation) window.CureStorageWorkstation.redraw();
  }

  function park(inst) {
    /* The scripted beats are cancelled rather than paused: a paused
       animation is still in the document timeline, and a paused flick is
       not something worth resuming half-way. */
    for (const t of inst.tracks) t.cancel();
    inst.tracks = [];
    if (inst.timer) { clearTimeout(inst.timer); inst.timer = 0; }
  }
  function resume(inst) {
    if (inst.timer) clearTimeout(inst.timer);
    inst.timer = 0;
    scheduleNext(inst);
  }

  /* ---------- deterministic beat scheduler -------------------------------- */

  function beat(inst, tracks) {
    /* Tracks that have finished are released whatever they animated. The
       earlier code only released the ones that happened to target the same
       node, so a long session accumulated finished animations without
       bound. */
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

  /* ---------- idle beats -------------------------------------------------- */

  const EAR_FLICK_L = [
    { transform: "rotate(0deg)",                                offset: 0    },
    { transform: "rotate(-13deg) translateY(-1px)", offset: 0.28, easing: "cubic-bezier(.34,1.56,.64,1)" },
    { transform: "rotate(4deg)",                                offset: 0.6, easing: "cubic-bezier(.6,0,.4,1)" },
    { transform: "rotate(0deg)",                                offset: 1    },
  ];
  const flickL = () => [
    { sel: ".lm-ear-l", keyframes: EAR_FLICK_L, opts: { duration: 640, easing: "ease-out" } },
  ];
  const flickR = () => [
    { sel: ".lm-ear-r", keyframes: [
      { transform: "rotate(0deg)",                               offset: 0    },
      { transform: "rotate(12deg) translateY(-1px)", offset: 0.28, easing: "cubic-bezier(.34,1.56,.64,1)" },
      { transform: "rotate(-3deg)",                              offset: 0.6, easing: "cubic-bezier(.5,0,.5,1)" },
      { transform: "rotate(0deg)",                               offset: 1    },
    ], opts: { duration: 640, easing: "ease-out" } },
  ];
  const glance = (dir) => {
    const dx = dir * 2.6;
    return [
      { sel: ".lm-eyes", keyframes: [
        { transform: "translateX(0px)",    offset: 0    },
        { transform: "translateX(" + dx + "px)", offset: 0.35, easing: "cubic-bezier(.3,.9,.4,1)" },
        { transform: "translateX(" + dx + "px)", offset: 0.75 },
        { transform: "translateX(0px)",    offset: 1    },
      ], opts: { duration: 1500, easing: "ease-out" } },
      { sel: ".lm-head", keyframes: [
        { transform: "rotate(0deg)",                      offset: 0    },
        { transform: "rotate(" + dir*3.2 + "deg)", offset: 0.4, easing: "cubic-bezier(.25,.8,.35,1)" },
        { transform: "rotate(" + dir*3.2 + "deg)", offset: 0.8  },
        { transform: "rotate(0deg)",                      offset: 1    },
      ], opts: { duration: 1900, easing: "ease-in-out" } },
    ];
  };
  const tailFlick = () => [
    { sel: ".lm-tail-1", keyframes: [
      { transform: "rotate(0deg)",   offset: 0    },
      { transform: "rotate(-13deg)", offset: 0.3, easing: "cubic-bezier(.3,.7,.4,1)" },
      { transform: "rotate(5deg)",   offset: 0.65, easing: "cubic-bezier(.4,0,.6,1)" },
      { transform: "rotate(0deg)",   offset: 1    },
    ], opts: { duration: 1050, easing: "ease-in-out" } },
    { sel: ".lm-tail-tip", keyframes: [
      { transform: "rotate(0deg)",  offset: 0    },
      { transform: "rotate(15deg)", offset: 0.45, easing: "ease-out" },
      { transform: "rotate(-4deg)", offset: 0.75  },
      { transform: "rotate(0deg)",  offset: 1    },
    ], opts: { duration: 1200, easing: "ease-in-out" } },
  ];
  const softBob = () => [
    { sel: ".lm-head", keyframes: [
      { transform: "rotate(0deg) translateY(0px)",     offset: 0    },
      { transform: "rotate(1.6deg) translateY(-1px)",  offset: 0.4, easing: "ease-in-out" },
      { transform: "rotate(-1deg) translateY(0.3px)",  offset: 0.8  },
      { transform: "rotate(0deg) translateY(0px)",     offset: 1    },
    ], opts: { duration: 2600, easing: "ease-in-out" } },
  ];
  const noseTwitch = () => [
    { sel: ".lm-nose", keyframes: [
      { transform: "translateY(0)",      offset: 0    },
      { transform: "translateY(-0.7px)", offset: 0.3  },
      { transform: "translateY(0.5px)",  offset: 0.55 },
      { transform: "translateY(0)",      offset: 1    },
    ], opts: { duration: 420, easing: "ease-in-out" } },
    { sel: ".lm-whiskers", keyframes: [
      { transform: "rotate(0deg)",    offset: 0    },
      { transform: "rotate(1.1deg)",  offset: 0.35 },
      { transform: "rotate(-0.6deg)", offset: 0.7  },
      { transform: "rotate(0deg)",    offset: 1    },
    ], opts: { duration: 700, easing: "ease-in-out" } },
  ];
  const doubleBlink = () => [
    { sel: ".lm-eyes .lm-eye:first-child .lm-lid", keyframes: [
      { transform: "translateY(-38px)", offset: 0    },
      { transform: "translateY(-0.6px)",offset: 0.14 },
      { transform: "translateY(-38px)", offset: 0.34 },
      { transform: "translateY(-0.6px)",offset: 0.5  },
      { transform: "translateY(-38px)", offset: 0.72 },
      { transform: "translateY(-38px)", offset: 1    },
    ], opts: { duration: 860, easing: "ease-in-out" } },
    { sel: ".lm-eyes .lm-eye:last-child .lm-lid", keyframes: [
      { transform: "translateY(-38px)", offset: 0    },
      { transform: "translateY(-0.6px)",offset: 0.15 },
      { transform: "translateY(-38px)", offset: 0.34 },
      { transform: "translateY(-0.6px)",offset: 0.51 },
      { transform: "translateY(-38px)", offset: 0.72 },
      { transform: "translateY(-38px)", offset: 1    },
    ], opts: { duration: 860, easing: "ease-in-out" } },
  ];
  /* New: a slow drowsy half-lid droop for idle variety */
  const drowsyDroop = () => [
    { sel: ".lm-eyes .lm-eye:first-child .lm-lid", keyframes: [
      { transform: "translateY(-38px)",  offset: 0    },
      { transform: "translateY(-22px)",  offset: 0.3, easing: "ease-in" },
      { transform: "translateY(-22px)",  offset: 0.65 },
      { transform: "translateY(-38px)",  offset: 1,   easing: "ease-out" },
    ], opts: { duration: 2200, easing: "ease-in-out" } },
    { sel: ".lm-eyes .lm-eye:last-child .lm-lid", keyframes: [
      { transform: "translateY(-38px)",  offset: 0    },
      { transform: "translateY(-22px)",  offset: 0.32, easing: "ease-in" },
      { transform: "translateY(-22px)",  offset: 0.65 },
      { transform: "translateY(-38px)",  offset: 1,   easing: "ease-out" },
    ], opts: { duration: 2200, easing: "ease-in-out" } },
  ];
  /* New: whisker flare — widen briefly like she smelled something */
  const whiskerFlare = () => [
    { sel: ".lm-whiskers", keyframes: [
      { transform: "rotate(0deg) scale(1)",       offset: 0    },
      { transform: "rotate(-1.4deg) scale(1.06)", offset: 0.25, easing: "ease-out" },
      { transform: "rotate(0.6deg) scale(1.04)",  offset: 0.55 },
      { transform: "rotate(0deg) scale(1)",       offset: 1    },
    ], opts: { duration: 900, easing: "ease-in-out" } },
  ];

  /* Weighted, per-state beat libraries */
  const BEATS = {
    idle: [
      [flickL,       2],
      [flickR,       2],
      [() => glance(-1), 2],
      [() => glance(1),  2],
      [tailFlick,    1],
      [softBob,      1],
      [noseTwitch,   1],
      [doubleBlink,  1],
      [drowsyDroop,  1],
      [whiskerFlare, 1],
    ],
    scan: [
      [() => [{
        sel: ".lm-head",
        keyframes: [
          { transform: "rotate(-4deg) translateY(0)",    offset: 0    },
          { transform: "rotate(-6deg) translateY(-0.6px)", offset: 0.45, easing: "ease-in-out" },
          { transform: "rotate(-3.4deg) translateY(0.2px)", offset: 1  },
        ],
        opts: { duration: 2300, easing: "ease-in-out" },
      }], 2],
      [doubleBlink, 1],
      [() => [{
        sel: ".lm-ear-r",
        keyframes: [
          { transform: "rotate(0deg)",  offset: 0    },
          { transform: "rotate(9deg)",  offset: 0.3, easing: "ease-out" },
          { transform: "rotate(0deg)",  offset: 1    },
        ],
        opts: { duration: 520, easing: "ease-out" },
      }], 1],
    ],
    discovering: [
      [() => [{
        sel: ".lm-ear-l",
        keyframes: [
          { transform: "rotate(-6deg)",  offset: 0    },
          { transform: "rotate(-14deg) translateY(-1px)", offset: 0.35, easing: "cubic-bezier(.34,1.56,.64,1)" },
          { transform: "rotate(-9deg)",  offset: 0.7  },
          { transform: "rotate(-6deg)",  offset: 1    },
        ],
        opts: { duration: 900, easing: "ease-out" },
      }, {
        sel: ".lm-ear-r",
        keyframes: [
          { transform: "rotate(6deg)",  offset: 0    },
          { transform: "rotate(13deg) translateY(-1px)", offset: 0.4, easing: "cubic-bezier(.34,1.56,.64,1)" },
          { transform: "rotate(8deg)",  offset: 0.75 },
          { transform: "rotate(6deg)",  offset: 1    },
        ],
        opts: { duration: 940, easing: "ease-out" },
      }, {
        sel: ".lm-eyes",
        keyframes: [
          { transform: "translateX(0)",     offset: 0    },
          { transform: "translateX(1.4px)", offset: 0.5, easing: "ease-out" },
          { transform: "translateX(-0.8px)",offset: 0.78 },
          { transform: "translateX(0)",     offset: 1    },
        ],
        opts: { duration: 1300, easing: "ease-out" },
      }], 3],
      [doubleBlink, 1],
    ],
    major: [
      /* A startle, a double-take, then a proud little lean toward it. The
         order matters: she notices before she reacts. */
      [() => [{
        sel: ".lm-head",
        keyframes: [
          { transform: "rotate(2deg) translateY(0)",     offset: 0    },
          { transform: "rotate(9deg) translateY(-1.5px)", offset: 0.3, easing: "cubic-bezier(.34,1.56,.64,1)" },
          { transform: "rotate(4deg) translateY(-0.4px)", offset: 0.62 },
          { transform: "rotate(7deg) translateY(-0.8px)", offset: 1    },
        ],
        opts: { duration: 1500, easing: "ease-out" },
      }, {
        sel: ".lm-ear-l",
        keyframes: [
          { transform: "rotate(-6deg)",  offset: 0    },
          { transform: "rotate(-17deg) translateY(-1.4px)", offset: 0.28, easing: "cubic-bezier(.34,1.56,.64,1)" },
          { transform: "rotate(-10deg)", offset: 1    },
        ],
        opts: { duration: 1200, easing: "ease-out" },
      }, {
        sel: ".lm-ear-r",
        keyframes: [
          { transform: "rotate(6deg)",  offset: 0    },
          { transform: "rotate(17deg) translateY(-1.4px)", offset: 0.32, easing: "cubic-bezier(.34,1.56,.64,1)" },
          { transform: "rotate(10deg)", offset: 1    },
        ],
        opts: { duration: 1240, easing: "ease-out" },
      }, {
        sel: ".lm-eyes",
        keyframes: [
          { transform: "translateX(0)",      offset: 0    },
          { transform: "translateX(2.2px)",  offset: 0.26, easing: "ease-out" },
          { transform: "translateX(-1.6px)", offset: 0.6  },
          { transform: "translateX(0.8px)",  offset: 0.82 },
          { transform: "translateX(0)",      offset: 1    },
        ],
        opts: { duration: 1700, easing: "ease-in-out" },
      }], 3],
      [doubleBlink, 1],
      [tailFlick, 1],
    ],
    curious: [
      [() => [{
        sel: ".lm-head",
        keyframes: [
          { transform: "rotate(-9deg) translateY(0.5px)",  offset: 0    },
          { transform: "rotate(-12deg) translateY(0.5px)", offset: 0.5, easing: "ease-in-out" },
          { transform: "rotate(-9deg) translateY(0.5px)",  offset: 1    },
        ],
        opts: { duration: 2800, easing: "ease-in-out" },
      }], 2],
      [flickL, 1],
      [doubleBlink, 1],
    ],
    thinking: [
      [() => [{
        sel: ".lm-head",
        keyframes: [
          { transform: "rotate(-6deg) translateY(-0.8px)", offset: 0    },
          { transform: "rotate(-8deg) translateY(-1.2px)", offset: 0.5, easing: "ease-in-out" },
          { transform: "rotate(-6deg) translateY(-0.8px)", offset: 1    },
        ],
        opts: { duration: 3200, easing: "ease-in-out" },
      }], 2],
      [doubleBlink, 1],
      [() => [{
        sel: ".lm-paw-l",
        keyframes: [
          { transform: "translate(-3px, -5px) rotate(-8deg)",  offset: 0    },
          { transform: "translate(-3.5px, -5.5px) rotate(-10deg)", offset: 0.5, easing: "ease-in-out" },
          { transform: "translate(-3px, -5px) rotate(-8deg)",  offset: 1    },
        ],
        opts: { duration: 3200, easing: "ease-in-out" },
      }], 1],
    ],
    review: [
      [() => [{
        sel: ".lm-tail-tip",
        keyframes: [
          { transform: "rotate(0deg)",   offset: 0    },
          { transform: "rotate(-7deg)",  offset: 0.4, easing: "ease-in-out" },
          { transform: "rotate(2deg)",   offset: 0.8  },
          { transform: "rotate(0deg)",   offset: 1    },
        ],
        opts: { duration: 1500, easing: "ease-in-out" },
      }], 2],
      [doubleBlink, 1],
      [() => [{
        sel: ".lm-head",
        keyframes: [
          { transform: "rotate(-5deg) translateY(1px)",   offset: 0    },
          { transform: "rotate(-6.5deg) translateY(1px)", offset: 0.5, easing: "ease-in-out" },
          { transform: "rotate(-5deg) translateY(1px)",   offset: 1    },
        ],
        opts: { duration: 2600, easing: "ease-in-out" },
      }], 1],
    ],
    cleanup: [
      [tailFlick,   2],
      [doubleBlink, 1],
      [() => [{
        sel: ".lm-eyes",
        keyframes: [
          { transform: "translateX(0)",     offset: 0    },
          { transform: "translateX(-1.8px)",offset: 0.3, easing: "ease-out" },
          { transform: "translateX(1.8px)", offset: 0.65 },
          { transform: "translateX(0)",     offset: 1    },
        ],
        opts: { duration: 1900, easing: "ease-in-out" },
      }], 1],
    ],
    quarantine: [
      [doubleBlink, 1],
      [() => [{
        sel: ".lm-ear-l",
        keyframes: [
          { transform: "rotate(-3deg)", offset: 0    },
          { transform: "rotate(-8deg)", offset: 0.35, easing: "ease-out" },
          { transform: "rotate(-3deg)", offset: 1    },
        ],
        opts: { duration: 800, easing: "ease-out" },
      }, {
        sel: ".lm-ear-r",
        keyframes: [
          { transform: "rotate(3deg)", offset: 0    },
          { transform: "rotate(8deg)", offset: 0.38, easing: "ease-out" },
          { transform: "rotate(3deg)", offset: 1    },
        ],
        opts: { duration: 800, easing: "ease-out" },
      }], 1],
    ],
    success: [
      [tailFlick, 2],
      [() => [{
        sel: ".lm-ear-l",
        keyframes: [
          { transform: "rotate(-10deg)", offset: 0    },
          { transform: "rotate(-15deg)", offset: 0.4, easing: "cubic-bezier(.34,1.56,.64,1)" },
          { transform: "rotate(-10deg)", offset: 1    },
        ],
        opts: { duration: 700, easing: "ease-out" },
      }], 1],
    ],
    failure: [
      [() => [{
        sel: ".lm-head",
        keyframes: [
          { transform: "rotate(-7deg)", offset: 0    },
          { transform: "rotate(-4deg)", offset: 0.5, easing: "cubic-bezier(.4,0,.6,1)" },
          { transform: "rotate(-7deg)", offset: 1    },
        ],
        opts: { duration: 3000, easing: "ease-in-out" },
      }], 2],
      [doubleBlink, 1],
    ],
    cancelled: [
      [softBob,  2],
      [flickL,   1],
      [flickR,   1],
      [tailFlick,1],
    ],
    sleeping: [
      [() => [{
        sel: ".lm-ear-l",
        keyframes: [
          { transform: "rotate(9deg)",  offset: 0    },
          { transform: "rotate(12deg)", offset: 0.35, easing: "ease-out" },
          { transform: "rotate(9deg)",  offset: 1    },
        ],
        opts: { duration: 500, easing: "ease-out" },
      }], 1],
      [() => [{
        sel: ".lm-foot.lm-foot-hind-r",
        keyframes: [
          { transform: "translate(0,0)",         offset: 0    },
          { transform: "translate(0.7px,-1.1px)",offset: 0.4, easing: "ease-in-out" },
          { transform: "translate(0,0)",         offset: 1    },
        ],
        opts: { duration: 800, easing: "ease-in-out" },
      }], 1],
    ],
  };

  function pickBeat(inst) {
    const lib  = BEATS[inst.state] || BEATS.idle;
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
    const delay = 3400 + inst.rng() * 5200;
    inst.timer = setTimeout(() => {
      inst.timer = 0;
      if (!motionAllowed()) return;
      beat(inst, pickBeat(inst));
      scheduleNext(inst);
    }, delay);
  }

  /* ---------- state application ------------------------------------------ */

  function clearTracks(inst) {
    for (const t of inst.tracks) t.cancel();
    inst.tracks = [];
    if (inst.timer) { clearTimeout(inst.timer); inst.timer = 0; }
  }

  function applyState(inst, stateName) {
    const was = inst.state;
    inst.state = stateName;
    clearTracks(inst);
    const svg  = inst.svg;
    svg.setAttribute("data-state", stateName);
    /* The fold between the standing cat and the curled one is authored in
       two halves that never overlap: the standing cat is completely gone
       before the curled one starts to arrive, and the curled one is
       completely gone before the standing one starts to rise. The phase
       attribute is what lets the stylesheet sequence the two halves. */
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
    }, 980);
  }

  /* ---------- visibility and preferences --------------------------------- */

  /* Showing and hiding the companion is an interface decision, not a
     character decision: the application owns the space she occupies, so it
     is told when she leaves. */
  function setVisible(next) {
    visible = !!next;
    try {
      localStorage.setItem("cure-companion-visible", String(visible));
    } catch (_) { /* storage is optional */ }
    for (const inst of instances) {
      inst.host.hidden = !visible;
      inst.host.setAttribute("data-hidden", String(!visible));
      if (!visible) {
        /* A hidden companion costs nothing: no timers, no animations. */
        if (inst.timer) { clearTimeout(inst.timer); inst.timer = 0; }
        for (const track of inst.tracks) track.cancel();
        inst.tracks = [];
        if (inst.pulseTimer) { clearTimeout(inst.pulseTimer); inst.pulseTimer = 0; }
      } else if (motionAllowed()) {
        scheduleNext(inst);
      }
    }
    window.dispatchEvent(
      new CustomEvent("cure-companion-visibility", { detail: { visible } })
    );
  }

  function setMotionPaused(next) {
    paused = !!next;
    try {
      localStorage.setItem("cure-companion-paused", String(paused));
    } catch (_) { /* storage is optional */ }
    syncMotion();
  }

  /* ---------- public API -------------------------------------------------- */

  window.CureCompanion = {
    idle: function (detail) {
      asleep = false;
      apply("idle", null, detail || null);
    },
    discovering: function (count, detail) {
      if (!count) { apply("idle", null, detail || null); return; }
      asleep = false;
      apply("discovering", count + (count === 1 ? " finding" : " findings"), detail || null);
    },
    /* A notable clue: one finding, said out loud, rather than a count. */
    notice: function (detail) {
      asleep = false;
      apply("discovering", "New clue discovered", detail || null);
    },
    /* The single largest concentration in the measurement. This is a
       size statement, never a danger statement. */
    major: function (detail) {
      asleep = false;
      apply("major", "Large concentration", detail || null);
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
    wake: function () {
      asleep = false;
      apply("idle", null, null);
    },
    isAsleep: function ()  { return asleep;  },
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
    isPaused: function ()  { return paused;  },
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
        idle:        (detail)              => scopedApply("idle",       null,                              detail || null),
        scan:        (stateName, detail)   => scopedApply(stateName || "scan", null,                       detail || null),
        curious:     (detail)              => scopedApply("curious",    null,                              detail || null),
        thinking:    (detail)              => scopedApply("thinking",   null,                              detail || null),
        discovering: (count, detail)       => {
          if (!count) { scopedApply("idle", null, detail || null); return; }
          scopedApply("discovering", count + (count === 1 ? " finding" : " findings"), detail || null);
        },
        notice:      (detail)              => scopedApply("discovering", "New clue discovered",                     detail || null),
        major:       (detail)              => scopedApply("major",       "Large concentration",                    detail || null),
        review:      (count, detail)       => scopedApply("review",     count ? count + " to review" : "Review", detail || null),
        cleanup:     (stateName, title, detail) => scopedApply(stateName || "cleanup", title || null, detail || null),
        seal:        (detail)              => scopedApply("quarantine",  null,                              detail || null),
        failed:      (detail)              => scopedApply("failure",     null,                              detail || null),
        cancelled:   (detail)              => scopedApply("cancelled",   null,                              detail || null),
        sleep:       ()                    => scopedApply("sleeping",    null,                              null),
      };
    },
  };

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
      /* A host that asked for its own persona keeps it: a global event is
         not allowed to overwrite a screen-specific companion. */
      if (inst.scoped) continue;
      writeCopy(inst, label, title || text, detail || null);
      inst.host.setAttribute("data-state", stateName);
      inst.copy.setAttribute("role", "status");
      applyState(inst, stateName);
    }
  }

  /* ---------- teardown ---------------------------------------------------- */

  /* Remove one companion completely: cancel its animations, drop its
     timers, remove every listener it owns and empty its host. Without this
     a page that mounts a companion more than once leaks a rig, a timer and
     a set of listeners each time. */
  function destroy(inst) {
    if (!inst) return;
    clearTracks(inst);
    if (inst.timer) { clearTimeout(inst.timer); inst.timer = 0; }
    if (inst.pulseTimer) { clearTimeout(inst.pulseTimer); inst.pulseTimer = 0; }
    for (const off of inst.listeners || []) {
      try { off(); } catch (_) { /* best effort */ }
    }
    inst.listeners = [];
    const at = instances.indexOf(inst);
    if (at >= 0) instances.splice(at, 1);
    try { inst.host.replaceChildren(); } catch (_) { /* best effort */ }
  }

  /* ---------- wiring ------------------------------------------------------ */

  function wire() {
    for (const hostEl of document.querySelectorAll("[data-companion]")) {
      hosts.push(hostEl);
    }
    for (const host of hosts) {
      const inst = mount(host);

      inst.hideBtn.addEventListener("click", () => {
        setVisible(false);
      });
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
