# Keyward — DESIGN.md

Design system for Keyward, a macOS/Windows utility that keeps a project's API keys
out of files a coding agent can read. Written to the
[Stitch DESIGN.md specification](https://stitch.withgoogle.com/docs/design-md/specification/).

> Architecture and product scope live in `ARCHITECTURE.md`. This file governs
> appearance only.

---

## 1. Visual Theme & Atmosphere

**Calm instrument.** Keyward sits in the menu bar and is opened for ten seconds at
a time — to check what a key is, or to see who just used one. It is not a workspace
and must never feel like one. The mood is a well-made measuring tool: quiet,
precise, unmistakably native to macOS, with nothing decorative competing for the
one thing the user came to read.

Three commitments follow:

- **Near-monochrome by default.** The interface is neutral grey. Colour appears
  only where it carries state. If a screen has colour in three places, two of them
  are wrong.
- **The accent is neutral.** Graphite in light, near-white in dark. **An earlier
  draft made the accent jade**, on the theory that "the accent *is* the safety
  signal" — one hue for the brand and for "this key cannot be read by any
  program". That was wrong, and it broke the rule directly above it: the app
  ended up with a green button, a green field label and a green status dot all
  competing for the same meaning, and the only one of the three that actually
  carried state — the dot — lost. Jade and amber survive as **status colours and
  nothing else**. With a neutral accent, the two statuses are the only colour in
  the product, which is what the rule was for.
- **Density with air.** Rows are compact (52px min) because lists get long, but
  padding, hairlines and type sizes are tuned so a fifteen-row list still reads
  calmly. Compact is not cramped.

The product is trusted with secrets. Visual restraint is a trust argument: an app
that looks like it is trying to impress you is an app that is spending effort
somewhere other than being correct.

---

## 2. Color Palette & Roles

The implementation of this section is `apps/mac/Keyward/Theme.swift` (`enum Tint`).
Hex values there must match the tables below; when they diverge, this file is the
source of truth and `Theme.swift` is the bug.

### Accent — neutral

The accent is where the product asserts itself: the primary button, the focus
ring, the text link in the popover footer. It is deliberately hueless, for the
reason recorded in §1.

| Token | Light | Dark | Role |
|---|---|---|---|
| `accent` | `#1D1F22` | `#EDEEEF` | Focus ring/border, text buttons, window tint. |
| `accent-top` | `#35393E` | `#FBFBFB` | Primary button gradient, top stop. |
| `accent-bottom` | `#1B1E21` | `#DFE1E2` | Primary button gradient, bottom stop. |
| `on-accent` | `#FFFFFF` | `#16181A` | Text and glyphs drawn on the accent fill. |
| `accent-wash` | `#ECEDEE` | `#2A2D30` | Focus glow behind an input, hover on a text button. |

`accentFill` is `linear-gradient(180deg, accent-top, accent-bottom)` — a vertical
gradient, not the 160° used for service marks. Buttons are lit from above; avatars
are objects lit from the top-left. Different jobs, different angles.

### Status — the only hues

| Token | Light | Dark | Role |
|---|---|---|---|
| `jade` | `#127A56` | `#45C596` | **"Protected"** in prose — the live-session count in the footer. |
| `jade-2` | `#16906A` | `#38B487` | Status dot fill, live dot, activity bars, brokered marker in the usage table. |
| `jade-3` | `#12B189` | `#57DDB2` | Reserved for a highlight stop; unused in the shipping app. |
| `jade-wash` | `#E3F1EB` | `#10312A` | Dot halo, and the fill of the "forwarding now" card. |
| `amber` | `#9A5615` | `#D89B5C` | **"Shared with the program"** — the honest downgrade. Never an accent. |
| `amber-wash` | `#F8EDDF` | `#33261A` | Amber dot halo, and the fill behind an inline error. |

Amber does double duty as the error colour in sheets and the detail pane, which is
consistent with the rule below: a refused write is a weaker outcome to report, not
a catastrophe to alarm about.

There is deliberately **no red** in the palette. Nothing in normal use is an error;
amber means "weaker guarantee", not "something went wrong". Red appears only where
the system supplies it — the `role: .destructive` menu item and the delete
confirmation dialog — and Keyward defines no red token of its own.

### Surfaces & ink

| Token | Light | Dark | Role |
|---|---|---|---|
| `page` | `#EFEFEE` | `#101112` | Behind the window (docs/marketing only). |
| `surf` | `#FFFFFF` | `#1D1F20` | Window body, list rows, cards. |
| `surf-2` | `#F8F8F7` | `#191B1C` | Title bar, footer, table header. Recedes. |
| `surf-3` | `#F1F1F0` | `#232527` | Search field, tag, readonly input. |
| `hover` | `#F4F4F3` | `#26292A` | Row hover. |
| `sel` | `#E9EDEB` | `#2A3230` | Selected row. Faintly green — a survivor of the jade era, kept because at this saturation it reads as "warmer grey" rather than as a hue, and because changing it moves the one fill the eye tracks down the whole list. |
| `line` | `#E6E6E4` | `#2C2F30` | Hairlines, 0.5px. |
| `line-2` | `#D5D5D2` | `#3A3D3E` | Control borders. |
| `ink` | `#16191A` | `#F0F1F1` | Primary text. |
| `ink-2` | `#61666A` | `#9BA1A4` | Labels, secondary. |
| `ink-3` | `#90969A` | `#6E7477` | Metadata, placeholders, masked values. |

### Service marks

Two treatments, because a row avatar is one of two different things.

**When Keyward ships the service's mark** (`stripe`, `openai`, `postgres`, … —
about 45 of them, matched by slug in `keywardd`'s `logo_for`), the logo sits on a
**white chip in both themes**, with a `rgba(0,0,0,.08)` hairline and the glyph at
62% of the chip. Most brand SVGs are a single dark colour — OpenAI's is `#0D0D0D`
— so dropping them straight onto a dark surface makes them vanish. A white chip is
the convention every app with a brand-logo list converges on.

**When it does not** — the common case, since a secret can be for anything — the
row falls back to a monogram on a two-stop diagonal gradient
(`linear-gradient(160deg, base @ 86%, base)`, top-left to bottom-right) with a
`rgba(255,255,255,.28)` inner highlight and the letter at 44% of the chip in bold.
Flat fills look pasted on; the gradient and highlight seat them in the surface.

The monogram's tint is **assigned by the daemon, not the app** — one of eight
fixed hues, chosen by the byte sum of the slug — so a secret keeps the same colour
across restarts and across the two desktop apps, and neither frontend gets to
disagree about it. These colours belong to the services, never to Keyward: they
never appear on buttons, links or anything chrome.

---

## 3. Typography Rules

**System faces only.** `-apple-system` / SF Pro on macOS, Segoe UI Variable on
Windows, with `"PingFang SC", "Hiragino Sans GB"` for Chinese. A native utility
that ships a webfont announces that it is not native. `ui-monospace` / SF Mono for
values, references and timestamps.

Negative tracking scaled to size — SF is optically loose at display sizes and needs
tightening, and needs *less* tightening as it gets smaller.

| Role | Size | Weight | Tracking | Notes |
|---|---|---|---|---|
| Page title (marketing) | 25–34px | 650 | −0.032em | `text-wrap: balance` |
| **Sheet title** | **19px** | **600** | −0.5pt | `Add secret`, `Replace…`, `Tell your AI…` |
| **Detail title** | **26px** | **600** | −0.5pt | The selected secret's name. Editable in place. |
| **Row title** | **14px** | **500** | −0.16pt | The secret's name in the list |
| **Row subtitle** | **12px** | 400 | −0.03pt | "codex in my-shop · 2 min ago", `ink-3` |
| **Field label** | **11.5px** | **500** | 0 | **`ink-2`**, sentence case — see below |
| **Field value** | **14px** | 400 | 0 | **Mono**, always — see below |
| Body / control | 13px | 400/500 | 0 | Buttons, fields, search |
| **Section header** | **11px** | **600** | **+0.66pt** | Uppercase, `ink-3` |
| Table header | 10.5px | 500 | +0.5pt | Uppercase, `ink-3` |
| Usage timestamp | 11px | 400 | 0 | Mono, tabular, `ink-3` |

Tracking is given in points rather than ems because SwiftUI's `.tracking` takes
points; `−0.16pt` at 14px is the `−0.01em` this table used to specify.

**Field labels are `ink-2`, not the accent.** **An earlier draft made them jade**,
borrowing 1Password's trick of a coloured label above a neutral value as a
scannable anchor, and called it the one deliberate exception to §1. The exception
did not survive contact with the rest of the palette: once the accent went neutral
(§1) a jade label was no longer "the product's own colour" but a third meaning for
the status hue, sitting two inches from a status dot that meant something else.
Grey labels lose nothing — the anchor comes from the size and weight step, not
from the hue.

**Field values are monospaced whether or not they are references.** A masked
value (`sk_live_…4f2a`) and a reference (`keyward://stripe`) sit one above the
other in the detail pane, and mixing a proportional face with a mono one across
two adjacent rows of the same card reads as a mistake rather than as a
distinction.

**Everything in this table went up a size after comparing side by side with
1Password.** The old scale — 13.5pt row titles, 12.5pt values, 10.5pt uppercase
labels — was internally consistent and collectively too small: no element was
wrong on its own, and the whole thing read as cramped.

**Rules**

- `font-variant-numeric: tabular-nums` on every timestamp, count and masked value.
  Digits that shift as they update read as sloppy.
- Uppercase only at ≤11px, and always with positive tracking.
- Line-height 1.6 for prose, 1.3–1.4 inside dense rows.
- Never bold a whole row to indicate state — that is the dot's job.

---

## 4. Component Stylings

### Window structure

A **`NavigationSplitView`**, which means the split is left-to-right and runs the
full height of the window. The regions are:

1. **Sidebar**, min 250pt / ideal 286pt / max 360pt, on `surface-2`. It runs the
   whole height, **the traffic lights sit over its top-left**, and it holds, in
   order: the search field, the scrolling list, a hairline, the footer. The system
   sidebar-collapse toggle is removed — it is oversized beside the app's own
   controls and this sidebar is never hidden.

2. **Detail pane**, on `surface`, 28pt side padding, 26pt top, 36pt bottom,
   **32pt between sections**, content capped at 560pt.

3. **Toolbar**, covering **the detail pane only** — not a band across the top of
   both. It holds two trailing actions: the ghost **`For your AI`** and the filled
   **`Add secret`** (⌘N).

**Three earlier structures are recorded here because each failed differently.**

- The first draft was a **full-width 52pt toolbar** with search on the left and
  `＋ Add secret` on the right, over a 268pt list column — the 1Password shape.
  The reasoning was that the two highest-frequency actions belong at the top at
  full size. It was replaced because **search filters the list and belongs to
  it**: in the toolbar, floating over the detail pane, the field belonged to
  nothing it affected, and the traffic lights, the search field and the column's
  first row were three left edges at three different insets.
- Before that, an attempt to **hide the title bar and draw a custom 52pt band**.
  The band drew fine, but the traffic lights keep their system position at the top
  of the window while a search field centres in the band — the two were ~10pt out
  and no band height made both right. The app now uses
  `.windowToolbarStyle(.unified(showsTitle: false))` and hands the whole strip to
  AppKit, which is the only thing that knows where it puts the lights.
- The list column briefly kept **`＋ Add secret` in its footer**, and that *was* a
  demotion: a primary action rendered at 12.5pt in a footer is not the primary
  action. It survives in the popover only, where there is no toolbar to put it in.

**Window size.** Default 780×560, minimum 640×460. **This reverses §5's old "keep
the window under 620px, resist adding a sidebar."** That rule was written for a
one-column list with no detail pane; the moment the detail view gained a usage
table with four columns and a fourteen-day activity strip, 620pt could not hold a
list *and* that table, and the honest options were a narrower table or a wider
window. The sidebar is not the feature-creep the old rule was guarding against —
it is the same list, given a fixed home so selecting a row does not replace it.

### Selection

**`sel` fill with a 0.5px `line-2` border**, radius 8, inset 6pt horizontally and
1pt vertically from the column's edges.

**An earlier draft made this a filled jade rectangle with white text**, on the
argument that selection is the app telling you where you are and should be the
strongest visual event in the window. Two things killed it. It needed the accent
to be a colour, and the accent is now graphite (§1) — a filled graphite row is a
black bar through the middle of a grey list, which is louder than "where you are"
needs to be. And a filled row forces every child to invert: the status dot in
particular had to turn white, which removed the only thing the dot was for, at
exactly the moment the user was looking at it. The dot now **keeps its own colour
in a selected row**, and the fill stays quiet enough to let it.

The border is what stops the quiet fill losing to hover — `sel` and `hover` are
two greys three steps apart, and the hairline is the difference the eye actually
reads.

### List row — the primary component

The product is a list; this row is where craft is judged.

- Horizontal stack, gap 11px, padding 11px 15px, **min-height 52px**.
- **Avatar 32×32** (28×28 in the popover), radius 29% of the side — 9.3px at 32.
  Brand mark on a white chip, or gradient + inner highlight with the monogram at
  44% of the side, bold.
- **Middle** two lines: name (14px/500, `ink`) over usage (12px, `ink-3`),
  assembled from localised fragments rather than one format string — the word
  order of "codex in my-shop · 2 min ago" differs between the two languages we
  ship.
- **Right**, in order: the masked value (11px mono, `ink-3`), then the dot, then a
  chevron. The masked value and the chevron are **dropped in the popover**, where
  296pt cannot hold them without the name losing. It stays in the window: the
  detail pane shows it too, but a list you have to click through to recognise a
  key is a list that failed at its one job.
- **Separator** is a 0.5px line inset to **52px** (past the avatar), drawn as a
  top overlay on the following row. It is suppressed around the hovered row *and*
  the selected row — the row's own separator and the one above it — so each fill
  reads as a single unbroken block.
- Hover: `hover` background, 130ms; chevron fades in and slides 3px.
- Focus: rows carry no focus ring yet. Text fields and search do (below); the row
  is a plain button and keyboard traversal of the list is not wired up. Listed
  here as a known gap rather than as a spec, because §7 asks for one.

### Status dot

7px circle with a 3px stroked halo in the matching wash. Three resting states:
`jade-2` protected · `amber` shared · `line-2` never used (no halo — a halo on
"nothing has happened" is a glow around an absence). **No text label in the list**
— the legend belongs in the detail view and the footer, not repeated on every row.

### Live dot

The fourth state, and the only animation in the product: while a broker session is
open, the row's dot is replaced by the same 7px `jade-2` circle inside a ring that
scales to 2.6× and fades out over 1.4s, forever. It **outranks the resting
status** — "something is happening right now" is the more useful thing for a dot
to say than "this is the mode it would use".

Motion is spent here and nowhere else on purpose. It is the one state in the
product that changes while you watch it, so it is the one thing that has earned
the right to move. The same dot appears in the footer beside the forwarding count
and in the detail pane's "forwarding now" card.

### Detail pane

Five blocks, top to bottom, 32pt apart, none of which appear unless they have
something to say:

1. **Header** — 52pt avatar, the name at 26px (tap to rename in place; a sheet for
   one text field is ceremony), the status dot **with its word beside it**, and an
   `⋯` menu holding Rename / Replace / Delete. The dot alone was an unlabelled
   speck floating beside the menu; paired with its word it becomes the one thing
   worth reading in the header.
2. **Error strip** — `amber` text on `amber-wash`, radius 9. Present only after a
   write is refused.
3. **Forwarding-now card** — the live dot, "%@ · N requests · the value has not
   left Keyward", and a `Stop` ghost button, on `jade-wash` with a
   `jade-2 @ 28%` hairline. This is the one place the product's central claim is
   visible *as it happens*, so it states the claim rather than a status word.
4. **Fields** — one card, hairline-divided: the masked secret with `Replace`, the
   reference with `Copy`. **Label above value, not beside it.** Sharing one line
   made the label, the value and the action fight for the same horizontal space,
   and the value lost: `postgres://app:…@db/app` truncated to `postgre…/app` in a
   pane with 400pt to spare.
5. **Activity strip and usage table** — fourteen bars, one per day, oldest left.
   The strip is shown only when at least three of the fourteen days have data;
   below that it is a chart of nothing. An empty day keeps a 2pt `line-2` tick
   rather than vanishing — a gap that reads as missing data is worse than one that
   reads as zero — while a day with any use gets a minimum 8pt bar, because
   scaling both from the same peak made the empty days as loud as the data.

The usage table is four columns — time, who, project, how. **"How" is the one
that matters**: it is where a user learns that "brokered" meant the program never
held the value. It gets a 5px dot in `jade-2` or `amber` and the word, and it is
the last column so it is the one the eye rests on.

### Buttons

| Kind | Spec |
|---|---|
| **Primary** | `accentFill` (`180deg, accent-top → accent-bottom`), `on-accent` text 13px/500, padding 7×16, radius 6px, `inset 0 0 0 .5px rgba(255,255,255,.18)` + `0 1px 1.5px rgba(0,0,0,.13)`. Pressed → opacity .82. Disabled → `surf-3` fill, `ink-3` text, `line-2` border, no shadow. |
| **Primary, toolbar** | Same fill and text, tighter: padding 6×13, white border at `.22`, shadow `0 1px 1.5px rgba(0,0,0,.14)`. `Add secret`, with a `+` glyph at 11px/bold. |
| **Ghost** | `surf` fill, 0.5px `line-2` border, `ink` text 13px, padding 7×14, radius 6px, `0 1px 1.5px rgba(0,0,0,.05)`. Pressed → `hover` fill. |
| **Field action** | The `Copy` / `Replace` buttons inside a field row: `surf-3` fill, 0.5px `line-2`, `ink` text 11.5px/500, padding 5×11, radius 6px. |
| **Text** | `accent`, 13px/500. Used for `＋ Add secret` in the popover footer, where there is no toolbar. |

Focus rings are on inputs, not on buttons — see below. Buttons rely on their fill
for prominence and on the sheet's default-action keybinding (⏎) for reachability.

### Input & search

Radius 7px, 0.5px `line-2` border. Fill is `surf` for an editable field and
`surf-3` for a read-only one and for search. **On focus the border becomes
`accent` at 1.5px and an `accent-wash` glow is drawn 3pt outside it**, animated
over 120ms — the neutral equivalent of the old `0 0 0 3px jade-wash`, and the
reason the accent has a wash token at all.

Search carries a `⌘F` keycap — 10.5px, `surf` fill, 0.5px `line-2`, radius 4 —
which disappears once the field is focused or has text. The shortcut is real: an
invisible zero-opacity button behind the field carries it, because a keycap that
lies about what the app does is worse than no keycap.

### Window chrome

**System, not drawn.** `.windowToolbarStyle(.unified(showsTitle: false))`: no
title text, one continuous band, traffic lights where AppKit puts them — over the
sidebar, since the sidebar starts at the window's left edge. The corner radius,
the shadow and the light's own inner ring all come from the OS.

The 37pt hand-drawn title bar this section used to specify is kept below as the
**marketing/mock** spec, since the docs and site render a window that has no
AppKit to borrow from:

> Radius 11px. Title bar 37px, `surf-2`, 0.5px bottom hairline, plus
> `inset 0 .5px 0 rgba(255,255,255,.9)` — the top highlight is what makes a Mac
> window look like a Mac window. Traffic lights 11px with
> `inset 0 0 0 .5px rgba(0,0,0,.14)`.

### Popover

`NSStatusItem` + a transient `NSPopover`, 296×400, hanging from the menu-bar mark.
The enter animation is AppKit's, not the 170ms cubic-bezier this section used to
specify — `NSPopover.animates` is a boolean, and matching a hand-tuned curve was
not worth reimplementing the presentation.

Content is **the sidebar column and nothing else**: search, list, footer, on
`surf` rather than `surf-2` (there is no second pane for it to recede from).
Avatars drop to 28pt and rows lose the masked value and the chevron. Tapping a row
selects it *and opens the main window* — the popover has no detail pane, and a row
that visibly does nothing reads as broken rather than as compact.

`MenuBarExtra` was tried first and produced no status item at all on macOS 26 —
not with a custom image, not with an SF Symbol, not as the only scene in the app.
`NSStatusItem` is what serious menu-bar apps use anyway: it gives control over
click handling, transient behaviour and template rendering, none of which
`MenuBarExtra` exposes. **A debugging note worth keeping**: the original symptom
was "no icon appears", and the cause was neither. On a notched MacBook the menu
bar's right-hand section runs from the notch to the clock, and once it is full
macOS silently drops new status items — no error, no log. Two unrelated menu-bar
apps vanishing at once is the tell.

### App icon

Squircle, `border-radius: 23%`, **graphite**: a three-stop diagonal gradient
`#4A4F55 → #24272B → #111315` (dark `#5A6068 → #2E3238 → #17191C`), top-left to
bottom-right, with a `rgba(255,255,255,.22) → transparent` gloss over the top
half and a `0 4px 10px rgba(0,0,0,.18)` drop shadow. The mark is white with a
1pt drop shadow.

**The icon was jade until the accent went neutral** (§1) and it followed for the
same reason: a green app icon promises that green means something in the app, and
in this app green now means exactly one thing — a secret's state — which the icon
cannot show.

**The ward glyph occupies 60% of the icon's height** — the first draft had it at
~30% and it read as a scratch.

### Ward glyph

The mark is the *ward* — the notched profile inside a lock that decides what
passes. Not a key, not a shield, not a padlock.

Drawn in a **14×26 design box**, scaled to fit preserving aspect: a circle of
radius 7 centred at (7, 7), swept over the top as the major arc, closing into a
notched stem that ends at y=26. The **bulb is deliberately wider than the stem**
— an earlier version made them equal and the silhouette read as a rounded
rectangle with bites out of it. Geometrically the same idea; visually a block. The
keyhole only reads as a keyhole when the head overhangs the shaft.

```
Stem vertices after the arc, (x, y) in the 14×26 box:

Large (≥24pt) — two notches
(10,16) (7,16) (7,18) (10,18) (10,21) (7,21) (7,23) (10,23) (10,26) (4,26)

Small (≤20pt, menu bar) — one notch
(10,17) (7,17) (7,20) (10,20) (10,26) (4,26)
```

Two notches collapse into mush below ~20pt, so the menu-bar rendition drops to one.
Shipping a second path for small sizes is normal icon practice, not a compromise.
The menu-bar image is rendered from this same shape rather than a separate asset,
and is marked `isTemplate` so the system tints and inverts it.

Both renditions are shown side by side **inside the shipping Settings screen**,
next to the app icon. That is not a style-guide page that drifted into the
product: it is the fastest way to notice the small variant regressing, and it
costs three views.

---

## 5. Layout Principles

**4px base grid.** Every spacing value is a multiple of 4, with 2px permitted for
optical nudges only.

| Step | Use |
|---|---|
| 4px | Icon-to-label, chip internals |
| 8px | Sibling controls, avatar-to-text |
| 12px | Row padding, card padding |
| 16–20px | Section gaps inside a window |
| 26–32px | Page-level gaps |

**Radii** climb with the size of the thing: 4px keycap · 6px small control ·
7px input · 8px avatar/row focus · 9px card · 11px window · 12px popover · 23% icon.
A small radius inside a large one always looks wrong; keep the family consistent.

**Widths.** Main window 780×560 by default, 640×460 minimum; sidebar 250–360pt,
286pt ideal; detail content capped at 560pt. Popover 296×400. Sheets 400pt wide
(560pt for the agent-instructions sheet, which shows a code block). Settings 460pt.
Prose caps at 54–56 characters.

An earlier version of this section capped the main window at **620px and forbade a
sidebar**. See §4 — the cap did not survive the detail pane gaining a four-column
usage table, and the sidebar turned out to be the list itself rather than the
navigation tree the rule was written against.

**Alignment.** One left edge per column, held down the whole list. The masked value
column is right-aligned against the status dot so the dots form an unbroken vertical
line — that column is the fastest scan in the product and must not jitter.

---

## 6. Depth & Elevation

Depth comes from **hairlines and layered shadows**, never from borders thicker than
0.5px or from large blurred glows.

| Level | Shadow | Used by |
|---|---|---|
| 0 — flush | none; separated by `line` hairline | Rows, table cells |
| 1 — raised | `0 0 0 .5px line, 0 1px 2px rgba(0,0,0,.04)` | Cards, tables |
| 2 — control | `0 1px 1.5px rgba(0,0,0,.05)` | Ghost buttons |
| 3 — popover | `0 0 0 .5px rgba(0,0,0,.14), 0 2px 6px rgba(0,0,0,.06), 0 16px 40px rgba(0,0,0,.20)` | Menu-bar popover |
| 4 — window | `0 0 0 .5px rgba(0,0,0,.14), 0 4px 10px rgba(0,0,0,.07), 0 24px 56px rgba(0,0,0,.20)` | App window |
| 5 — icon | `0 1px 1px rgba(0,0,0,.10), 0 8px 20px rgba(0,0,0,.18)` | App icon |

Every floating surface starts with a **0.5px spread ring** before any blur. That
ring is what keeps an edge crisp on a light background; blur alone gives a soft,
cheap edge.

**Levels 3 and 4 are AppKit's in the shipping app.** The popover and the window
take their shadow, radius and edge from the system; the values above are the spec
for the marketing site and the mocks, which have no AppKit to borrow from. Levels
0–2 and 5 are drawn by the app.

Level 1 in the app is currently the **ring only** — cards (the fields card, the
usage table, the settings group, the live card) carry a 0.5px `line` stroke and no
blur. The 1px blur is in the spec because it is what the mocks use; a card sitting
flush on `surface` inside a pane that is already on `surface` gains nothing from
it, and inside the popover it made the list look like it was floating over itself.

**Dark mode is not an inversion.** Shadow opacities roughly triple (0.20 → 0.55)
because a dark surface on a dark ground separates only by shadow. The top
highlight drops from `rgba(255,255,255,.9)` to `.10` — a bright highlight on a dark
title bar looks like a rendering bug.

---

## 7. Do's and Don'ts

**Do**

- Let the status dot carry state. It is the only thing on a row allowed to be
  coloured.
- Use 0.5px hairlines. 1px reads as a wireframe at Retina density.
- Inset separators past the avatar, and dissolve them on hover.
- Give every interactive element a visible focus ring — this app is used with the
  keyboard between two other windows. (Inputs have one; list rows do not yet. §4.)
- Say what a downgrade means in plain words when a key must be handed to a program.
- Keep the sidebar the *list*. It is one flat column of secrets with a search
  field over it — the moment it grows a tree, §7's "don't group the list" is gone.

**Don't**

- **Don't give the accent a hue.** It is graphite so that jade and amber are the
  only colour in the product and both mean something. A coloured accent is a third
  meaning competing with two that carry state — see §1 for what that cost.
- **Don't add a third status colour.** Jade and amber carry meaning; a third hue
  would have to mean something, and there is nothing left for it to mean.
- **Don't put security jargon in the UI.** No "tier", "T1", "T2", "injection",
  "policy", "grant". A secret's state is 保护中 / Protected and
  会共享给程序 / Shared with the program; never used is 还没用过 / Never used.

  One word is allowed through: the usage table's "how" column says
  **转发 / Brokered** and **交出 / Handed over**. It is jargon, and it is the
  exception because that column is the only place a user can learn *why* the two
  statuses differ — a past-tense verb about one recorded request is teachable in a
  way that a noun on a settings row is not. It appears nowhere else, and nothing
  the user must understand depends on reading it.
- **Don't offer a security-level picker.** Keyward chooses the strongest mode
  available and reports it. A segmented control gives weak options equal visual
  weight, and asks the user to learn a model to stay safe.
- **Don't use red for the amber state.** It is a weaker guarantee, not a failure,
  and crying wolf costs the colour its meaning.
- **Don't show a full secret anywhere.** Masked (`sk_live_…4f2a`) always; revealing
  is a deliberate, confirmed action.
- **Don't animate anything above 200ms** except the live dot, which is the one
  state that changes while you watch it (§4). Honour `prefers-reduced-motion`.
- **Don't group the list.** No folders, no tags, no project tree. Project context is
  observed and shown inside the row, never a hierarchy the user maintains.

---

## 8. Responsive Behavior

A desktop app, so "responsive" means window resizing and the marketing site — not
phones. Still specified, because the docs and site share these tokens.

| Breakpoint | Behaviour |
|---|---|
| ≥ 900px | Full layout. Marketing grids at `repeat(auto-fit, minmax(238px, 1fr))`. |
| 780–900px | Window at default width, centred; page padding 24px. |
| 640–780px | Sidebar shrinks toward its 250pt minimum before the detail pane gives up anything. |
| ≤ 640px | The app's own minimum. On the marketing site the window fills width minus 20px, the popover centres instead of right-aligning, and note grids collapse to one column. |
| ≤ 420px | Masked-value column hides; row keeps name, usage line and dot — the popover's layout, reached by a different route. |

**Touch targets** 44×44 minimum on any touch-capable surface. The 52px row height
satisfies this without a separate mobile spec.

**Overflow.** Tables and mono values scroll inside their own `overflow-x: auto`
container. The page body never scrolls sideways.

**Appearance and language are user settings, not just media queries.** Light /
Dark / System and 中文 / English switches ship in Settings, alongside a
launch-at-login toggle — three rows, which is the whole settings screen of a tool
opened for ten seconds at a time. On the web the theme is applied by stamping
`data-theme` on the root, which must override `prefers-color-scheme` in both
directions; in the app it is `preferredColorScheme` on the scene.

Chinese and English strings differ in length by up to 40%, so no row may depend on
a fixed text width.

**One trap worth recording**, because it produced a sheet with English chrome
around a Chinese body twice: `String(localized:)` resolves against the *system*
language and ignores the environment locale, so an in-app language switch moves
every `Text` while stranding strings built in code. Strings assembled in Swift go
through a bundle chosen from the same source SwiftUI uses — the environment
locale's **language**, not `locale.identifier` (which is the *region*: on a Mac
set to English in China it is `zh_CN`) and not `Bundle.preferredLocalizations`
(which reads a separate list that can order differently). Date formatters need the
same treatment or timestamps stay English while every label around them switches.

