# Media Care — Layout & Style Design Instructions
# For: TheGrid Design Collaboration / Claude Implementation

Status: Active Design Spec
Date: 2026-04-29
Authority: Supersedes any external mockup not matching these rules.

---

## 0. Hard Rules — Read First

1. **No new navigation sidebar.** Media Care is a single Screen entry in the existing left rail under `Screen::MediaIngest`. The left nav is UNCHANGED. Do not design a separate nav panel, side menu, or navigation tree inside Media Care.

2. **TheGrid visual language is non-negotiable.** Terminal green on near-black. Zero border radius. Heavy monospace. No gradients, no shadows, no rounded corners, no soft UI. If a mockup shows soft cards, white backgrounds, rounded tiles, or a light theme — reject it.

3. **Internal navigation = flat tab strip only.** Media Care may have one horizontal tab strip at the top of the content area for sub-views (Ingest, Queue, Tool Health). Nothing deeper. No sub-menus, no drawers as navigation.

4. **Extend, do not replace.** The existing Media Ingest culling UI (grid, preview, keyboard shortcuts, filter strip) stays intact as Zone B. Media Care wraps around it.

---

## 1. Design Token Reference (from `crates/thegrid-gui/src/theme.rs`)

Use ONLY these tokens. Never invent new colors.

```
BACKGROUNDS
  BG           #080808   — app root background
  BG_PANEL     #0f0f0f   — panel surfaces
  BG_WIDGET    #141414   — input fields, cards, list rows
  BG_HOVER     #1c1c1c   — hover state on rows/cards
  BG_ACTIVE    #001e08   — selected/active state (subtle green tint)

BORDERS
  BORDER       #1e1e1e   — default separator lines
  BORDER2      #2a2a2a   — slightly more visible dividers

ACCENT
  GREEN        #00ff41   — primary accent: active, selected, confirmed, titles
  GREEN_DIM    #008020   — secondary: subdued values, badges, dimmed state
  AMBER        #ffd600   — warning, in-progress, pending
  RED          #ff2244   — error, failed, reject flag

TYPE-SPECIFIC COLORS (for file type chips)
  VIDEO        rgb(255,104,32)   — orange
  RAW          rgb(255,214,0)    — amber
  PHOTO        rgb(68,136,255)   — blue
  AUDIO        rgb(0,255,65)     — green
  DRONE        rgb(0,229,255)    — cyan

STATE COLORS (for job/queue status)
  ONLINE       #00ff41   — complete/healthy
  SYNCING      rgb(0,180,200)    — running/transferring
  INDEXING     #ffd600   — queued/waiting
  BUSY         rgb(255,130,0)    — active processing
  ERROR        #ff2244   — failed
  OFFLINE      #333333   — cancelled/paused

TEXT
  TEXT         #e8e8e8   — primary readable text
  TEXT_DIM     #666666   — secondary labels, metadata
  TEXT_MUTED   #333333   — placeholder, disabled
```

Typography: **Monospace everywhere. No exceptions.**
- Labels/metadata: 10–11px mono
- Body text: 12px mono
- Section headers: 12–13px mono UPPERCASE
- Screen/panel titles: 14px mono UPPERCASE GREEN

Rounding: **Rounding::ZERO everywhere.** No rounded buttons, no rounded panels, no rounded chips.

---

## 2. Screen Topology — 4 Zones

