# LUMA — companion verification

This document records what the companion **is**, what was measured, and
how each claim was checked. Every statement here is either observed from a
render of the shipped rig or verifiable by running the checks named.

LUMA is a ginger tabby with amber eyes, a violet tech collar, a cream
belly and white paws. She is drawn as a vector rig, not a picture: a
named-joint hierarchy with twelve authored poses, a four-joint tail and a
seeded beat scheduler.

---

## The character

| Property | Value | How it was checked |
|---|---|---|
| Drawing | Inline SVG, built at runtime, 42 paths + 15 ellipses + 6 circles + 4 rects in 30 groups | `document.querySelectorAll` on the live rig |
| Palette | Warm ginger coat, cream belly and bib, white socks, pink nose, amber eyes, violet collar, amber tabby stripes and tail rings | Rendered at 3× and inspected |
| Proportions | 152 × 136 viewBox, chibi: head 34 % of the height, big eyes, small paws | Measured from the live SVG |
| Ears | Pointed, with a pink inner triangle | Rendered at 3× |
| Eyes | Amber radial gradient, slit pupil in its own group, two glints, a wet-line arc, a shaped lid | Rendered at 3×; the lid is a path, not a rectangle |
| Whiskers | Six, three per side | Rendered at 3× |
| Tail | Four joints (`tailBase → tail1 → tail2 → tailTip`), tapering strokes 10.6 → 6.2, with a tuft and a cap | Measured from the rig and from the CSS |
| Stripes | Forehead "M", two cheek stripes per side, three flank stripes per side, three tail rings | Counted in the live rig |

### The artwork refresh

A later pass warmed the ginger coat and amber iris palette and added static
definition inside the existing rig: a cream blaze and chin lift, a brighter
iris ring, a nose highlight, coat fur tufts, visible sock cuffs and a collar
sheen. The sleeping pose gained the same collar detail. These are drawing
changes only — no state names, mappings, triggers, durations or animation
engine behaviour were changed, and the character checks below still pass.
The before/after at the same idle pose and scale is
`docs/screenshots/15-luma-before.png` and
`docs/screenshots/16-luma-after.png`.

### The rim light actually paints

The earlier build traced a path out along the back edge and back along the
identical reversed curve. That path has **zero area**, so it rendered
nothing at all, while the documentation claimed "rich 3D contouring".

The rim is now a **stroke** along the back curve, filled with a linear
gradient that fades down the flank. The check in the browser suite
measures the painted bounding box and fails if it is degenerate:

```
the rim light covers a real area: {'present': True, 'w': 33, 'h': 64}
```

### The eyelid is an eyelid

The earlier build drew the lid as a plain rectangle 23.2 units wide over an
eye 17.6 units wide. When the eye closed, the result was a black
rectangle across the face with a visible seam — a visor, not an eyelid.

The lid is now a path that follows the eye's width and ends in a soft
curve, with a separate lash line. The suite checks that the lid is not a
rectangle and that it is no wider than the eye:

```
the eyelid is shaped like an eye, not a visor: {'tag': 'path', 'eyeW': 132, 'lidW': 132}
```

### The coat follows the theme

The gradient stops used literal colours read at load time, so the theme
palette only affected the parts painted from CSS — the body and head
ignored the palette entirely. The stops now reference the custom
properties directly, so a light theme gets a lighter cat.

The cream alternative was rendered from the same rig for comparison. On the
graphite console it is clearly the worse choice: the coat, the belly, the
bib and the paws merge into one pale mass, the stripes all but vanish and
the face loses its definition. **The ginger tabby was selected.** The light
theme gets a deeper ginger with a firmer outline instead of a pale cat.

---

## The two poses

LUMA is not one picture. The rig carries two:

* the **standing** cat — pear body, four-joint tail, upright head;
* the **curled** cat — one continuous mass, head tucked onto the paws,
  tail wrapped around the front, one white paw showing, one closed eye.

The earlier build described sleeping as "curled, slow, warm" while
rendering a sitting cat with closed eyes. The sleeping state now
cross-fades to a genuinely curled body.

### The fold is authored, not dissolved

The two drawings used to cross-fade over each other, so for a moment both
cats were on screen at once. The recording of that is
`docs/native-captures/luma-sleep-before.png`: frames 7–10 of that strip
show the standing cat at partial opacity with the curled one already
arriving underneath it.

The change is a **fold**, sequenced so the two drawings are never on
screen together. The standing cat sits down, compresses, drops its head
and tail, and is fully transparent by 48 % of the transition; the curled
cat does not begin to appear until 48 %. Waking is the same move in
reverse.

Measured in the browser, sampling the computed opacity of both drawings
every 40 ms across the whole transition:

| Direction | Frames where both drawings are visible |
|---|---|
| Standing → curled | **0** |
| Curled → standing | **0** |

The same measurement against the previous build is `docs/native-captures/luma-sleep-before-detail.png`;
the corresponding new recording is `docs/native-captures/luma-sleep-after.png`.

### The standing cat is not still

The ambient movement used to be declared twice for the same joints: the
"idle" block and the "standing motion" block both set `animation` on
`.lm-body`, and the later rule won. The consequence was that **the
breathing never ran** and the **purr was replaced by the weight shift**
when the companion was petted.

The layers are now split across the independent `transform`, `translate`
and `scale` properties, so they compose instead of cancelling:

| Layer | Property | Period |
|---|---|---|
| Weight shift | `transform` | 7.2 s |
| Breathing | `scale` | 4.9 s |
| Head lean and ride | `rotate` / `translate` | 7.2 s / 4.9 s |
| Paw load, hind-foot load | `translate` | 7.2 s |

Measured over 7.8 s of idle samples, the number of distinct values each
joint took:

```
body transform 28   head rotate 28   head translate 28
body scale 28       paw L translate 19  paw R translate 19
tail 28
```

A joint that never moves is a defect, and the standing cat now moves at
four different rates on six different joints.

### The inspector is held, not floating

A prop that floats in front of her is not being held. The device is now
carried by three layers drawn in a deliberate order: the **forearms**
rise from the shoulders behind it, the **device** sits between them, and
the **paws** close over its lower corners in front of it. The standing
rig's own forelegs step aside while it is held, so she is never left with
four legs.

The seal is pressed by a different limb, because a paw pressing a lid
comes from above, not from the side.

### The curled pose was redrawn

The earlier curled drawing was a head beside a lump: the head sat
outside the body silhouette, the "paws" were a white ellipse in the
middle of the back, and the tail was a pale crescent that read as a
second egg. It is now one loaf — haunch at the right, back arcs over the
top, head down on the front paws at the left, tail wrapped around the
near side and lying along the front under the chin — with a wider floor
shadow to match.

---

## The rig and the animation

| Property | Value | Evidence |
|---|---|---|
| Rig | Named-joint nested groups with explicit `transform-origin` and `transform-box: view-box` | `luma.css` joint table |
| Tail | Four joints, independent periods 5.8 s / 4.4 s / 3.4 s / 2.7 s — a travelling wave | CSS keyframes |
| Paws | Single groups, not multi-segment limbs | `luma.js` |
| Expressions | Per-state CSS poses across five channels: brow angle, iris, pupil shape, nine mouth paths, ear and blush | `luma.css` |
| Idle | CSS loops plus a deterministic seeded beat scheduler every 3.4–8.6 s | `luma.js` `BEATS` |
| States | 12: idle, scan, discovering, curious, thinking, review, cleanup, quarantine, success, failure, cancelled, sleeping | `luma.js` `KIND_LABEL` |
| Props | One per active state, drawn as vector geometry — no text, no raster | `luma.js` `PROPS` |
| Determinism | A seeded LCG, so a run is reproducible | `luma.js` `makeRng` |

---

## Behaviour

| Property | Result | How it was checked |
|---|---|---|
| Reduced motion | The operating system's preference stops every animation; the copy says so | Emulated in the native app: `data-paused="true"`, **0** running `lm-` animations |
| Unfocused window | Motion stops when the window loses focus, so a minimised or backgrounded app is not burning a frame budget | `luma.js` `syncMotion` |
| Petting | The artwork is the hit region. Clicking an empty corner of the canvas is not a pet | Browser suite: `clicking empty space is not a pet` |
| One cat at a time | Petting one companion purrs that companion only | Browser suite: `petting one cat purrs one cat` |
| Keyboard | The stage is a real control: `role="button"`, `tabindex="0"`, a label, and Enter or Space pets | Browser suite: `the companion can be reached and operated from the keyboard` |
| Scoped personas | A host that asks for its own companion keeps it. A global event never overwrites it | Browser suite: `a global event does not overwrite a screen-specific companion` |
| Live states | All twelve interface-reachable states, including the major-concentration pose, were driven in the **native** application | `testing/native/native-e2e.py` |
| Animation tracks | Finished tracks are released whatever they animated. The earlier code only released the ones that targeted the same node, so a long session accumulated animations without bound | `luma.js` `beat()` |
| Teardown | `CureCompanion.destroy(host)` cancels the animations, clears the timers, removes the listeners and empties the host | `luma.js` `destroy()` |
| Multi-instance safety | Each mount builds its own rig with a unique suffix, so gradient ids never collide | Measured live: `dfur_4`, `drim_4`, `diris_4` |

---

## Restraint

The companion is vector only. There is no `<text>`, no raster `<image>`
and no glow filter anywhere in the rig; the "z" glyphs are drawn as
zigzag strokes so the character carries no language of its own. The
browser suite checks this on every state.

---

## Reproducing the checks

```powershell
# The browser suite, which covers the coat, the rig, the lid, the rim,
# the petting hit region, the keyboard route and the scoped personas.
python gui/devtools/evidence-console.py all

# The native journey, which drives the real release binary and reaches
# every state the interface can ask for.
cargo build --release --manifest-path gui/src-tauri/Cargo.toml
python testing/native/native-e2e.py
```

## Known limits

* The paws are single groups rather than multi-segment limbs, so a raised
  paw rotates as a whole.
* The companion is a secondary figure at 96–158 px. The artwork is
  authored for that size and does not carry to poster scale. The
  recordings crop it at 600 px so the motion can be judged; that crop is
  a review artefact, not the shipping size.
* The light theme is a deeper ginger with a firmer outline rather than a
  separately drawn character. It has not been reviewed against a light
  console, because the console has no light theme.
* The arms that carry the inspector are stroked paths rather than
  tapered limbs, so they do not narrow towards the wrist.

---

## The recordings

| File | What it is | Publishable |
|---|---|---|
| `docs/native-captures/luma-showcase.mp4` | Every authored state in order, with a real pet in the middle, cropped to the character | Yes |
| `docs/native-captures/luma-sleep-before.png` | The old cross-fade: both drawings on screen at once | Yes |
| `docs/native-captures/luma-sleep-before-detail.png` | The same moment, frame by frame | Yes |
| `docs/native-captures/luma-sleep-after.png` | The fold: one drawing, then the other | Yes |
| `docs/native-captures/public-demo-walkthrough.mp4` | The storage journey against the deterministic sample backend | Yes |
| `docs/native-captures/native-walkthrough.mp4` | The same journey against the real engine on a real machine | **No** — it contains real paths and file names |

`docs/native-captures/README.md` records which of these may be published
and why.
