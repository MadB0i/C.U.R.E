#!/usr/bin/env python3
"""Apply comprehensive futuristic UI polish to C.U.R.E.
Updates luma.js, luma.css, agent-radar.js, agent.css, index.html, and style.css.
"""

from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
DIST = ROOT / "gui" / "dist"

# ==============================================================================
# 1. LUMA V2 HOLOGRAPHIC VISOR (luma.js & luma.css)
# ==============================================================================
luma_js_path = DIST / "luma.js"
luma_js = luma_js_path.read_text(encoding="utf-8")

visor_svg_code = """
    /* Holographic Visor HUD (Active in Scanning State) */
    const visor = el("g", { class: "lm-holo-visor" }, head);
    P("M 51 43.5 Q 76 39 101 43.5 L 99 48.5 Q 76 44 53 48.5 Z", "lm-visor-glass", visor);
    P("M 52 46 Q 76 41.5 100 46", "lm-visor-scanline", visor);
    el("circle", { cx: 53.5, cy: 46, r: 1.5, class: "lm-visor-node" }, visor);
    el("circle", { cx: 98.5, cy: 46, r: 1.5, class: "lm-visor-node" }, visor);
"""

if "lm-holo-visor" not in luma_js:
    # Insert visor right after whiskers in head
    target = '    const whiskers = el("g", { class: "lm-whiskers" }, head);'
    assert target in luma_js, "Whiskers target not found in luma.js"
    luma_js = luma_js.replace(target, visor_svg_code + "\n" + target)
    luma_js_path.write_text(luma_js, encoding="utf-8")
    print("Updated luma.js with holographic visor")

luma_css_path = DIST / "luma.css"
luma_css = luma_css_path.read_text(encoding="utf-8")

visor_css_code = """
/* Holographic Visor HUD (Active in Scanning State) */
.lm-holo-visor {
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.3s ease;
}
.luma-svg[data-state="scan"] .lm-holo-visor {
  opacity: 1;
}
.lm-visor-glass {
  fill: rgba(34, 211, 238, 0.22);
  stroke: rgba(167, 139, 250, 0.7);
  stroke-width: 0.8;
  filter: drop-shadow(0 0 5px rgba(34, 211, 238, 0.5));
}
.lm-visor-scanline {
  stroke: #22d3ee;
  stroke-width: 0.7;
  stroke-dasharray: 4 2;
  animation: lm-visor-pulse 1.4s ease-in-out infinite;
}
.lm-visor-node {
  fill: #22d3ee;
}
@keyframes lm-visor-pulse {
  0%, 100% { opacity: 0.45; }
  50% { opacity: 1; }
}
@media (prefers-reduced-motion: reduce) {
  .lm-visor-scanline { animation: none; }
}
"""

if "lm-holo-visor" not in luma_css:
    luma_css += "\n" + visor_css_code
    luma_css_path.write_text(luma_css, encoding="utf-8")
    print("Updated luma.css with holographic visor styles")

# ==============================================================================
# 2. AGENT RADAR V3: Wire up #ar-agent-inspector (agent-radar.js & agent.css)
# ==============================================================================
agent_js_path = DIST / "agent-radar.js"
agent_js = agent_js_path.read_text(encoding="utf-8")