Media Care occupies the full content area after the existing left nav rail.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  [MEDIA CARE]  [INGEST] [QUEUE] [TOOL HEALTH]           ← Tab Strip (top)  │
├──────────────┬──────────────────────────────┬──────────────────────────────┤
│              │                              │                              │
│   ZONE A     │         ZONE B               │         ZONE C               │
│   SOURCES    │   INGEST GRID + PREVIEW      │   SMART CARE STACK           │
│              │                              │                              │
│   ~220px     │        flexible fill         │        ~280px                │
│              │                              │                              │
├──────────────┴──────────────────────────────┴──────────────────────────────┤
│   ZONE D  —  PROCESSING QUEUE TIMELINE                                     │
│   ~140px fixed height, collapsible to 32px header bar                      │
└─────────────────────────────────────────────────────────────────────────────┘
```

Widths are soft targets. Zones A and C are fixed-width panels with a 1px BORDER separator. Zone B takes all remaining space.

---

## 3. Tab Strip (Top Navigation within Media Care)

One horizontal strip. No icons in tabs — labels only, UPPERCASE monospace.

```
[INGEST ▐] [QUEUE] [TOOL HEALTH]
```

- Active tab: GREEN text, 1px bottom line GREEN, BG_ACTIVE background
- Inactive tab: TEXT_DIM text, no underline, hover → BG_HOVER
- Tab strip height: 28px
- Separator below strip: 1px BORDER line across full width
- The "MEDIA CARE" label at far left of the strip is a static screen title in GREEN, not a clickable tab

Tab views:
- **INGEST** — default view, all 4 zones visible
- **QUEUE** — Zone B becomes queue job list (full width, no Zone A/C), Zone D stays
- **TOOL HEALTH** — Zone B becomes tool status grid (ffmpeg, gyroflow, whisper, etc.)

---

## 4. Zone A — Sources Panel

Width: ~220px. BG_PANEL background. Right border: 1px BORDER.

Header:
```
SOURCES                           [+]
```
- "SOURCES" in GREEN 12px mono UPPERCASE
- [+] button to add watched folder: BG_WIDGET, BORDER border, TEXT label

Source list rows:
- Row height: 26px
- BG_WIDGET background, hover → BG_HOVER, selected → BG_ACTIVE
- Left: source type icon (phosphor, 14px GREEN_DIM) + source name TEXT 11px mono
- Right: item count in TEXT_DIM 10px mono
- Selected row: left 2px accent bar in GREEN

Source type chips below the list (filter by source type):
- Inline horizontal wrap: DRONE (cyan), CAMERA (blue), PHONE (green), AUDIO (green)
- Same chip style as existing media type chips in media_ingest.rs
- Active chip: colored bg at 20% opacity, colored border at 70% opacity, colored text
- Inactive chip: BG_WIDGET, BORDER border, TEXT_DIM text

Session filter row:
```
[TODAY] [WEEK] [UNREVIEWED] [PICKS]
```
- Same chip style as source type chips but TEXT_DIM / BORDER when inactive

---

## 5. Zone B — Ingest Grid + Preview

This is the existing `media_ingest.rs` view. **Do not redesign the grid or the preview panel.** Keep all existing keyboard shortcuts, filter strip, and card layout.

What changes in Zone B when Media Care wraps it:
- Filter strip gains one extra chip group for CARE STATUS: [UNCARED] [QUEUED] [DONE] [FAILED]
- Chip style is identical to existing media type chips
- No other changes to Zone B layout

---

## 6. Zone C — Smart Care Stack

Width: ~280px. BG_PANEL background. Left border: 1px BORDER.

Header:
```
SMART CARE                   [▶ RUN]
```
- "SMART CARE" in GREEN 12px mono UPPERCASE
- [▶ RUN] button: BG_WIDGET, BORDER border, GREEN text, hover → BG_HOVER

Preset shelf (top of Zone C, below header):
```
[QUICK FIX ▾] [SOCIAL] [ARCHIVE] [CUSTOM]
```
- Same chip style. Clicking a preset populates the operation stack below.

Operation cards (scrollable list):

Each card:
```
┌────────────────────────────────────────┐
│ ⬤ IMAGE RESIZE            [≡] [✕]      │ ← header row
│ Width  [1920  ] Height [1080  ] [LOCK]  │ ← params row
│ Scope: [SELECTION ▾]                   │ ← scope selector
└────────────────────────────────────────┘
```
- Card background: BG_WIDGET
- Card border: 1px BORDER
- Header: op name in TEXT 11px mono UPPERCASE, left enable dot (GREEN = on / TEXT_MUTED = off), right drag handle [≡] and remove [✕]
- Enabled dot click toggles the operation
- Params row: minimal inline fields, BG_WIDGET input boxes with BORDER border
- Scope selector: BG_WIDGET dropdown, BORDER border

Operation categories (stack section labels):
```
── IMAGE ──────────────────────────────
── VIDEO ──────────────────────────────
── AUDIO ──────────────────────────────
── AI ASSIST ──────────────────────────
```
- Section labels: 1px BORDER2 line with centered label in TEXT_MUTED 10px mono UPPERCASE
- [+] button at right of each section label to add an op from that category

Dry-run estimate footer (bottom of Zone C):
```
┌────────────────────────────────────────┐
│ EST. OUTPUT   12 files  ~4.2 GB        │
│ EST. TIME     ~8 min                   │
└────────────────────────────────────────┘
```
- BG background, BORDER top border
- Labels in TEXT_DIM 10px mono, values in TEXT 11px mono

---

## 7. Zone D — Processing Queue Timeline

Height: 140px. Collapsible. BG_PANEL background. Top border: 1px BORDER.

Collapse toggle (left of header):
```
▼ QUEUE  [QUEUED: 3] [RUNNING: 1] [DONE: 12] [FAILED: 0]   [RETRY ALL FAILED] [CLEAR DONE]
```
- Header row: 28px height, top border 1px BORDER
- Status badge chips: same chip style, color-coded by state token
- [RETRY ALL FAILED]: RED text, BG_WIDGET background — only shown when failed count > 0
- [CLEAR DONE]: TEXT_DIM text, BG_WIDGET background
- Clicking ▼ collapses Zone D to just the 28px header bar

Job rows (when expanded):
```
┌──────────────────────────────────────────────────────────────────────────────┐
│ ▶ IMAGE RESIZE × 12 files    [████████████░░░░] 67%   Step: resizing 8/12   │
│   /media/shoot_2026-04-29                        00:42 elapsed  [■] [↻]     │
└──────────────────────────────────────────────────────────────────────────────┘
```
- Row height: 44px (two text lines), BG_WIDGET background, BORDER bottom border
- Row hover → BG_HOVER
- Progress bar: BG_WIDGET track, GREEN fill for running, AMBER for queued, RED for failed
- Row left accent bar: 2px wide — GREEN (running), AMBER (queued), RED (failed), TEXT_DIM (done)
- Op type label: TEXT 11px mono UPPERCASE
- File count + path: TEXT_DIM 10px mono
- Elapsed time: TEXT_DIM 10px mono
- [■] cancel button, [↻] retry button — BG_WIDGET, BORDER border, 22×22px
- Click anywhere on row → opens job detail overlay (see below)

---

## 8. Job Detail Overlay

Triggered by clicking a queue row. Full-height right-side drawer, ~360px wide.
Pushed from the right over Zone C (Zone C dims but does not disappear).

```
JOB DETAIL                              [✕]
──────────────────────────────────────────
IMAGE RESIZE × 12 files
Started: 10:42:01   Elapsed: 00:42
──────────────────────────────────────────
ITEMS
 ✓  DSC_0001.dng   done     0.8s
 ✓  DSC_0002.dng   done     0.9s
 ▶  DSC_0003.dng   running  ...
 ○  DSC_0004.dng   queued
 ✕  DSC_0005.dng   failed   →  [see log]
