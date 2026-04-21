# TheGrid UI Design Requirements for Google Stitch

## Project Context
TheGrid is a desktop control panel for mesh devices (Tailscale nodes), built in Rust with egui.

Primary problem to solve:
- Current UI feels visually outdated and flat.
- Telemetry readability is inconsistent under dense data.
- CPU visualization style is not yet accepted.

## Design Goal
Create a bold, futuristic, high-legibility interface that feels like a next-generation transparent operations console, while keeping real operational data trustworthy and fast to scan.

## Core UX Principles
- Data first, style second.
- Every visual effect must improve readability or hierarchy.
- No decorative noise that looks like a stock chart.
- Avoid retro terminal look.
- Keep interaction friction minimal for operators.

### Hardware-Truth Visual Plan (Non-Optional)
This is a mandatory design track, not a decorative suggestion.

Objective:
- Make visual decoration informative and hardware-accurate.
- Users must be able to identify real hardware context at a glance.

Scope:
- RAM: distinguish module type and format (for example SO-DIMM DDR3 vs SO-DIMM DDR4/DDR5 when data is available).
- Storage: distinguish OS/system disk from data/archive disks when detectable.
- Storage media: represent HDD 3.5", SSD SATA, and M.2/NVMe with distinct visual blueprints/outlines, not generic repeated icons.
- Drive purpose cues: allow users to quickly infer where old photos/videos or archive data are likely stored.

Implementation rules:
- Use blueprint-like silhouettes, outlines, and labels tied to telemetry fields.
- If exact data is unavailable, show a clear fallback state (Unknown/Not detected) instead of faking precision.
- Keep aesthetics and usability balanced: visual richness must never hide key metrics.

Acceptance criteria:
- A user can distinguish RAM form factor/type and main storage classes in under 2 seconds.
- Disk visual identity must match detected device kind when data exists.
- No ambiguous icon reuse across different hardware classes.
- The design remains readable at compact telemetry density.

## Hard Layout Requirements
- Keep telemetry band independent from main body content.
- Do not fuse telemetry strip with the actions/content panel.
- Telemetry band must support adjustable height.
- Telemetry internal cards/panes must scale with height changes.
- Wide telemetry order must be:
  CPU | RAM+GPU | DISKS | NET | TASKS
- Identity panel on the left and Perf panel on the right are allowed, but center telemetry order must remain fixed.

## Main Screen Hierarchy
Top to bottom hierarchy:
- Telemetry band (compact but information-dense)
- Tab bar
- Main content body

Main content body in Actions tab:
- Action matrix (high priority operations)
- Control block (RDP options and immediate controls)
- Information block (Node info + watched paths)

## Visual Direction
Target style:
- Futuristic transparent HUD
- Layered depth and glass-like planes
- Isometric or pseudo-3D data surfaces where useful
- Controlled glow, not oversaturated bloom

Color direction:
- Low load: green
- Medium load: amber
- Critical load: red
- Avoid cyan-driven palette for core telemetry emphasis

Typography direction:
- Compact technical labels
- Strong value hierarchy
- Minimal clutter text

## CPU Visualization Requirements
Current accepted data model:
- CPU topology must show real detected values (for example 6C/12T on i7-10750H).
- Show CPU model string.
- Distinguish logical threads vs physical cores clearly.

Visualization requirements:
- Must not resemble stock market chart lines.
- Prefer isometric lane or isometric block-based rendering for depth.
- Threads and physical lanes may cross in perspective but must remain readable.
- Keep quick stats visible: average load, hotspot, current load context.

## Data Integrity Requirements
Telemetry must never feel misleading:
- CPU topology values must come from system detection, not hardcoded assumptions.
- Disk totals in headers must match per-drive rows.
- Network bars must remain visible at low throughput and include numeric labels.

## Interaction Requirements
- Telemetry width splits should be user-adjustable.
- Telemetry band height should be user-adjustable.
- Resizing should be stable between frames.
- Interaction affordances must be visible but subtle.

## Performance and Motion
- Motion should feel smooth and intentional.
- Use subtle temporal smoothing for fast-changing metrics.
- Avoid jitter and flicker in graph traces.
- Prefer low-cost effects compatible with egui desktop rendering.

## Anti-Patterns to Avoid
- Flat old-terminal appearance.
- Excessive neon that reduces legibility.
- Tiny labels over complex backgrounds.
- Visual motifs that look like finance dashboards.
- Decorative effects that hide real values.

## Design Deliverables Requested from Stitch
- High-fidelity layout proposal for desktop wide viewport.
- Telemetry band concept variants (2 to 3 options).
- CPU panel concepts focused on isometric depth.
- Component spec for card spacing, typography, and scaling behavior.
- Interaction spec for telemetry height/width resizing.

## Acceptance Criteria
A design proposal is accepted when:
- It clearly feels futuristic and modern.
- Operators can read key telemetry in under 2 seconds.
- CPU panel is visually distinctive and not stock-chart-like.
- Telemetry remains structurally separate from the main body.
- Layout hierarchy is obvious without relying on heavy color blocks.

## Technical Constraints for Implementation
- Runtime UI framework: egui (Rust desktop app).
- Effects must be implementable via 2D painter primitives and lightweight layering.
- Avoid solutions that require full 3D engine migration.
- Keep accessibility in mind with contrast and text size at compact scale.

## Prompt Snippet for Stitch Agent
Design a futuristic desktop operations dashboard for a mesh-device control app. Keep a separate adjustable telemetry strip at the top with center columns in this exact order: CPU, RAM+GPU, DISKS, NET, TASKS. Style must feel transparent HUD and 22nd-century, not retro terminal, not stock chart. CPU must use an isometric lane or block approach that clearly differentiates logical threads from physical cores with real topology labels. Use green amber red severity states, high readability, compact typography, subtle depth, and smooth motion cues. Then provide component specs and resizing interaction behavior for implementation in a Rust egui app.