inspector_handler = """
  function openAgentInspector(agent, proc) {
    const dlg = document.getElementById("ar-agent-inspector");
    const title = document.getElementById("ar-insp-title");
    const body = document.getElementById("ar-insp-body");
    const closeBtn = document.getElementById("ar-insp-close");
    if (!dlg || !body) return;

    const p = proc || (agent.processes && agent.processes[0]) || {};
    title.textContent = agent.label + " · Process Intelligence";
    body.innerHTML = "";

    const wrap = node("div", "ar-insp-content");

    // Top metadata card
    const card = node("div", "ar-insp-card");
    const badgeRow = node("div", "ar-insp-badges");
    if (agent.is_model_server) {
      badgeRow.appendChild(node("span", "chip chip-blue", "Model Server"));
    } else {
      badgeRow.appendChild(node("span", "chip chip-purple", "AI Coding Agent"));
    }
    const stBadge = node("span", "ar-agent-state", agent.state_label || "Present");
    stBadge.dataset.state = agent.state || "idle";
    badgeRow.appendChild(stBadge);

    const identLower = String(p.identity || "").toLowerCase();
    if (identLower.indexOf("suspect") !== -1) {
      badgeRow.appendChild(node("span", "chip chip-red", "Suspect Identity"));
    } else if (identLower.indexOf("unverified") !== -1) {
      badgeRow.appendChild(node("span", "chip chip-amber", "Unverified Process"));
    } else {
      badgeRow.appendChild(node("span", "chip chip-green", "Verified Tool"));
    }
    card.appendChild(badgeRow);

    if (agent.integration_note) {
      card.appendChild(node("p", "ar-insp-desc", agent.integration_note));
    }
    wrap.appendChild(card);

    // Process properties grid
    const pCard = node("div", "ar-insp-proc-card");
    pCard.appendChild(node("h4", "ar-insp-proc-title", "Process Telemetry · PID " + (p.pid || "N/A")));

    const grid = node("dl", "ar-insp-grid");
    function addProp(k, v, isCode) {
      grid.appendChild(node("dt", "ar-insp-key", k));
      const dd = node("dd", "ar-insp-val");
      if (isCode) {
        const c = node("code", "ar-insp-code", v || "N/A");
        dd.appendChild(c);
      } else {
        dd.textContent = v || "N/A";
      }
      grid.appendChild(dd);
    }

    addProp("Image Name", p.name || "Unknown");
    addProp("Executable Path", p.exe_path || "Unknown", true);
    addProp("Identity Confidence", p.identity_label || p.identity || "Unknown");
    if (p.started_unix) {
      addProp("Start Time", new Date(p.started_unix * 1000).toLocaleString());
    }
    if (p.cmdline_masked) {
      addProp("Command Line (Masked)", p.cmdline_masked, true);
    }
    addProp("Forensic Boundary", "Presence is not activity. Workspace modifications are recorded separately via Flight Recorder.");

    pCard.appendChild(grid);
    wrap.appendChild(pCard);
    body.appendChild(wrap);

    dlg.classList.remove("hidden");
    if (typeof dlg.showModal === "function") {
      try { dlg.showModal(); } catch (_) {}
    }

    function close() {
      dlg.classList.add("hidden");
      if (typeof dlg.close === "function") {
        try { dlg.close(); } catch (_) {}
      }
    }

    if (closeBtn) closeBtn.onclick = close;
    dlg.onclick = function (e) {
      if (e.target === dlg) close();
    };
    dlg.onkeydown = function (e) {
      if (e.key === "Escape") close();
    };
  }
"""

if "function openAgentInspector" not in agent_js:
    # Insert helper before renderRadar
    target = "  function renderRadar(payload) {"
    assert target in agent_js, "renderRadar not found in agent-radar.js"
    agent_js = agent_js.replace(target, inspector_handler + "\n" + target)

    # Insert action button in agent cards
    card_target = '      li.appendChild(\n        node("span", "ar-agent-note", agent.integration_note || ""),\n      );'
    inspect_btn_code = """
      const inspBtn = node("button", "btn btn-ghost ar-agent-btn-inspect", "Inspect Details");
      inspBtn.type = "button";
      inspBtn.addEventListener("click", function (ev) {
        ev.stopPropagation();
        openAgentInspector(agent, (agent.processes && agent.processes[0]) || null);
      });
      li.appendChild(inspBtn);
      li.addEventListener("click", function () {
        openAgentInspector(agent, (agent.processes && agent.processes[0]) || null);
      });"""
    assert card_target in agent_js, "Card note target not found"
    agent_js = agent_js.replace(card_target, card_target + "\n" + inspect_btn_code)
    agent_js_path.write_text(agent_js, encoding="utf-8")
    print("Updated agent-radar.js with openAgentInspector modal wiring")

agent_css_path = DIST / "agent.css"
agent_css = agent_css_path.read_text(encoding="utf-8")

