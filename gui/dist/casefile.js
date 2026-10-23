/* ==========================================================================
   C.U.R.E · case file renderer
   --------------------------------------------------------------------------
   The case file is a rendered image, not a screenshot. It is drawn from an
   allowlisted model, and the allowlist is the only thing this file reads.

   Nothing here is allowed to touch a path, a user name, a machine name or a
   file name, because the model it is given has no field that can hold one.
   Every string that reaches the canvas is pushed onto a list as it is drawn,
   and that list is what the privacy test inspects.

   The image is produced locally. Nothing is uploaded, and no network request
   is ever made from this file.
   ========================================================================== */
(function () {
  "use strict";

  const WIDTH = 1200;
  const HEIGHT = 1500;

  const INK = {
    bg: "#0d1017",
    bgLift: "#141926",
    panel: "#171d2b",
    line: "#28314a",
    text: "#eef2fb",
    muted: "#93a0bd",
    faint: "#5f6c8a",
    accent: "#8f7bf0",
    accentSoft: "rgba(143, 123, 240, 0.16)",
    ok: "#54d1a4",
    warn: "#f0b45c",
    danger: "#f0708a",
  };

  const BAND_COLORS = {
    Regenerable: "#54d1a4",
    "Needs review": "#f0b45c",
    Protected: "#5f7fd6",
    Unknown: "#7a6a9c",
  };

  const FUR = {
    deep: "#a94f1c",
    base: "#df8038",
    lit: "#f7b76a",
    cream: "#fff4e2",
    collar: "#8268e6",
    eye: "#2a1a10",
    nose: "#f48fa6",
  };

  const FONT = '"Segoe UI", "Segoe UI Variable Text", system-ui, -apple-system, sans-serif';

  /* ---------------------------------------------------------------------
     The allowlist
     ---------------------------------------------------------------------
     This is the privacy boundary. The renderer reads the returned object and
     nothing else, so a field that is not copied here can never be drawn. */
  function allowlist(caseFile) {
    const src = caseFile || {};
    const categories = Array.isArray(src.categories) ? src.categories : [];
    if (String(src.kind || "") === "agent_session") {
      const number = function (value) {
        return typeof value === "number" && isFinite(value) ? value : 0;
      };
      return {
        kind: "agent_session",
        caseId: String(src.case_id || ""),
        title: String(src.title || "AGENT SESSION CASE"),
        agentLabel: String(src.agent_label || "No supported agent observed"),
        agentPresence: String(src.agent_presence || ""),
        observationLabel: String(src.observation_label || "0s"),
        changesTotal: number(src.changes_total),
        created: number(src.created),
        modified: number(src.modified),
        removed: number(src.removed),
        renamed: number(src.renamed),
        storageDeltaLabel: String(src.storage_delta_label || "No net size change"),
        storageNote: String(src.storage_note || ""),
        confidenceLabel: String(src.confidence_label || "No changes observed"),
        confidenceNote: String(src.confidence_note || ""),
        completeness: String(src.completeness || "Coverage not stated"),
        coverageNote: String(src.coverage_note || ""),
        observedEvents: number(src.observed_events),
        correlatedEvents: number(src.correlated_events),
        unknownEvents: number(src.unknown_events),
        windowsAdded: number(src.windows_added),
        windowsRemoved: number(src.windows_removed),
        windowsChanged: number(src.windows_changed),
        windowsNote: String(src.windows_note || "No Windows persistence comparison was included."),
        reconciliationUsed: !!src.reconciliation_used,
        summary: src.summary ? String(src.summary) : null,
        summaryWithheld: !!src.summary_withheld,
        demo: !!src.demo,
      };
    }
    return {
      kind: "storage_case",
      title: String(src.title || "THE MISSING STORAGE CASE"),
      status: String(src.status || "investigation_complete"),
      statusLabel: String(src.status_label || "Investigation complete"),
      measuredLabel: String(src.measured_label || "—"),
      candidateLabel: String(src.candidate_label || "—"),
      verifiedLabel: src.verified_recovered_label ? String(src.verified_recovered_label) : null,
      removedLabel: src.bytes_removed_label ? String(src.bytes_removed_label) : null,
      filesRemoved: typeof src.files_removed === "number" ? src.files_removed : null,
      categories: categories.map(function (c) {
        return {
          label: String(c.label || ""),
          bandLabel: String(c.band_label || ""),
          bytesLabel: String(c.bytes_label || ""),
          share: typeof c.share === "number" ? c.share : 0,
          itemCount:
            typeof c.item_count === "number" ? c.item_count : null,
        };
      }),
      leads: (Array.isArray(src.notable_leads) ? src.notable_leads : [])
        .map(function (l) {
          /* The backend sends labels, not ids. Accept an object shape too,
             so a stale caller can never print "[object Object]" on a file
             the operator might publish. */
          if (typeof l === "string") return l;
          return l && typeof l.title === "string" ? l.title : "";
        })
        .filter(function (l) {
          return l !== "";
        }),
      completeness: String(src.completeness || ""),
      summary: src.summary ? String(src.summary) : null,
      summaryWithheld: !!src.summary_withheld,
      caseId: String(src.case_id || ""),
      demo: !!src.demo,
    };
  }

  /* ---------------------------------------------------------------------
     Drawing helpers
     --------------------------------------------------------------------- */
  function makeCanvas() {
    const canvas = document.createElement("canvas");
    canvas.width = WIDTH;
    canvas.height = HEIGHT;
    return canvas;
  }

  function roundRect(ctx, x, y, w, h, r) {
    const radius = Math.min(r, w / 2, h / 2);
    ctx.beginPath();
    ctx.moveTo(x + radius, y);
    ctx.lineTo(x + w - radius, y);
    ctx.quadraticCurveTo(x + w, y, x + w, y + radius);
    ctx.lineTo(x + w, y + h - radius);
    ctx.quadraticCurveTo(x + w, y + h, x + w - radius, y + h);
    ctx.lineTo(x + radius, y + h);
    ctx.quadraticCurveTo(x, y + h, x, y + h - radius);
    ctx.lineTo(x, y + radius);
    ctx.quadraticCurveTo(x, y, x + radius, y);
    ctx.closePath();
  }

  function tracked(ctx, text, x, y, spacing) {
    /* Letter-spaced text, drawn glyph by glyph so the spacing is exact. */
    let cursor = x;
    for (const ch of text) {
      ctx.fillText(ch, cursor, y);
      cursor += ctx.measureText(ch).width + spacing;
    }
    return cursor - x;
  }

  function trackedWidth(ctx, text, spacing) {
    let total = 0;
    for (const ch of text) total += ctx.measureText(ch).width + spacing;
    return Math.max(0, total - spacing);
  }

  /* ---------------------------------------------------------------------
     LUMA
     ---------------------------------------------------------------------
     A small, deliberately simplified mark of the companion: the same
     silhouette language as the animated character, drawn from primitives so
     the exported image never depends on an external asset. */
  function drawLuma(ctx, x, y, scale) {
    ctx.save();
    ctx.translate(x, y);
    ctx.scale(scale, scale);

    /* tail */
    ctx.strokeStyle = FUR.base;
    ctx.lineWidth = 7;
    ctx.lineCap = "round";
    ctx.beginPath();
    ctx.moveTo(26, 36);
    ctx.bezierCurveTo(42, 44, 44, 62, 28, 68);
    ctx.stroke();
    ctx.strokeStyle = FUR.lit;
    ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.moveTo(26, 36);
    ctx.bezierCurveTo(38, 42, 40, 56, 30, 62);
    ctx.stroke();

    /* body */
    ctx.fillStyle = FUR.base;
    ctx.beginPath();
    ctx.ellipse(24, 44, 20, 22, 0, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = FUR.cream;
    ctx.beginPath();
    ctx.ellipse(24, 50, 11, 13, 0, 0, Math.PI * 2);
    ctx.fill();

    /* ears */
    ctx.fillStyle = FUR.deep;
    ctx.beginPath();
    ctx.moveTo(8, 20);
    ctx.quadraticCurveTo(4, 2, 18, 10);
    ctx.closePath();
    ctx.fill();
    ctx.beginPath();
    ctx.moveTo(36, 16);
    ctx.quadraticCurveTo(48, 0, 46, 20);
    ctx.closePath();
    ctx.fill();
    ctx.fillStyle = FUR.nose;
    ctx.beginPath();
    ctx.moveTo(11, 17);
    ctx.quadraticCurveTo(9, 8, 16, 12);
    ctx.closePath();
    ctx.fill();
    ctx.beginPath();
    ctx.moveTo(38, 14);
    ctx.quadraticCurveTo(45, 6, 44, 17);
    ctx.closePath();
    ctx.fill();

    /* head */
    ctx.fillStyle = FUR.base;
    ctx.beginPath();
    ctx.ellipse(24, 20, 19, 17, 0, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = FUR.lit;
    ctx.beginPath();
    ctx.ellipse(19, 15, 9, 7, -0.4, 0, Math.PI * 2);
    ctx.fill();

    /* eyes */
    ctx.fillStyle = FUR.eye;
    ctx.beginPath();
    ctx.ellipse(16, 21, 3.1, 4.1, 0, 0, Math.PI * 2);
    ctx.fill();
    ctx.beginPath();
    ctx.ellipse(32, 21, 3.1, 4.1, 0, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = "rgba(255,255,255,0.92)";
    ctx.beginPath();
    ctx.ellipse(17, 19.2, 1.1, 1.4, 0, 0, Math.PI * 2);
    ctx.fill();
    ctx.beginPath();
    ctx.ellipse(33, 19.2, 1.1, 1.4, 0, 0, Math.PI * 2);
    ctx.fill();

    /* muzzle and nose */
    ctx.fillStyle = FUR.cream;
    ctx.beginPath();
    ctx.ellipse(24, 28, 9, 6, 0, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = FUR.nose;
    ctx.beginPath();
    ctx.moveTo(24, 24);
    ctx.lineTo(27, 27);
    ctx.lineTo(21, 27);
    ctx.closePath();
    ctx.fill();

    /* collar */
    ctx.strokeStyle = FUR.collar;
    ctx.lineWidth = 4.5;
    ctx.beginPath();
    ctx.moveTo(9, 36);
    ctx.quadraticCurveTo(24, 42, 39, 35);
    ctx.stroke();
    ctx.fillStyle = "#c9bdf7";
    ctx.beginPath();
    ctx.arc(24, 41, 3.2, 0, Math.PI * 2);
    ctx.fill();

    ctx.restore();
  }


  function wrapText(ctx, text, x, y, maxWidth, lineHeight, maxLines) {
    const words = String(text || "").split(/\s+/);
    let line = "";
    let used = 0;
    for (let i = 0; i < words.length; i++) {
      const candidate = line ? line + " " + words[i] : words[i];
      if (ctx.measureText(candidate).width > maxWidth && line) {
        ctx.fillText(line, x, y);
        y += lineHeight;
        used += 1;
        line = words[i];
        if (maxLines && used >= maxLines - 1 && i < words.length - 1) {
          line = line + " " + words.slice(i + 1).join(" ");
          break;
        }
      } else {
        line = candidate;
      }
    }
    if (line) ctx.fillText(line, x, y);
    return y + lineHeight;
  }

  function renderAgent(model, canvas, ctx) {
    const texts = [];
    const say = function (text) {
      texts.push(String(text));
      return String(text);
    };
    const PAD = 76;
    const W = WIDTH - PAD * 2;

    const bg = ctx.createLinearGradient(0, 0, WIDTH * 0.55, HEIGHT);
    bg.addColorStop(0, "#10141d");
    bg.addColorStop(1, INK.bg);
    ctx.fillStyle = bg;
    ctx.fillRect(0, 0, WIDTH, HEIGHT);

    ctx.strokeStyle = "rgba(143,123,240,0.05)";
    ctx.lineWidth = 1;
    for (let x = 0; x <= WIDTH; x += 40) {
      ctx.beginPath();
      ctx.moveTo(x + 0.5, 0);
      ctx.lineTo(x + 0.5, HEIGHT);
      ctx.stroke();
    }
    for (let y = 0; y <= HEIGHT; y += 40) {
      ctx.beginPath();
      ctx.moveTo(0, y + 0.5);
      ctx.lineTo(WIDTH, y + 0.5);
      ctx.stroke();
    }

    const glow = ctx.createRadialGradient(WIDTH * 0.8, 110, 20, WIDTH * 0.8, 110, 600);
    glow.addColorStop(0, "rgba(143,123,240,0.17)");
    glow.addColorStop(1, "rgba(143,123,240,0)");
    ctx.fillStyle = glow;
    ctx.fillRect(0, 0, WIDTH, HEIGHT);

    ctx.textBaseline = "alphabetic";
    ctx.fillStyle = INK.accent;
    ctx.font = "700 22px " + FONT;
    tracked(ctx, say("C.U.R.E"), PAD, 92, 4.5);

    ctx.fillStyle = INK.faint;
    ctx.font = "600 16px " + FONT;
    tracked(ctx, say("AGENT EVIDENCE"), PAD + trackedWidth(ctx, "C.U.R.E", 4.5) + 46, 92, 3);

    const chipText = say(String(model.confidenceLabel || "No changes observed").toUpperCase());
    ctx.font = "700 15px " + FONT;
    const chipW = Math.min(W - 40, trackedWidth(ctx, chipText, 2.4) + 34);
    const chipX = WIDTH - PAD - chipW;
    ctx.fillStyle = INK.accentSoft;
    roundRect(ctx, chipX, 66, chipW, 34, 17);
    ctx.fill();
    ctx.strokeStyle = "rgba(143,123,240,0.5)";
    ctx.lineWidth = 1;
    ctx.stroke();
    ctx.fillStyle = INK.accent;
    tracked(ctx, chipText, chipX + 17, 89, 2.4);

    ctx.fillStyle = INK.text;
    ctx.font = "700 62px " + FONT;
    ctx.fillText(say("AGENT SESSION CASE"), PAD, 184);

    ctx.fillStyle = INK.muted;
    ctx.font = "500 19px " + FONT;
    ctx.fillText(
      say("A privacy-safe record of observed workspace changes"),
      PAD,
      224
    );

    ctx.strokeStyle = INK.line;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(PAD, 252);
    ctx.lineTo(WIDTH - PAD, 252);
    ctx.stroke();

    const figures = [
      { label: "CHANGES OBSERVED", value: String(model.changesTotal || 0), color: INK.text },
      { label: "OBSERVATION TIME", value: model.observationLabel, color: INK.text },
      { label: "CORRELATED EVENTS", value: String(model.correlatedEvents || 0), color: INK.accent },
      { label: "OBSERVED EVENTS", value: String(model.observedEvents || 0), color: INK.ok },
    ];
    let y = 300;
    const colW = W / figures.length;
    figures.forEach(function (fig, i) {
      const x = PAD + colW * i;
      ctx.fillStyle = INK.faint;
      ctx.font = "700 12px " + FONT;
      tracked(ctx, say(fig.label), x, y, 2.2);
      ctx.fillStyle = fig.color;
      ctx.font = "700 46px " + FONT;
      ctx.fillText(say(fig.value), x, y + 52);
      if (i > 0) {
        ctx.strokeStyle = "rgba(40,49,74,0.9)";
        ctx.beginPath();
        ctx.moveTo(x - 22, y - 24);
        ctx.lineTo(x - 22, y + 62);
        ctx.stroke();
      }
    });

    /* evidence-state card */
    y = 396;
    ctx.fillStyle = INK.panel;
    roundRect(ctx, PAD, y, W, 168, 14);
    ctx.fill();
    ctx.strokeStyle = INK.line;
    ctx.lineWidth = 1;
    ctx.stroke();

    ctx.fillStyle = INK.faint;
    ctx.font = "700 12px " + FONT;
    tracked(ctx, say("WHAT THE RECORDER CAN SAY"), PAD + 26, y + 30, 2.4);
    ctx.fillStyle = INK.text;
    ctx.font = "600 26px " + FONT;
    ctx.fillText(say(model.confidenceLabel), PAD + 26, y + 64);
    ctx.fillStyle = INK.muted;
    ctx.font = "500 18px " + FONT;
    wrapText(ctx, say(model.confidenceNote), PAD + 26, y + 94, W - 52, 23, 2);
    ctx.fillStyle = INK.faint;
    ctx.font = "500 15px " + FONT;
    ctx.fillText(
      say("Completeness: " + model.completeness + " · Reconciliation used: " + (model.reconciliationUsed ? "yes" : "no")),
      PAD + 26,
      y + 160
    );

    /* change composition */
    y = 596;
    ctx.fillStyle = INK.faint;
    ctx.font = "700 13px " + FONT;
    tracked(ctx, say("OBSERVED CHANGE COMPOSITION"), PAD, y, 2.6);
    y += 24;

    const bands = [
      { label: "Created", value: model.created, color: INK.ok },
      { label: "Modified", value: model.modified, color: INK.warn },
      { label: "Removed", value: model.removed, color: INK.danger },
      { label: "Renamed", value: model.renamed, color: INK.accent },
    ];
    const total = bands.reduce(function (sum, b) { return sum + Math.max(0, b.value || 0); }, 0);
    const barH = 44;
    ctx.fillStyle = "rgba(255,255,255,0.03)";
    roundRect(ctx, PAD, y, W, barH, 9);
    ctx.fill();
    if (total > 0) {
      let cx = PAD;
      bands.forEach(function (b) {
        const w = ((b.value || 0) / total) * W;
        if (w <= 0) return;
        ctx.fillStyle = b.color;
        ctx.fillRect(cx, y, Math.max(1, w - 2), barH);
        cx += w;
      });
    }
    y += barH + 30;

    bands.forEach(function (b) {
      ctx.fillStyle = b.color;
      roundRect(ctx, PAD, y - 13, 11, 11, 3);
      ctx.fill();
      ctx.fillStyle = INK.text;
      ctx.font = "500 19px " + FONT;
      ctx.fillText(say(b.label), PAD + 24, y - 3);
      ctx.fillStyle = INK.muted;
      ctx.font = "500 17px " + FONT;
      ctx.textAlign = "right";
      ctx.fillText(say(String(b.value || 0)), WIDTH - PAD, y - 3);
      ctx.textAlign = "left";
      y += 30;
    });

    /* Windows persistence leads */
    y += 8;
    const winTotal = (model.windowsAdded || 0) + (model.windowsRemoved || 0) + (model.windowsChanged || 0);
    ctx.fillStyle = INK.bgLift;
    roundRect(ctx, PAD, y, W, 116, 12);
    ctx.fill();
    ctx.strokeStyle = INK.line;
    ctx.stroke();
    ctx.fillStyle = INK.faint;
    ctx.font = "700 12px " + FONT;
    tracked(ctx, say("WINDOWS PERSISTENCE LEADS"), PAD + 24, y + 30, 2.4);
    ctx.fillStyle = winTotal ? INK.warn : INK.muted;
    ctx.font = "600 25px " + FONT;
    ctx.fillText(
      say(winTotal ? winTotal + " review lead" + (winTotal === 1 ? "" : "s") : "None included"),
      PAD + 24,
      y + 64
    );
    ctx.fillStyle = INK.muted;
    ctx.font = "500 17px " + FONT;
    wrapText(ctx, say(model.windowsNote), PAD + 24, y + 90, W - 48, 21, 1);
    if (winTotal) {
      ctx.fillStyle = INK.faint;
      ctx.font = "500 15px " + FONT;
      ctx.textAlign = "right";
      ctx.fillText(
        say(
          "+" + model.windowsAdded + "  ~" + model.windowsChanged + "  -" + model.windowsRemoved
        ),
        WIDTH - PAD - 24,
        y + 64
      );
      ctx.textAlign = "left";
    }

    /* boundaries */
    y += 136;
    ctx.fillStyle = INK.faint;
    ctx.font = "700 13px " + FONT;
    tracked(ctx, say("WHAT THIS FILE DOES NOT SAY"), PAD, y, 2.6);
    y += 30;
    const limits = [
      "It does not attribute a change to an agent or to a person.",
      "It contains no paths, file names, prompts, credentials or machine identifiers.",
      "A detected process is not proof that code was generated or a task was run.",
      "Changes while an agent was present are correlated, not caused.",
    ];
    ctx.font = "500 19px " + FONT;
    limits.forEach(function (line) {
      ctx.fillStyle = INK.accent;
      ctx.beginPath();
      ctx.arc(PAD + 4, y - 6, 3.2, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = INK.text;
      ctx.fillText(say(line), PAD + 22, y);
      y += 28;
    });

    y += 12;
    ctx.fillStyle = INK.muted;
    ctx.font = "500 18px " + FONT;
    ctx.fillText(say(model.agentPresence), PAD, y);
    y += 22;
    ctx.fillStyle = INK.faint;
    ctx.font = "500 16px " + FONT;
    ctx.fillText(say("Agent observed: " + model.agentLabel), PAD, y);
    y += 24;
    ctx.fillText(say("Net metadata change: " + model.storageDeltaLabel + " · " + model.storageNote), PAD, y);

    y += 24;
    ctx.fillStyle = INK.faint;
    ctx.font = "500 15px " + FONT;
    y = wrapText(ctx, say("Coverage: " + model.coverageNote), PAD, y, W, 20, 2);

    if (model.summary) {
      y += 26;
      ctx.fillStyle = INK.accentSoft;
      roundRect(ctx, PAD, y - 20, W, 64, 12);
      ctx.fill();
      ctx.strokeStyle = "rgba(143,123,240,0.35)";
      ctx.stroke();
      ctx.fillStyle = INK.text;
      ctx.font = "italic 500 20px " + FONT;
      ctx.fillText(say("“" + model.summary + "”"), PAD + 24, y + 16);
    } else if (model.summaryWithheld) {
      y += 28;
      ctx.fillStyle = INK.muted;
      ctx.font = "italic 500 17px " + FONT;
      ctx.fillText(
        say("The operator note was withheld because it appeared to contain private data."),
        PAD,
        y
      );
    } else {
      y += 28;
      ctx.fillStyle = INK.faint;
      ctx.font = "700 12px " + FONT;
      tracked(ctx, say("RECORDER PROVENANCE"), PAD, y, 2.4);
      y += 24;
      ctx.fillStyle = INK.muted;
      ctx.font = "500 16px " + FONT;
      ctx.fillText(
        say("Observation: directory watcher plus final snapshot reconciliation."),
        PAD,
        y
      );
      y += 22;
      ctx.fillText(
        say("Contents: counts, fixed labels and an allowlisted evidence vocabulary."),
        PAD,
        y
      );
    }

    if (model.demo) {
      const bandY = HEIGHT - 196;
      ctx.fillStyle = "rgba(240,180,92,0.12)";
      roundRect(ctx, PAD, bandY, W, 34, 8);
      ctx.fill();
      ctx.strokeStyle = "rgba(240,180,92,0.55)";
      ctx.lineWidth = 1;
      ctx.stroke();
      ctx.fillStyle = "rgba(240,180,92,0.95)";
      ctx.font = "700 14px " + FONT;
      const demoText = say("SYNTHETIC DEMO · SAMPLE DATA, NOT A REAL OBSERVATION");
      tracked(
        ctx,
        demoText,
        PAD + (W - trackedWidth(ctx, demoText, 2.2)) / 2,
        bandY + 22,
        2.2
      );
    }

    /* footer */
    const footY = HEIGHT - 116;
    ctx.strokeStyle = INK.line;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(PAD, footY - 34);
    ctx.lineTo(WIDTH - PAD, footY - 34);
    ctx.stroke();

    drawLuma(ctx, PAD, footY - 22, 1.05);

    const footX = PAD + 118;
    ctx.fillStyle = INK.muted;
    ctx.font = "500 17px " + FONT;
    ctx.fillText(
      say("C.U.R.E · Clean USB Rescue Engine · github.com/MadB0i/C.U.R.E"),
      footX,
      footY
    );
    ctx.fillStyle = INK.faint;
    ctx.font = "500 15px " + FONT;
    ctx.fillText(
      say(model.caseId + " · " + model.completeness + " · " + model.observationLabel),
      footX,
      footY + 26
    );
    ctx.textAlign = "right";
    ctx.fillText(
      say("No paths, names or file names are included in this image."),
      WIDTH - PAD,
      footY + 26
    );
    ctx.textAlign = "left";

    return {
      canvas: canvas,
      dataUrl: canvas.toDataURL("image/png"),
      width: WIDTH,
      height: HEIGHT,
      texts: texts,
      model: model,
    };
  }


  /* ---------------------------------------------------------------------
     The renderer
     --------------------------------------------------------------------- */
  function render(caseFile, options) {
    const opts = options || {};
    const model = allowlist(caseFile);
    const canvas = makeCanvas();
    const ctx = canvas.getContext("2d");
    const texts = [];

    if (model.kind === "agent_session") {
      return renderAgent(model, canvas, ctx);
    }

    const say = function (text) {
      texts.push(String(text));
      return String(text);
    };

    /* --- background ------------------------------------------------- */
    const bg = ctx.createLinearGradient(0, 0, WIDTH * 0.6, HEIGHT);
    bg.addColorStop(0, "#10141d");
    bg.addColorStop(1, INK.bg);
    ctx.fillStyle = bg;
    ctx.fillRect(0, 0, WIDTH, HEIGHT);

    /* a faint measurement grid, the way graph paper sits under a plan */
    ctx.strokeStyle = "rgba(143,123,240,0.05)";
    ctx.lineWidth = 1;
    for (let x = 0; x <= WIDTH; x += 40) {
      ctx.beginPath();
      ctx.moveTo(x + 0.5, 0);
      ctx.lineTo(x + 0.5, HEIGHT);
      ctx.stroke();
    }
    for (let y = 0; y <= HEIGHT; y += 40) {
      ctx.beginPath();
      ctx.moveTo(0, y + 0.5);
      ctx.lineTo(WIDTH, y + 0.5);
      ctx.stroke();
    }

    /* corner light */
    const glow = ctx.createRadialGradient(WIDTH * 0.82, 90, 20, WIDTH * 0.82, 90, 620);
    glow.addColorStop(0, "rgba(143,123,240,0.18)");
    glow.addColorStop(1, "rgba(143,123,240,0)");
    ctx.fillStyle = glow;
    ctx.fillRect(0, 0, WIDTH, HEIGHT);

    const PAD = 76;

    /* --- masthead --------------------------------------------------- */
    ctx.textBaseline = "alphabetic";
    ctx.fillStyle = INK.accent;
    ctx.font = "700 22px " + FONT;
    tracked(ctx, say("C.U.R.E"), PAD, 92, 4.5);

    ctx.fillStyle = INK.faint;
    ctx.font = "600 16px " + FONT;
    tracked(ctx, say("DETECTIVE MODE"), PAD + trackedWidth(ctx, "C.U.R.E", 4.5) + 26, 92, 3);

    /* status chip, right aligned */
    const statusText = say(model.statusLabel.toUpperCase());
    ctx.font = "700 15px " + FONT;
    const chipW = trackedWidth(ctx, statusText, 2.4) + 34;
    const chipX = WIDTH - PAD - chipW;
    const chipY = 66;
    ctx.fillStyle = INK.accentSoft;
    roundRect(ctx, chipX, chipY, chipW, 34, 17);
    ctx.fill();
    ctx.strokeStyle = "rgba(143,123,240,0.5)";
    ctx.lineWidth = 1;
    ctx.stroke();
    ctx.fillStyle = INK.accent;
    tracked(ctx, statusText, chipX + 17, chipY + 23, 2.4);

    /* --- title ------------------------------------------------------ */
    ctx.fillStyle = INK.text;
    ctx.font = "700 62px " + FONT;
    const title = say(model.title);
    let titleY = 190;
    /* The title is fixed vocabulary, but keep it inside the frame even if a
       future wording is longer. */
    const maxWidth = WIDTH - PAD * 2;
    while (ctx.measureText(title).width > maxWidth && ctx.font !== "700 40px " + FONT) {
      const size = parseInt(ctx.font.match(/(\d+)px/)[1], 10) - 4;
      ctx.font = "700 " + size + "px " + FONT;
    }
    ctx.fillText(title, PAD, titleY);

    ctx.strokeStyle = INK.line;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(PAD, titleY + 30);
    ctx.lineTo(WIDTH - PAD, titleY + 30);
    ctx.stroke();

    /* --- headline figures ------------------------------------------- */
    let y = titleY + 92;
    const figures = [
      { label: "MEASURED", value: model.measuredLabel, color: INK.text },
      {
        label: "REMOVAL CANDIDATES",
        value: model.candidateLabel,
        color: INK.warn,
      },
    ];
    if (model.verifiedLabel) {
      figures.push({
        label: "VERIFIED RECOVERED",
        value: model.verifiedLabel,
        color: INK.ok,
      });
    } else if (model.removedLabel) {
      figures.push({
        label: "FILES CONFIRMED REMOVED",
        value: model.removedLabel,
        color: INK.muted,
      });
    }

    const colW = (WIDTH - PAD * 2) / figures.length;
    figures.forEach(function (fig, i) {
      const x = PAD + colW * i;
      ctx.fillStyle = INK.faint;
      ctx.font = "700 13px " + FONT;
      tracked(ctx, say(fig.label), x, y, 2.6);
      ctx.fillStyle = fig.color;
      ctx.font = "700 54px " + FONT;
      ctx.fillText(say(fig.value), x, y + 56);
      if (i > 0) {
        ctx.strokeStyle = "rgba(40,49,74,0.9)";
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(x - 26, y - 26);
        ctx.lineTo(x - 26, y + 66);
        ctx.stroke();
      }
    });

    y += 108;

    /* --- the band chart --------------------------------------------- */
    ctx.fillStyle = INK.faint;
    ctx.font = "700 13px " + FONT;
    tracked(ctx, say("WHERE IT WENT"), PAD, y, 2.6);
    y += 26;

    const barX = PAD;
    const barW = WIDTH - PAD * 2;
    const barH = 46;
    const total = model.categories.reduce(function (sum, c) {
      return sum + Math.max(0, c.share);
    }, 0);

    ctx.fillStyle = "rgba(255,255,255,0.03)";
    roundRect(ctx, barX, y, barW, barH, 10);
    ctx.fill();

    if (total > 0) {
      let cx = barX;
      model.categories.forEach(function (c) {
        const w = (Math.max(0, c.share) / total) * barW;
        if (w <= 0) return;
        ctx.fillStyle = BAND_COLORS[c.bandLabel] || INK.accent;
        ctx.fillRect(cx, y, Math.max(1, w - 2), barH);
        cx += w;
      });
    }
    y += barH + 26;

    /* legend rows */
    const rows = model.categories.slice(0, 6);
    ctx.font = "500 19px " + FONT;
    rows.forEach(function (c) {
      ctx.fillStyle = BAND_COLORS[c.bandLabel] || INK.accent;
      roundRect(ctx, barX, y - 13, 11, 11, 3);
      ctx.fill();

      ctx.fillStyle = INK.text;
      ctx.fillText(say(c.label), barX + 24, y - 3);

      const pct = Math.round((c.share || 0) * 100) + "%";
      ctx.fillStyle = INK.muted;
      ctx.font = "500 17px " + FONT;
      ctx.textAlign = "right";
      ctx.fillText(say(c.bytesLabel + "  ·  " + pct), WIDTH - PAD, y - 3);
      ctx.textAlign = "left";
      ctx.font = "500 19px " + FONT;
      y += 30;
    });

    y += 16;

    /* --- notable discoveries ---------------------------------------- */
    ctx.fillStyle = INK.faint;
    ctx.font = "700 13px " + FONT;
    tracked(ctx, say("NOTABLE DISCOVERIES"), PAD, y, 2.6);
    y += 30;

    if (model.leads.length) {
      ctx.font = "500 20px " + FONT;
      model.leads.slice(0, 5).forEach(function (lead) {
        ctx.fillStyle = INK.accent;
        ctx.beginPath();
        ctx.arc(PAD + 4, y - 6, 3.4, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillStyle = INK.text;
        ctx.fillText(say(lead), PAD + 22, y);
        y += 30;
      });
    } else {
      ctx.fillStyle = INK.muted;
      ctx.font = "500 20px " + FONT;
      ctx.fillText(say("No single concentration stood out."), PAD, y);
      y += 30;
    }

    /* --- the operator's note ---------------------------------------- */
    if (model.summary) {
      y += 18;
      ctx.fillStyle = INK.accentSoft;
      roundRect(ctx, PAD, y - 26, barW, 66, 12);
      ctx.fill();
      ctx.strokeStyle = "rgba(143,123,240,0.35)";
      ctx.lineWidth = 1;
      ctx.stroke();
      ctx.fillStyle = INK.text;
      ctx.font = "italic 500 20px " + FONT;
      ctx.fillText(say("“" + model.summary + "”"), PAD + 24, y + 14);
      y += 66;
    }

    /* --- footer ----------------------------------------------------- */
    const footY = HEIGHT - 116;
    ctx.strokeStyle = INK.line;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(PAD, footY - 34);
    ctx.lineTo(WIDTH - PAD, footY - 34);
    ctx.stroke();

    drawLuma(ctx, PAD, footY - 22, 1.05);

    const footX = PAD + 118;
    ctx.fillStyle = INK.muted;
    ctx.font = "500 17px " + FONT;
    ctx.fillText(
      say("C.U.R.E · Clean USB Rescue Engine · github.com/MadB0i/C.U.R.E"),
      footX,
      footY
    );
    ctx.fillStyle = INK.faint;
    ctx.font = "500 15px " + FONT;
    const footLine = model.completeness
      ? model.caseId + " · " + model.completeness
      : model.caseId;
    ctx.fillText(say(footLine), footX, footY + 26);

    ctx.fillStyle = INK.faint;
    ctx.font = "500 15px " + FONT;
    ctx.textAlign = "right";
    ctx.fillText(
      say("No paths, names or file names are included in this image."),
      WIDTH - PAD,
      footY + 26
    );
    ctx.textAlign = "left";

    /* --- synthetic marker ------------------------------------------- */
    if (model.demo) {
      ctx.save();
      ctx.translate(WIDTH / 2, HEIGHT / 2);
      ctx.rotate(-Math.PI / 9);
      ctx.fillStyle = "rgba(240,180,92,0.16)";
      ctx.strokeStyle = "rgba(240,180,92,0.75)";
      ctx.lineWidth = 3;
      const rw = 660;
      const rh = 96;
      roundRect(ctx, -rw / 2, -rh / 2, rw, rh, 12);
      ctx.fill();
      ctx.stroke();
      ctx.fillStyle = "rgba(240,180,92,0.95)";
      ctx.font = "700 44px " + FONT;
      ctx.textAlign = "center";
      tracked(ctx, say("SYNTHETIC DEMO"), -trackedWidth(ctx, "SYNTHETIC DEMO", 6) / 2, 16, 6);
      ctx.textAlign = "left";
      ctx.restore();
    }

    const dataUrl = canvas.toDataURL("image/png");
    return {
      canvas: canvas,
      dataUrl: dataUrl,
      width: WIDTH,
      height: HEIGHT,
      texts: texts,
      model: model,
    };
  }

  /* ---------------------------------------------------------------------
     Public API
     --------------------------------------------------------------------- */
  window.CureCaseFile = {
    allowlist: allowlist,
    render: render,
    width: WIDTH,
    height: HEIGHT,
    /* The exact list of strings the renderer would draw. The privacy test
       inspects this list rather than trying to read pixels. */
    audit: function (caseFile) {
      return allowlist(caseFile);
    },
  };
})();