---

## 9. Agent Prompt Guide

**Quick tokens**

```
accent (neutral)    #1D1F22  (dark #EDEEEF)
accent gradient     #35393E → #1B1E21  (dark #FBFBFB → #DFE1E2)
on accent           #FFFFFF  (dark #16181A)
accent wash         #ECEDEE  (dark #2A2D30)   focus glow only
protected / dot     #16906A  (dark #38B487)
protected, prose    #127A56  (dark #45C596)
shared / warning    #9A5615  (dark #D89B5C)
surface             #FFFFFF  (dark #1D1F20)
recessed surface    #F8F8F7  (dark #191B1C)
sunken (fields)     #F1F1F0  (dark #232527)
hairline            #E6E6E4  (dark #2C2F30)   always 0.5px
control border      #D5D5D2  (dark #3A3D3E)
text                #16191A  (dark #F0F1F1)
text secondary      #61666A  (dark #9BA1A4)
text metadata       #90969A  (dark #6E7477)
radius   4 / 6 / 7 / 8 / 9 / 11 / 12 / 23%
spacing  4 / 8 / 12 / 16 / 20 / 26 / 32
type     -apple-system; mono ui-monospace; tracking negative, scaled to size
```

**Prompts that produce correct output**

> Build a macOS list row for a secrets manager: 32px avatar at radius 9 — a brand
> logo on a white chip, or a diagonal gradient with a bold monogram — then the
> name at 14px/500 tracking −0.16pt over a 12px metadata line in `#90969A`, then a
> right-aligned 11px monospace masked value, a 7px `#16906A` status dot with a 3px
> wash halo, and a chevron that fades in and slides 3px on hover. 0.5px separator
> inset to 52px, dissolving around the hovered and selected rows. Padding 11×15,
> min-height 52px.