inspector_css = """
/* Agent Detail Inspector Modal */
.ar-inspector-modal {
  position: fixed;
  inset: 0;
  width: 100vw;
  height: 100vh;
  background: rgba(0, 0, 0, 0.75);
  backdrop-filter: blur(8px);
  border: none;
  padding: 0;
  margin: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}
.ar-inspector-modal.hidden {
  display: none !important;
}
.ar-inspector-modal .modal-card {
  width: min(640px, 92vw);
  max-height: 84vh;
  background: var(--ink-100, #131722);
  border: 1px solid var(--hair, rgba(255, 255, 255, 0.12));
  border-radius: 14px;
  box-shadow: 0 16px 48px rgba(0, 0, 0, 0.6);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
[data-theme="light"] .ar-inspector-modal .modal-card {
  background: #ffffff;
  border-color: #e2e8f0;
  box-shadow: 0 12px 36px rgba(0, 0, 0, 0.15);
}
.ar-inspector-modal .modal-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--s4) var(--s5);
  border-bottom: 1px solid var(--hair);
}
.ar-inspector-modal .modal-body {
  padding: var(--s5);
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: var(--s4);
}
.ar-insp-card {
  background: rgba(255, 255, 255, 0.02);
  border: 1px solid var(--hair);
  border-radius: 10px;
  padding: var(--s4);
}
.ar-insp-badges {
  display: flex;
  align-items: center;
  gap: var(--s3);
  margin-bottom: var(--s2);
}
.ar-insp-desc {
  font-size: var(--sm);
  color: var(--text-dim);
  margin: 0;
}
.ar-insp-proc-card {
  background: var(--ink-150, #0c0e15);
  border: 1px solid var(--hair);
  border-radius: 10px;
  padding: var(--s4);
}
[data-theme="light"] .ar-insp-proc-card {
  background: #f8fafc;
}
.ar-insp-proc-title {
  font: 600 13px/1.3 Inter, system-ui, sans-serif;
  color: var(--text);
  margin: 0 0 var(--s3) 0;
}
.ar-insp-grid {
  display: grid;
  grid-template-columns: 140px 1fr;
  gap: 8px 12px;
  margin: 0;
  font-size: 12px;
}
.ar-insp-key {
  color: var(--text-dim);
  font-weight: 500;
}
.ar-insp-val {
  color: var(--text);
  margin: 0;
  word-break: break-all;
}
.ar-insp-code {
  font-family: monospace;
  font-size: 11px;
  padding: 2px 6px;
  background: rgba(255, 255, 255, 0.06);
  border-radius: 4px;
}
.ar-agent-btn-inspect {
  margin-top: var(--s3);
  align-self: flex-start;
  font-size: 11px;
  padding: 4px 10px;
  border-radius: 6px;
}
.chip-green {
  background: rgba(16, 185, 129, 0.16);
  color: #10b981;
  border: 1px solid rgba(16, 185, 129, 0.3);
}
.chip-amber {
  background: rgba(245, 158, 11, 0.16);
  color: #f59e0b;
  border: 1px solid rgba(245, 158, 11, 0.3);
}
.chip-red {
  background: rgba(239, 68, 68, 0.16);
  color: #ef4444;
  border: 1px solid rgba(239, 68, 68, 0.3);
}
"""

if "ar-inspector-modal" not in agent_css:
    agent_css += "\n" + inspector_css
    agent_css_path.write_text(agent_css, encoding="utf-8")
    print("Updated agent.css with inspector dialog styles")

# ==============================================================================
# 3. QUARANTINE SECURE VAULT & INVESTIGATE DOSSIER (index.html)
# ==============================================================================
index_html_path = DIST / "index.html"
index_html = index_html_path.read_text(encoding="utf-8")

# Quarantine empty state replacement
old_q_empty = """                <div class="empty-state hidden" id="q-empty">
                  <strong>Quarantine is empty</strong>Confirmed file moves will
                  appear here with integrity metadata and Undo.
                </div>"""

