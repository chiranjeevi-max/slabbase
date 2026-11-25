# SlabBase Design Contract

Status: normative. This document defines the desktop visual and interaction baseline. Any change to these values updates this file and `apps/desktop/tests/ui-audit.spec.ts` in the same change.

Scope: `apps/desktop` in browser and Tauri macOS modes. Rust domain crates (`crates/*`) and workspace storage formats are outside this contract.

## 1. Spatial rhythm and breathing room

The layout uses a 4px base unit and an 8px grouping rhythm.

| Surface | Height | Outer padding | Gap | Control height |
| --- | ---: | ---: | ---: | ---: |
| Main toolbar | 48px | 16px horizontal | 8px inside groups, 16px between groups | 28px |
| Table sub-toolbar | 44px | 16px horizontal | 8px to 10px | 28px |
| Status bar | 26px | 16px horizontal | 12px between items | none |
| Sidebar rows | 30px | 12px sidebar horizontal, 10px row horizontal | 8px icon to label | 30px |
| Inspector | full body height | 16px | 16px section, 12px field | 28px |

Toolbar geometry:

- 48px total height leaves 10px above and below 28px controls.
- The workspace switcher starts 16px from the main pane's left edge.
- The sidebar header is 48px tall and holds the macOS traffic-light reserve on the left plus the sidebar toggle on the right.
- Sidebar section groups (`TABLES`, `FORMS`, `WORKSPACE`) have 20px of vertical separation.
- The sidebar `+` button's visible glyph right edge and the row count right edge share one 10px inset axis.
- Numeric and currency column headers right-align to the same 9px inset as their cell values. The `ID` header and `ID` cell use the same 9px inset.
- The sidebar is 240px by default and resizes from 200px to 300px. The record inspector is 360px by default and resizes from 320px to 480px.
- When both panes are docked, the inspector's effective maximum shrinks first so the workspace keeps at least 540px. This keeps the sub-toolbar intact while preserving the sidebar's 200px to 300px range.

## 2. Viewport and containment laws

- The root shell is exactly `100vw × 100vh`, with `max-width: 100vw`, `max-height: 100vh`, `overflow: hidden`.
- `box-sizing: border-box` applies to every element.
- Every flex or grid parent that holds dynamic content sets `min-width: 0` and `min-height: 0`.
- No chrome container may produce an unintended horizontal scrollbar. This covers the toolbar, sidebar, table sub-toolbar, inspector, status bar, command palette, context menu, and appearance popover.
- Data grids may scroll inside their own viewport when columns exceed the pane. Portal grids may scroll inside their own bordered viewport when columns exceed the inspector. The surrounding chrome must not scroll or clip content outside its bounds.
- Below 1080px viewport width the sidebar becomes a fixed overlay drawer with a scrim, outside-click dismissal, Escape dismissal, and focus return. The main pane keeps its full width.
- Below 1200px viewport width the record inspector becomes a fixed elevated sheet above the grid. The docked inspector appears at 1200px and wider. The grid pane keeps at least 480px of width in both modes.
- The sidebar hairline runs from `y = 0` to the window bottom. The status bar belongs to the main pane and never cuts through the sidebar.

## 3. Native macOS interaction checklist

### Keyboard focus

- Pointer focus does not draw a ring: `:focus:not(:focus-visible) { outline: none; }`.
- Keyboard focus draws a crisp 2px accent ring on `:focus-visible`.
- Active grid and portal cells use a 1.5px inset accent border and do not shift layout.
- Modals, sheets, the audit drawer, and `⌘K` trap Tab. Dismissal restores focus to the element that opened the surface.

### Inactive window

- The app listens to window `focus` and `blur` and writes `data-window-focused="true|false"` on `<html>`.
- When the window is unfocused, active sidebar and grid row tints become neutral macOS gray, `rgba(128, 128, 128, 0.16)`.
- When the window regains focus, the accent tint returns.

### Context menus

- macOS native `NSMenu` is the preferred platform path. This build ships the DOM fallback because the Tauri scope is limited to window chrome and traffic lights.
- The fallback clamps at least 8px from every viewport edge.
- Minimum width is 180px. Row height is 24px. Corner radius is 7px.
- Escape, outside pointer down, and pane scroll dismiss the menu.
- A 1px separator precedes destructive actions.

### Motion

- Sheets enter in 160ms with `cubic-bezier(0.2, 0, 0, 1)`, moving from `opacity: 0; translateY(-6px) scale(0.985)` to identity. Dismissal completes within 110ms.
- `⌘K` and popovers enter in 120ms. Dismissal completes within 110ms.
- Animate `transform` and `opacity` only.
- Under `prefers-reduced-motion: reduce`, transitions collapse to 40ms or less and animate opacity only.

### Scroll surfaces

- Scrollbars are thin and overlay-style. The thumb is a translucent neutral token.
- Every scroll pane sets `overscroll-behavior: contain`.
- The sticky grid header and sticky portal header have no shadow at `scrollTop === 0`. Once scrolled, they show a 1px bottom hairline. The surface itself has no permanent heavy border.