> Style a primary button: `linear-gradient(180deg,#35393E,#1B1E21)`, white 13px/500,
> padding 7×16, radius 6px, `inset 0 0 0 .5px rgba(255,255,255,.18)` and
> `0 1px 1.5px rgba(0,0,0,.13)`. Pressed drops to 82% opacity. Never tint it.

> Style a focused text field: radius 7, fill `#FFFFFF`, border `#1D1F22` at 1.5px,
> and an `#ECEDEE` glow drawn 3pt outside the border. Unfocused it is a 0.5px
> `#D5D5D2` border and no glow. Animate over 120ms.

> Give this window macOS depth: radius 11px, 37px `#F8F8F7` title bar with a 0.5px
> bottom hairline and `inset 0 .5px 0 rgba(255,255,255,.9)`, and a shadow of
> `0 0 0 .5px rgba(0,0,0,.14), 0 4px 10px rgba(0,0,0,.07), 0 24px 56px rgba(0,0,0,.20)`.
> (Marketing/mock only — the shipping app takes all of this from AppKit.)

**Say this to stay on-system**

"Near-monochrome; colour only where it carries state." ·
"The accent is graphite, deliberately. Jade and amber are statuses, not brand." ·
"Jade means the key cannot be read by any program." ·
"0.5px hairlines, layered shadows, no thick borders." ·
"No security jargon in the interface."

**Never ask an agent for**: a coloured accent, a third status colour, a
security-level picker, red status, folders or tags in the list, a webfont, or a
full secret displayed on screen.