new_q_empty = """                <div class="empty-state hidden vault-empty-state" id="q-empty">
                  <div class="vault-hero">
                    <div class="vault-icon-wrap">
                      <svg class="vault-svg" viewBox="0 0 96 96" width="76" height="76" aria-hidden="true">
                        <circle cx="48" cy="48" r="42" fill="none" stroke="var(--hair)" stroke-width="1.8" stroke-dasharray="4 4" />
                        <circle cx="48" cy="48" r="35" fill="var(--ink-100)" stroke="var(--cure-purple)" stroke-width="2" />
                        <circle cx="48" cy="48" r="26" fill="none" stroke="var(--cure-cyan)" stroke-width="1.2" />
                        <path d="M48 22 V32 M48 64 V74 M22 48 H32 M64 48 H74" stroke="var(--cure-cyan)" stroke-width="2.2" stroke-linecap="round" />
                        <circle cx="48" cy="48" r="13" fill="var(--ink-200)" stroke="var(--cure-purple)" stroke-width="1.8" />
                        <path d="M44 46 L47.5 49.5 L53 43" fill="none" stroke="#22d3ee" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" />
                      </svg>
                    </div>
                    <div class="vault-copy">
                      <h2 class="vault-title">Secure Vault Enclave · Ready</h2>
                      <p class="vault-subline">
                        Quarantine isolates dangerous persistence files under <code>%LOCALAPPDATA%\\CURE\\quarantine</code> with preserved metadata and zero deletions.
                      </p>
                    </div>
                  </div>
                  <div class="vault-pillars">
                    <div class="vault-pillar">
                      <div class="pillar-icon">🔒</div>
                      <div class="pillar-body">
                        <strong>Move, Never Delete</strong>
                        <span>Files are moved to an isolated enclave with preserved NTFS attributes and timestamps.</span>
                      </div>
                    </div>
                    <div class="vault-pillar">
                      <div class="pillar-icon">🛡️</div>
                      <div class="pillar-body">
                        <strong>Cryptographic Integrity</strong>
                        <span>Original SHA-256 hashes, security descriptors, and registry mappings are sealed for forensic review.</span>
                      </div>
                    </div>
                    <div class="vault-pillar">
                      <div class="pillar-icon">↩️</div>
                      <div class="pillar-body">
                        <strong>100% Scoped Reversible Undo</strong>
                        <span>Every confirmed move can be restored byte-for-byte to its original path with a single click.</span>
                      </div>
                    </div>
                  </div>
                </div>"""

if "vault-empty-state" not in index_html:
    assert old_q_empty in index_html, "Old q-empty not found in index.html"
    index_html = index_html.replace(old_q_empty, new_q_empty)
    print("Updated index.html with Secure Vault Enclave")

# Investigate pre-scan replacement
old_review_clear = """                  <div class="all-clear" id="review-clear">
                    <span
                      >No evidence collected yet. Run a scan from Rescue.</span
                    >
                  </div>"""

new_review_clear = """                  <div class="all-clear" id="review-clear">
                    <div class="dossier-empty-card">
                      <div class="dossier-hero">
                        <div class="dossier-badge">INVESTIGATION DOSSIER</div>
                        <h3 class="dossier-title">No Findings Collected This Session</h3>
                        <span>Run a Rescue Scan to collect findings for review.</span>
                      </div>
                      <div class="dossier-vectors">
                        <span class="d-chip">Registry</span>
                        <span class="d-chip">Startup</span>
                        <span class="d-chip">Tasks</span>
                        <span class="d-chip">Services</span>
                        <span class="d-chip">WMI</span>
                        <span class="d-chip">IFEO</span>
                        <span class="d-chip">COM</span>
                        <span class="d-chip">Processes</span>
                        <span class="d-chip">Ransom</span>
                      </div>
                    </div>
                  </div>"""

if "dossier-empty-card" not in index_html:
    assert old_review_clear in index_html, "Old review-clear not found in index.html"
    index_html = index_html.replace(old_review_clear, new_review_clear)
    print("Updated index.html with Investigation Dossier card")

index_html_path.write_text(index_html, encoding="utf-8")

# ==============================================================================
# 4. GLOBAL STYLES (style.css)
# ==============================================================================
style_css_path = DIST / "style.css"
style_css = style_css_path.read_text(encoding="utf-8")