### Dark mode surfaces

The dark theme steps luminance in this order:

| Surface | Value |
| --- | --- |
| Backdrop | `#0E0E10` |
| Grid and content | `#141416` |
| Sidebar | `#1C1C1F` |
| Toolbar | `#202024` |
| Elevated popover or sheet | `#26262B` |

- Pane separators use `rgba(255, 255, 255, 0.09)`.
- Floating dark surfaces use a 1px outer border of `rgba(255, 255, 255, 0.12)` and a top specular inset of `rgba(255, 255, 255, 0.07)`.

## 4. Theme and customization contract

The `<html>` element carries these attributes:

| Attribute | Values |
| --- | --- |
| `data-theme` | `light`, `dark` (resolved from `system`, `light`, or `dark`) |
| `data-surface` | `solid`, `glass` |
| `data-accent` | `blue`, `graphite`, `indigo`, `teal`, `green`, `orange`, `rose`, `custom` |
| `data-density` | `compact`, `default`, `comfortable` |
| `data-window-focused` | `true`, `false` |
| `data-native` | `true` when `window.__TAURI_INTERNALS__` exists |

Component CSS uses semantic variables only. Literal colors live in `apps/desktop/src/styles/tokens.css`.

| Family | Variables |
| --- | --- |
| Backgrounds | `--bg-backdrop`, `--bg-grid`, `--bg-sidebar`, `--bg-toolbar`, `--bg-sheet`, `--bg-elevated`, `--bg-popover`, `--bg-field`, `--bg-hover`, `--bg-search`, `--bg-segmented`, `--bg-paper` |
| Borders | `--border-hairline`, `--border-control`, `--border-grid`, `--border-elevated` |
| Text | `--text-primary`, `--text-secondary`, `--text-tertiary`, `--text-on-accent`, `--text-on-danger` |
| Accent | `--accent`, `--accent-hover`, `--accent-soft`, `--accent-ring`, `--accent-text`, `--accent-on-solid`, `--accent-foreground` |
| Geometry | `--toolbar-h`, `--subtoolbar-h`, `--statusbar-h`, `--sidebar-row-h`, `--grid-header-h`, `--row-h`, `--sidebar-w`, `--inspector-w` |
| Motion | `--ease`, `--motion-sheet-in`, `--motion-sheet-out`, `--motion-popover-in`, `--motion-popover-out`, `--motion-reduced` |

### Theme resolution

The default preference is `system`. It follows `prefers-color-scheme` and writes the resolved value to `data-theme`. Explicit `light` and `dark` ignore the media query.

### Surfaces

Solid is the default. Glass applies `backdrop-filter: blur(22px) saturate(180%)` only to chrome: sidebar, toolbar, popovers, `⌘K`, appearance popover, and context menus. Data grids and form sheets stay opaque with alpha at or above 0.96. Under `prefers-reduced-transparency: reduce`, Glass falls back to Solid and removes the filter.

### Accent

Seven presets ship with light and dark values. A custom hex picker accepts `#RGB` and `#RRGGBB`.

Derived values:

- `--accent-hover`: `color-mix()` of the accent with black in light mode and white in dark mode.
- `--accent-soft`: translucent accent tint for selection.
- `--accent-ring`: translucent accent ring for focus shadows.
- `--accent-text`: accent mixed with the theme text color for readable selection text.
- `--accent-on-solid`: the more legible of black and white, chosen by relative luminance contrast against `--accent`.
- `--accent-foreground`: alias of `--accent-on-solid` for foreground use on accent fills.

### Density

| Mode | Row height |
| --- | ---: |
| Compact | 26px |
| Default | 29px |
| Comfortable | 34px |

Both table and portal virtualizers derive their row math from the active density value. CSS rows use `--row-h`.

### Persistence

Appearance preferences persist in `localStorage` under `slabbase.appearance.v1` with `version: 1`. The legacy `slabbase.theme` key migrates into the new schema once.

## 5. Enforcement map

| Rule | Playwright check |
| --- | --- |
| Shell fits viewport | `shell has no viewport overflow` |
| Chrome containment and scroll width | `chrome containers stay contained` |
| Clipped text and sibling overlap | `chrome text and spacing are clean` |
| Toolbar, sidebar, inspector, status rhythm | `shell rhythm matches the contract` |
| Workspace switcher position | `workspace switcher clears the sidebar divider` |
| Sidebar glyph and count axis | `sidebar plus glyphs share the count axis` |
| Numeric header and cell axis | `numeric headers share the cell axis` |
| Focus rings and trap | `focus rings follow keyboard intent` |
| Inactive window tint | `window blur neutralizes active tints` |
| Context menu clamping | `context menu clamps and dismisses` |
| Reduced motion | `reduced motion collapses surfaces` |
| Dark luminance and bevels | `dark surfaces step and bevel` |
| Glass surfaces | `glass keeps chrome translucent and grids opaque` |
| Density virtualizer alignment | `density keeps virtual rows aligned` |
| Theme and preference persistence | `appearance controls persist` |
| Native traffic-light reserve | `native chrome reserves traffic lights` |