──────────────────────────────────────────
LOG
  [10:42:33] starting item 3
  [10:42:33] ffmpeg -i DSC_0003.dng ...
  [10:42:34] ERR: codec not supported
──────────────────────────────────────────
[CANCEL JOB]                  [RETRY FAILED]
```
- Drawer background: BG_PANEL
- Left border: 1px BORDER
- Title: GREEN 13px mono UPPERCASE
- Section labels: same BORDER2 divider lines as Zone C
- Item status icons: ✓ GREEN, ▶ SYNCING cyan, ○ TEXT_DIM, ✕ RED
- Log area: BG (darkest), TEXT_DIM 10px mono, scrollable
- [CANCEL JOB]: RED text, BG_WIDGET background
- [RETRY FAILED]: AMBER text, BG_WIDGET background

---

## 9. Tool Health Modal

Triggered by clicking [TOOL HEALTH] tab or a tool warning chip.

Full-width content area replacement (not a popup). Layout:

```
TOOL HEALTH                                         [✕ CLOSE]
──────────────────────────────────────────────────────────────
TOOL           STATUS        VERSION      PATH
──────────────────────────────────────────────────────────────
ffmpeg         ● READY       6.1.0        /usr/bin/ffmpeg
ffprobe        ● READY       6.1.0        /usr/bin/ffprobe
gyroflow       ○ NOT FOUND   —            [SET PATH...]
whisper.cpp    ○ NOT FOUND   —            [SET PATH...]
──────────────────────────────────────────────────────────────
```
- Table rows: BG_WIDGET, BORDER bottom, hover → BG_HOVER
- Status dot: GREEN (ready), RED (error), TEXT_DIM (not found), AMBER (found but outdated)
- [SET PATH...] button: BG_WIDGET, BORDER border, TEXT 11px mono
- All text: monospace, column-aligned

---

## 10. What NOT to Design

- No sidebar navigation menu inside Media Care (left rail belongs to the main app)
- No rounded corners on anything
- No soft cards with drop shadows
- No light theme or mixed-theme panels
- No "Stewardship", "Parity", "Care Tier Distribution", "MTTD" concepts — those belong to a different product
- No duplicate screen title bar or redundant breadcrumbs
- No floating action buttons
- No modal popups that break out of the panel system (use inline drawers instead)
- No persistent status bars that duplicate Zone D info

---

## 11. Mapping to Existing Code Patterns

| Design Element         | Existing Code Reference                                    |
|------------------------|------------------------------------------------------------|
| Color tokens           | `crates/thegrid-gui/src/theme.rs` Colors struct            |
| File type chips        | `media_ingest.rs` MediaFileType chip render pattern        |
| Ingest grid + preview  | `media_ingest.rs` unchanged — Zone B wraps it              |
| Screen routing         | `app.rs` Screen::MediaIngest — extend, don't fork          |
| Tab strip pattern      | `dashboard.rs` DashTab enum + tab render pattern           |
| Event dispatch         | `app.rs` AppEvent — add MediaJob* variants                 |
| Worker pattern         | `runtime.rs` spawn_* pattern — add spawn_media_job_*       |
| DB queue tables        | `db.rs` index_queue pattern — replicate for media_jobs     |
