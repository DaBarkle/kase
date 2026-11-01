# Kase design rules

Kase is an **augmented Dynamic Island**. Apple's Live Activities and Dynamic Island
guidance is the base layer: shapes, margins, type, colour, motion and, above all, calm
behaviour. On top sits a deliberate **rich layer** that Apple's island doesn't have (the 3D
case renders), with its own rules for where and when it appears. The app window follows
Apple's iOS grouped style, with numbers from Konsta UI's iOS theme (MIT) where Apple gives
none.

Every visual change is checked against this file and recorded with the journey suite
(`tools/visual-qa`, `just visual-journeys`). What the island should do in each real-life
moment is in [JOURNEYS.md](JOURNEYS.md).

Sources: Apple HIG "Live Activities", WWDC23 "Design dynamic Live Activities" (10194) and
"Animate with springs" (10158), Konsta UI iOS theme. We follow Apple's rules and numbers;
we don't ship Apple's design kits or SF fonts (Kase uses Inter).

## 1. The island (base layer)

### Presentations

| Presentation | Use | Content |
| --- | --- | --- |
| Compact | Always-on or after an event, in Compact mode | L, R and case levels, one glyph |
| Alert | One thing happened (charging, low, mic, noise control, elsewhere) | One line and one level ring |
| Card (expanded) | The whole picture: name, state and three rings | Glanceable only |
| Case card (rich) | Opening or putting away the case | Card plus the 3D case clip |
| Quick look (rich) | Tray click | Card plus one control row (noise control) |

One thing the person does is told as **one** presentation. A presentation that is already
on screen updates in place rather than being replaced.

### Shape

| Rule | Value |
| --- | --- |
| Compact height | 36 (Apple: 36.67) |
| Alert height | 48 |
| Card height | 112 (Apple's range 84 to 160); case card 268, a deliberate rich-layer exception |
| Card width | 344 with buds, 252 for headphones |
| Inner margin | 14 on every side (Apple's 14 pt), even left and right |
| Corners | Pills: height / 2. Card: 30. Inner shapes are concentric: inner radius = outer radius - margin (card: 16) |
| Background | Opaque black, always, in both themes |
| Keyline | Tinted by the event (green charging, orange low, red critical, white otherwise), 15% at rest |

All sizes are logical pixels at the Medium size; the size setting scales them together.

### Type (Inter)

| Use | Size, weight |
| --- | --- |
| Card title | 15 Semibold |
| Ring numbers | 16.5 Bold |
| Labels and secondary lines | 12 Medium |
| Compact numbers | 13.5 Semibold |
| Alert text | 15 Semibold |

Nothing on the island is lighter than Medium: it sits over anything, at a glance.

### Content

- Glanceable only: left, right, case, and one short state line.
- At most one control (the quick look's noise control row counts as one).
- Numbers are never shown for a value Kase doesn't know: an unknown or "0%, case
  disconnected" reading keeps the last known value, dimmed.

## 2. Motion (base layer)

Both spring engines use Apple's formula, so a spring is described by its duration and
bounce: stiffness = (2π / duration)², damping = 4π (1 - bounce) / duration, mass 1. The
named springs live in one place, `src/ui/tokens.rs`, used by the island and the window.

| Token | Duration (s), bounce | Used for |
| --- | --- | --- |
| `SMOOTH` | 0.50, 0.00 | Position and size changes with no overshoot |
| `FILL` | 0.80, 0.00 | Battery rings filling |
| `SNAPPY` | 0.32, 0.15 | Selection highlights, segmented thumbs, page moves |
| `GROW` | 0.50, 0.22 | The island growing into a card or alert, the quick look opening |
| `SHRINK` | 0.42, 0.04 | The island folding back |
| `FADE_IN` / `FADE_OUT` | 0.26 / 0.30, 0.00 | Content cross-fades |
| `POP` | 0.40, 0.30 | A value changing inside an open card |
| `PRESS` | 0.26, 0.30 | Buttons pressed |
| `SHAKE` | 0.22, 0.40 | "Not possible" (mic unavailable) |

Rules:

1. Bounce is never above 0.4.
2. Base-layer animations finish within 2 s. The charging sheen plays one 1.8 s pass.
3. Numbers roll (0.36 s); text swaps cross-fade; things that stay on screen move, they
   don't disappear and reappear.
4. Every animation can be interrupted and reversed mid-way (springs keep their velocity).
5. Reduce motion: shape changes jump, content only fades, rich clips show their last frame.

## 3. Colour (one palette for the island and the window)

Apple's system colours; the island always uses the dark values.

| Token | Light | Dark | Meaning |
| --- | --- | --- | --- |
| `green` | #34C759 | #30D158 | Charging, connected, healthy |
| `orange` | #FF8D28 | #FF9230 | Low: 20% or less |
| `red` | #FF383C | #FF4245 | Critical: 10% or less, errors |
| `blue` | #0088FF | #0091FF | Accent, selection, links |
| `gray` | #8E8E93 | #8E8E93 | Inactive, unknown |

Text: primary black 88% / white 92%, secondary 55%, tertiary 30%. Separators are hairlines:
black 20% (light) / white 15% (dark). The window's backgrounds and surfaces keep Apple's
grouped colours. In the light theme, the status pill is white with a hairline edge, and white
knobs (toggle, slider) get a hairline edge and a soft shadow.

## 4. The app window

| Element | Value |
| --- | --- |
| Window | 420 x 680 by default, so every AirPods page fits without scrolling |
| Spacing grid | 4: 4, 8, 12, 16, 20, 24, 32 |
| Page side margin | 16 |
| Groups (cards) | Radius 20; rows padded 13 vertical, 16 horizontal; rows at least 48 tall |
| Section labels | 12 Semibold uppercase, secondary colour; 22 above, 8 below |
| Type | Page title 26 Semibold; empty-state title 20 Semibold; row title 14 Medium; body 14 Regular; secondary 13 Regular; caption 12 Regular; ring label 13 Medium |
| Segmented control | Height 32, radius 10 |
| Toggle | 44 x 26 |
| Slider | Track 4, knob 20 |
| AirPods tab hero | Always present, so the page keeps its shape: a 200 px slot with the 3D render for AirPods 4; a 56 px slot with the model glyph for others (they carry more controls, and the page must fit) |

## 5. The rich layer (Kase's extension)

| Rule | Value |
| --- | --- |
| Where | Case card, the AirPods tab hero, first connect of the day, putting away. Never on Compact or Alerts. |
| When | Only for the moments in JOURNEYS.md that name a clip; not on routine updates. A reward for moments that matter, not decoration. |
| Size | Case card art 150 px inside a 156 px block; card 268 tall. Window hero slot 200 px (56 px glyph slot for models without a render). |
| Timing | The clip starts once the card has finished growing (about 0.45 s); it may run up to 2.6 s; battery numbers appear with the card and never wait for the clip. |
| Honesty | The clip matches the real state: a closed case for put away and out of range, open with pods for live. |
| Fallbacks | Never over fullscreen games; Reduce motion shows the clip's last frame; missing frames fall back to the glyph. |
| Family | Same black, corner radii and springs as the base layer, so it reads as one island. |

## 6. Checks

The journey suite checks every frame of every journey for: a presentation replaced in
under 1.2 s, the island cut off at its surface edge, and base-layer motion longer than 2 s.
Before and after filmstrips go on every pull request that changes how Kase looks.