global_ui_css = """
/* ==========================================================================
   FUTURISTIC UI POLISH & COMPONENT HARMONIZATION
   ========================================================================== */

/* Standardize card radius to 14px */
.panel, .card, .ov-hero, .ar-radar-card, .ar-flight-card {
  border-radius: 14px !important;
}

/* Secure Vault Enclave Empty State */
.vault-empty-state {
  display: flex !important;
  flex-direction: column;
  align-items: center;
  padding: 36px 24px !important;
  text-align: center;
}
.vault-hero {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 14px;
  max-width: 620px;
  margin-bottom: 28px;
}
.vault-svg {
  filter: drop-shadow(0 0 16px rgba(139, 111, 240, 0.25));
}
.vault-title {
  font: 700 18px/1.3 Inter, system-ui, sans-serif;
  color: var(--text);
  margin: 0;
}
.vault-subline {
  font-size: 13px;
  color: var(--text-dim);
  line-height: 1.5;
  margin: 0;
}
.vault-subline code {
  color: var(--cure-cyan);
  background: rgba(34, 211, 238, 0.08);
  padding: 2px 6px;
  border-radius: 4px;
  font-family: monospace;
}
.vault-pillars {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 16px;
  max-width: 860px;
  width: 100%;
  text-align: left;
}
.vault-pillar {
  display: flex;
  gap: 12px;
  padding: 16px;
  background: rgba(255, 255, 255, 0.02);
  border: 1px solid var(--hair);
  border-radius: 10px;
}
[data-theme="light"] .vault-pillar {
  background: #f8fafc;
  border-color: #e2e8f0;
}
.pillar-icon {
  font-size: 20px;
  flex-shrink: 0;
}
.pillar-body strong {
  display: block;
  font: 600 13px/1.3 Inter, system-ui, sans-serif;
  color: var(--text);
  margin-bottom: 4px;
}
.pillar-body span {
  display: block;
  font-size: 11px;
  line-height: 1.45;
  color: var(--text-dim);
}

/* Investigation Dossier Card */
.dossier-empty-card {
  padding: 24px;
  text-align: center;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
}
.dossier-badge {
  font: 700 10px/1 monospace;
  letter-spacing: 0.1em;
  color: var(--cure-cyan);
  background: rgba(34, 211, 238, 0.12);
  border: 1px solid rgba(34, 211, 238, 0.28);
  padding: 4px 8px;
  border-radius: 4px;
  display: inline-block;
  margin-bottom: 6px;
}
.dossier-title {
  font: 600 16px/1.3 Inter, system-ui, sans-serif;
  color: var(--text);
  margin: 0 0 6px 0;
}
.dossier-vectors {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  justify-content: center;
  max-width: 580px;
}
.d-chip {
  font: 500 11px/1.3 monospace;
  padding: 4px 10px;
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid var(--hair);
  border-radius: 6px;
  color: var(--text-dim);
}
[data-theme="light"] .d-chip {
  background: #f1f5f9;
  border-color: #e2e8f0;
}

/* Evidence Drawer Two-Column Tabular Formatting */
.evidence-section {
  background: rgba(255, 255, 255, 0.02);
  border: 1px solid var(--hair);
  border-radius: 10px;
  padding: 14px 16px;
  margin-bottom: 12px;
}
[data-theme="light"] .evidence-section {
  background: #f8fafc;
  border-color: #e2e8f0;
}
.evidence-section h3 {
  font: 600 11px/1.2 Inter, system-ui, sans-serif;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-dim);
  margin: 0 0 8px 0;
}
.evidence-section p {
  margin: 0;
  font-size: 13px;
  line-height: 1.5;
  color: var(--text);
  word-break: break-all;
}
.evidence-section p code {
  font-family: monospace;
  font-size: 11px;
  background: rgba(255, 255, 255, 0.06);
  padding: 2px 6px;
  border-radius: 4px;
}
"""

if "vault-empty-state" not in style_css:
    style_css += "\n" + global_ui_css
    style_css_path.write_text(style_css, encoding="utf-8")
    print("Updated style.css with global UI polish")

print("ALL FUTURISTIC UI UPDATES APPLIED SUCCESSFULLY!")
