/* Original offline SVG artwork. This presentation layer never invokes a command. */
(function () {
  "use strict";
  const art = `<svg class="companion-art" aria-hidden="true" focusable="false" viewBox="0 0 240 200">
    <path class="luma-ground" d="M20 181H221M32 185H72M187 185H210"/>
    <g class="luma-body">
      <path class="luma-hair" d="M78 59Q78 25 114 26Q148 27 150 66L154 118Q140 129 123 120L78 120Q67 109 74 88Z"/>
      <path class="luma-jacket" d="M93 120Q73 122 67 147L64 178H164L158 142Q152 125 129 120Z"/>
      <path class="luma-skin" d="M101 103H123L126 123Q111 137 98 122Z"/>
      <path class="luma-shade" d="M101 103H123V113Q113 120 101 112Z"/>
      <path class="luma-lapel" d="M94 121L110 137L102 156L88 126ZM129 121L114 137L123 155L137 127Z"/>
      <path class="luma-ground" d="M111 140V177M80 146L77 173M144 145L150 174"/>
      <g class="luma-head">
        <path class="luma-skin" d="M83 63Q82 42 111 40Q140 41 141 69L138 88Q132 111 112 115Q91 111 85 89Z"/>
        <ellipse class="luma-skin" cx="83" cy="80" rx="5" ry="9"/><ellipse class="luma-skin" cx="140" cy="81" rx="4" ry="8"/>
        <path class="luma-hair" d="M80 73Q72 43 93 32Q128 16 145 49L143 76L133 65L128 44Q115 60 86 62L85 85L78 96Z"/>
        <path class="luma-hair-detail" d="M87 45Q100 31 120 34M86 54Q108 54 123 40M139 54L144 108"/>
        <path class="luma-lapel" d="M135 48L139 47L143 61L139 62Z"/>
        <g class="luma-eyes">
          <path class="luma-face-line" d="M91 76Q97 71 103 76M120 76Q126 71 132 76"/>
          <g class="luma-pupils"><ellipse class="luma-eye" cx="98" cy="78" rx="2.8" ry="4"/><ellipse class="luma-eye" cx="126" cy="78" rx="2.8" ry="4"/>
          <circle fill="var(--companion-skin)" cx="99" cy="77" r="0.9"/><circle fill="var(--companion-skin)" cx="127" cy="77" r="0.9"/></g>
        </g>
        <path class="luma-face-line" opacity=".5" d="M93 67L102 66M122 66L131 68M112 82L110 89L114 90"/>
        <path class="luma-face-line luma-neutral-mouth" d="M105 98Q112 102 119 97"/>
        <path class="luma-face-line luma-satisfied" d="M103 97Q112 108 122 96"/>
        <g class="luma-concern"><path class="luma-face-line" d="M106 100Q112 96 119 100M93 69L102 66M122 66L130 69"/></g>
      </g>
      <g class="luma-tablet-group">
        <rect class="luma-tablet" x="141" y="131" width="61" height="43" rx="6" transform="rotate(-7 171 152)"/>
        <path class="luma-ground" d="M150 144L191 139M152 166L191 161"/>
        <path class="luma-screen-line luma-track" d="M152 151L164 149L169 155L179 145L191 146"/>
        <path class="luma-skin" d="M133 149Q147 140 153 147L159 157Q160 162 153 163L138 159Z"/>
      </g>
      <g class="luma-cleaning">
        <g class="luma-paper"><rect class="luma-tablet" x="180" y="160" width="18" height="13" rx="2"/><path class="luma-ground" d="M184 165H194M184 169H191"/></g>
        <g class="luma-paper second"><rect class="luma-tablet" x="207" y="151" width="14" height="18" rx="2"/><path class="luma-ground" d="M211 157H217M211 162H217"/></g>
        <g class="luma-brush"><path class="luma-skin" d="M133 144Q145 142 156 148L162 157Q159 163 153 160L139 155Z"/>
          <path d="M156 154L182 169" stroke="var(--accent)" stroke-width="7" stroke-linecap="round"/>
          <path fill="var(--companion-hair-light)" d="M177 164L185 168L180 179L167 176Z"/>
          <path class="luma-ground" d="M171 172L176 175M175 169L180 173"/>
        </g>
      </g>
    </g>
  </svg>`;
  const stages = [...document.querySelectorAll("[data-companion]")];
  const reduced = matchMedia("(prefers-reduced-motion: reduce)");
  let paused = false;
  try {
    paused = localStorage.getItem("cure-companion-paused") === "true";
  } catch (_) {
    /* preference storage is optional */
  }
  function syncMotion() {
    stages.forEach((stage) => {
      stage.dataset.paused = String(
        paused || reduced.matches || document.hidden,
      );
      const button = stage.querySelector(".companion-motion");
      button.textContent = paused ? "Resume motion" : "Pause motion";
      button.setAttribute("aria-label", button.textContent + " for Luma");
      button.setAttribute("aria-pressed", String(paused));
    });
  }
  stages.forEach((stage) => {
    // Only authored artwork is HTML. Backend strings below always use textContent.
    stage.innerHTML =
      art +
      `<div class="companion-copy"><div class="companion-name">Luma · C.U.R.E companion</div><div class="companion-title"></div><p class="companion-detail"></p><span class="companion-reduced">Reduced motion</span></div><button class="btn companion-motion" type="button" aria-label="Pause assistant motion" aria-pressed="false">Pause motion</button>`;
    stage.querySelector("button").addEventListener("click", () => {
      paused = !paused;
      try {
        localStorage.setItem("cure-companion-paused", String(paused));
      } catch (_) {
        /* optional */
      }
      syncMotion();
    });
  });
  function set(key, state, title, detail = "") {
    const stage = stages.find((e) => e.dataset.companion === key);
    if (!stage) return;
    stage.dataset.state = state;
    stage.querySelector(".companion-title").textContent = title;
    stage.querySelector(".companion-detail").textContent = detail;
  }
  window.CureCompanion = {
    overview(state, title) {
      set("overview", state, title);
    },
    scan(state, detail, sourceIndex = 0) {
      set(
        "scan",
        state,
        state === "review"
          ? "Collection interrupted"
          : state === "idle"
            ? "Collection ended"
            : "Following the evidence",
        detail,
      );
      const stage = stages.find((e) => e.dataset.companion === "scan");
      stage.style.setProperty("--look-x", `${(sourceIndex % 3) - 1}px`);
      stage.style.setProperty(
        "--look-angle",
        `${(sourceIndex % 3) * 2 - 2}deg`,
      );
    },
    result(state, title, detail) {
      set("result", state, title, detail);
    },
    cleanup(state, title, detail) {
      set("cleanup", state, title, detail);
    },
  };
  set("overview", "idle", "Ready when you are");
  set(
    "rescue",
    "idle",
    "Ready for inspection",
    "Local collection. Your decisions.",
  );
  set("scan", "scan", "Following the evidence", "Awaiting collection");
  set(
    "cleanup",
    "idle",
    "Ready to measure",
    "Review the selection before any files are deleted.",
  );
  set("result", "idle", "Evidence first", "Run a scan to begin.");
  reduced.addEventListener("change", syncMotion);
  document.addEventListener("visibilitychange", syncMotion);
  syncMotion();
})();
