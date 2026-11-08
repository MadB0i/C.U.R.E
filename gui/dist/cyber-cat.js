/**
 * Cyber-Cat Companion Mascot
 * Reusable helper that creates and manages the Cyber-Cat SVG across C.U.R.E
 */
(function () {
  const CAT_SVG = `
    <svg class="cyber-cat cat-idle" viewBox="0 0 54 48" width="52" height="48" fill="none" xmlns="http://www.w3.org/2000/svg">
      <defs>
        <filter id="catGlow" x="-20%" y="-20%" width="140%" height="140%">
          <feGaussianBlur stdDeviation="1.2" result="blur" />
          <feComposite in="SourceGraphic" in2="blur" operator="over" />
        </filter>
      </defs>
      <!-- Tail -->
      <g class="cat-tail-group">
        <path class="cat-tail" d="M12 36 C8 32, 4 23, 8 17 C10 14, 14 16, 13 20 C12 24, 14 31, 16 36 Z" fill="#131b2e" stroke="#38bdf8" stroke-width="1.2" stroke-linecap="round" />
      </g>
      <!-- Cat Body -->
      <path class="cat-body" d="M14 36 C14 27, 21 22, 30 22 C37 22, 42 27, 43 36 C43 40, 39 42, 30 42 C20 42, 14 40, 14 36 Z" fill="#0f172a" stroke="#475569" stroke-width="1.2" />
      <!-- Cyber Collar / Nodes -->
      <path class="cat-collar" d="M28 26 C33 26, 38 28, 41 32" stroke="#818cf8" stroke-width="1.5" stroke-linecap="round" />
      <circle class="cat-collar-node" cx="34" cy="29" r="1.5" fill="#22d3ee" filter="url(#catGlow)" />
      <!-- Head Group -->
      <g class="cat-head-group">
        <path class="cat-head" d="M30 14 C30 9, 34 8, 38 8 C42 8, 46 9, 46 14 C47 19, 45 22, 38 22 C31 22, 29 19, 30 14 Z" fill="#0f172a" stroke="#475569" stroke-width="1.2" />
        <!-- Ears -->
        <path class="cat-ear-l" d="M31 10 L27 2 L35 7 Z" fill="#0f172a" stroke="#38bdf8" stroke-width="1.2" />
        <path class="cat-ear-l-inner" d="M30.5 8 L28.5 4 L33 6.5 Z" fill="#818cf8" opacity="0.85" />
        <path class="cat-ear-r" d="M41 7 L49 2 L45 10 Z" fill="#0f172a" stroke="#38bdf8" stroke-width="1.2" />
        <path class="cat-ear-r-inner" d="M43 6.5 L47.5 4 L45.5 8 Z" fill="#818cf8" opacity="0.85" />
        <!-- Glowing Cyber Eyes -->
        <g class="cat-eyes">
          <ellipse class="cat-eye cat-eye-l" cx="34" cy="14" rx="2" ry="2.2" fill="#22d3ee" filter="url(#catGlow)" />
          <ellipse class="cat-eye cat-eye-r" cx="42" cy="14" rx="2" ry="2.2" fill="#22d3ee" filter="url(#catGlow)" />
          <circle class="cat-pupil-l" cx="34" cy="14" r="0.8" fill="#ffffff" />
          <circle class="cat-pupil-r" cx="42" cy="14" r="0.8" fill="#ffffff" />
        </g>
        <!-- Whiskers -->
        <path class="cat-whiskers" d="M26 15 L20 14 M26 17 L21 18 M50 15 L56 14 M50 17 L55 18" stroke="#38bdf8" stroke-width="0.8" opacity="0.6" stroke-linecap="round" />
      </g>
      <!-- Paws -->
      <g class="cat-paw-group">
        <ellipse class="cat-paw-back" cx="24" cy="41" rx="3.5" ry="2.5" fill="#1e293b" stroke="#38bdf8" stroke-width="0.8" />
        <ellipse class="cat-paw-front" cx="38" cy="41" rx="4" ry="2.8" fill="#1e293b" stroke="#22d3ee" stroke-width="1" />
      </g>
      <!-- Moving/Deleting Glowing Cube -->
      <g class="cat-file-cube">
        <rect x="44" y="32" width="9" height="9" rx="1.5" fill="rgba(34, 211, 238, 0.25)" stroke="#22d3ee" stroke-width="1.2" />
        <path d="M46 35 L51 35 M46 38 L49 38" stroke="#38bdf8" stroke-width="0.8" stroke-linecap="round" />
      </g>
    </svg>
  `;

  function render(container, state) {
    if (!container) return null;
    let wrapper = container.querySelector(".cyber-cat-wrapper");
    if (!wrapper) {
      wrapper = document.createElement("div");
      wrapper.className = "cyber-cat-wrapper";
      wrapper.innerHTML = CAT_SVG.trim();
      container.appendChild(wrapper);
    }
    setState(wrapper, state || "idle");
    return wrapper;
  }

  function setState(target, state) {
    if (!target) return;
    const cat = target.classList && target.classList.contains("cyber-cat")
      ? target
      : target.querySelector ? target.querySelector(".cyber-cat") : null;
    if (!cat) return;
    cat.classList.remove("cat-idle", "cat-scanning", "cat-moving", "cat-deleting", "cat-clean");
    cat.classList.add("cat-" + (state || "idle"));
  }

  function swat(target, callback) {
    if (!target) {
      if (callback) callback();
      return;
    }
    setState(target, "deleting");
    setTimeout(function () {
      setState(target, "idle");
      if (callback) callback();
    }, 550);
  }

  window.CureCat = {
    render: render,
    setState: setState,
    swat: swat,
    CAT_SVG: CAT_SVG,
  };
})();
