/**
 * Cyber-Cat Companion Mascot Controller
 * Renders geometric vector Cyber-Cat SVG with 4 reactive states:
 * 1. idle: sitting calmly next to green shield, soft blink & tail sway
 * 2. scanning: crouched hunter mode, ears twitching, cyan eyes darting with laser sweep
 * 3. threat-found: alert arched posture, crimson eyes, exclamation badge (!)
 * 4. deleting: paw-swat slapping red threat cube into vortex!
 */
(function () {
  "use strict";

  const CAT_SVG = `
    <svg class="cyber-cat cat-idle" viewBox="0 0 90 70" width="100%" height="100%" fill="none" xmlns="http://www.w3.org/2000/svg">
      <defs>
        <filter id="catGlow" x="-30%" y="-30%" width="160%" height="160%">
          <feGaussianBlur stdDeviation="1.5" result="blur" />
          <feComposite in="SourceGraphic" in2="blur" operator="over" />
        </filter>
        <linearGradient id="shieldGrad" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="#10b981" stop-opacity="0.35" />
          <stop offset="100%" stop-color="#047857" stop-opacity="0.1" />
        </linearGradient>
      </defs>

      <!-- Emerald Defense Shield (Left) -->
      <g class="cat-shield">
        <path d="M10 26 C10 38, 19 46, 19 46 C19 46, 28 38, 28 26 L28 17 L19 14 L10 17 Z" fill="url(#shieldGrad)" stroke="#10b981" stroke-width="1.4" stroke-linejoin="round" />
        <path d="M19 21 L19 39 M14 28 L24 28" stroke="#34d399" stroke-width="1.2" stroke-linecap="round" opacity="0.75" />
      </g>

      <!-- Tail -->
      <g class="cat-tail-group">
        <path class="cat-tail" d="M18 48 C12 42, 6 29, 12 21 C15 17, 20 20, 19 25 C17 31, 20 40, 23 48 Z" fill="#0f172a" stroke="#38bdf8" stroke-width="1.4" stroke-linecap="round" />
      </g>

      <!-- Cat Body -->
      <path class="cat-body" d="M22 48 C22 36, 32 30, 44 30 C53 30, 60 36, 61 48 C61 54, 55 56, 44 56 C30 56, 22 54, 22 48 Z" fill="#090d16" stroke="#334155" stroke-width="1.4" />

      <!-- Cyber Collar / Nodes -->
      <path class="cat-collar" d="M41 35 C48 35, 54 38, 58 43" stroke="#818cf8" stroke-width="1.6" stroke-linecap="round" />
      <circle class="cat-collar-node" cx="49" cy="39" r="1.8" fill="#22d3ee" filter="url(#catGlow)" />

      <!-- Head Group -->
      <g class="cat-head-group">
        <path class="cat-head" d="M42 20 C42 12, 47 11, 53 11 C59 11, 64 12, 64 20 C65 27, 62 30, 53 30 C43 30, 41 27, 42 20 Z" fill="#090d16" stroke="#334155" stroke-width="1.4" />
        <!-- Ears -->
        <path class="cat-ear-l" d="M43 14 L38 4 L48 10 Z" fill="#0f172a" stroke="#38bdf8" stroke-width="1.4" stroke-linejoin="round" />
        <path class="cat-ear-l-inner" d="M42.5 12 L39.5 6 L46 9 Z" fill="#818cf8" opacity="0.9" />
        <path class="cat-ear-r" d="M57 10 L67 4 L62 14 Z" fill="#0f172a" stroke="#38bdf8" stroke-width="1.4" stroke-linejoin="round" />
        <path class="cat-ear-r-inner" d="M59 9 L65.5 6 L62.5 12 Z" fill="#818cf8" opacity="0.9" />

        <!-- Glowing Cyber Eyes -->
        <g class="cat-eyes">
          <ellipse class="cat-eye cat-eye-l" cx="48" cy="19" rx="2.4" ry="2.6" fill="#00f0ff" filter="url(#catGlow)" />
          <ellipse class="cat-eye cat-eye-r" cx="58" cy="19" rx="2.4" ry="2.6" fill="#00f0ff" filter="url(#catGlow)" />
          <circle class="cat-pupil-l" cx="48" cy="19" r="0.9" fill="#ffffff" />
          <circle class="cat-pupil-r" cx="58" cy="19" r="0.9" fill="#ffffff" />
        </g>

        <!-- Whiskers -->
        <path class="cat-whiskers" d="M37 21 L29 19 M37 23 L30 25 M68 21 L76 19 M68 23 L75 25" stroke="#38bdf8" stroke-width="0.9" opacity="0.65" stroke-linecap="round" />
      </g>

      <!-- Paws -->
      <g class="cat-paw-group">
        <ellipse class="cat-paw-back" cx="35" cy="54" rx="4.5" ry="3.2" fill="#1e293b" stroke="#38bdf8" stroke-width="1" />
        <ellipse class="cat-paw-front" cx="53" cy="54" rx="5" ry="3.5" fill="#1e293b" stroke="#00f0ff" stroke-width="1.3" />
      </g>

      <!-- Threat Alert Badge (!) -->
      <g class="cat-alert-badge">
        <circle cx="68" cy="8" r="7.5" fill="#f43f5e" stroke="#ffe4e6" stroke-width="1.2" filter="url(#catGlow)" />
        <text x="68" y="11.5" font-family="system-ui, -apple-system, sans-serif" font-size="10" font-weight="900" fill="#ffffff" text-anchor="middle">!</text>
      </g>

      <!-- Red Malicious File Cube (Target for Paw Swat) -->
      <g class="cat-virus-cube">
        <rect x="58" y="42" width="12" height="12" rx="2.5" fill="#ef4444" stroke="#fecaca" stroke-width="1.4" filter="url(#catGlow)" />
        <path d="M61 48 L67 48 M64 45 L64 51" stroke="#ffffff" stroke-width="1.2" stroke-linecap="round" />
      </g>

      <!-- Spinning Trash Vortex -->
      <g class="cat-vortex">
        <circle cx="76" cy="50" r="8" stroke="#f43f5e" stroke-width="1.5" stroke-dasharray="5,4" fill="none" opacity="0.75" />
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
    cat.classList.remove("cat-idle", "cat-scanning", "cat-threat-found", "cat-deleting", "cat-clean", "cat-moving");
    cat.classList.add("cat-" + (state || "idle"));
  }

  function swat(target, callback) {
    if (!target) {
      if (callback) callback();
      return;
    }
    setState(target, "deleting");
    setTimeout(function () {
      setState(target, "threat-found");
      if (callback) callback();
    }, 750);
  }

  window.CureCat = {
    render: render,
    setState: setState,
    swat: swat,
    CAT_SVG: CAT_SVG,
  };
})();
