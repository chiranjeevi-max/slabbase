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
- The containment, overflow, alignment, and virtualization laws are verified at `820x840`, `1024x680`, `1440x900`, `2560x1440`, and `3440x1440` (21:9), with a `5120x1440` super-ultrawide spot check. Layout decisions use the effective width after app zoom, not the raw window width.
- On wide and ultra-wide displays, non-grid content keeps a readable measure: the schema `COLUMNS`/`RELATIONS` editor caps at 960px, form canvases at 920px, modals at 660px to 980px by kind, and empty-state copy at 460px. `DataGrid` uses the full available width so wide tables stay useful.
- At `prefers-contrast: more`, borders strengthen and secondary/tertiary text tokens meet at least 4.5:1 against their surfaces. Control borders meet at least 3:1.
- App zoom shortcuts (`⌘+`, `⌘-`, `⌘0`) scale the whole shell in 25% steps from 100% to 200%. The scaled shell still occupies exactly one viewport; media-style layout thresholds follow `window.innerWidth / zoom`.
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

### Text entry

- Inline grid editors and inspector fields set `dir="auto"` so mixed Arabic/Hebrew and Latin text isolate correctly.
- Enter during IME composition is owned by the input method and never commits or dismisses an editor. Commit resumes after `compositionend`.
- Date-only values round-trip as stored `YYYY-MM-DD` strings in every timezone; rendering never constructs a local `Date` for a calendar-only value.

### Selection and drafts

- The inspector shows only the record that is currently selected. A read that resolves after a table switch or a newer selection is discarded, and a pending row selection is cancelled by a table switch.
- Double-clicking another row while the inspector is open loads that row into the inspector.
- A New Record draft owns the inspector until it is saved or cancelled. Grid clicks and double-clicks do not change the selection while a draft is open.
- Unsaved Form Studio edits survive a schema refresh, including one caused by the user's own save.

### Scroll surfaces

- Scrollbars are thin and overlay-style. The thumb is a translucent neutral token.
- Every scroll pane sets `overscroll-behavior: contain`.
- The sticky grid header and sticky portal header have no shadow at `scrollTop === 0`. Once scrolled, they show a 1px bottom hairline. The surface itself has no permanent heavy border.
- The record history sheet is inline and non-modal inside the inspector. It uses the inspector's 16px padding, caps at 42% of the inspector height, and scrolls inside itself. It never traps focus or dismisses the record.
- The Index Advisor pill is a 20px capsule in the status bar with 11px status typography, one action, and one dismiss control. Long column names ellipsize; the pill never widens the status bar.

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
| `data-native` | `true` when the Tauri internals bridge is present |

Component CSS uses semantic variables only. Literal colors live in `apps/desktop/src/styles/tokens.css`.

| Family | Variables |
| --- | --- |
| Backgrounds | `--bg-backdrop`, `--bg-grid`, `--bg-sidebar`, `--bg-toolbar`, `--bg-sheet`, `--bg-elevated`, `--bg-popover`, `--bg-field`, `--bg-control`, `--bg-hover`, `--bg-search`, `--bg-segmented`, `--bg-paper` |
| Borders | `--border-hairline`, `--border-control`, `--border-grid`, `--border-elevated`, `--glass-edge` |
| Glass | `--glass-blur`, `--glass-sheen` |
| Text | `--text-primary`, `--text-secondary`, `--text-tertiary`, `--text-on-accent`, `--text-on-danger` |
| Accent | `--accent`, `--accent-hover`, `--accent-soft`, `--accent-ring`, `--accent-text`, `--accent-on-solid`, `--accent-foreground` |
| Geometry | `--toolbar-h`, `--subtoolbar-h`, `--statusbar-h`, `--sidebar-row-h`, `--grid-header-h`, `--row-h`, `--sidebar-w`, `--inspector-w` |
| Motion | `--ease`, `--motion-sheet-in`, `--motion-sheet-out`, `--motion-popover-in`, `--motion-popover-out`, `--motion-reduced` |

### Theme resolution

The default preference is `system`. It follows `prefers-color-scheme` and writes the resolved value to `data-theme`. Explicit `light` and `dark` ignore the media query.

### Surfaces

Solid is the default. Glass changes only chrome: sidebar, main toolbar, status bar, table sub-toolbar, popovers, `⌘K`, appearance popover, context menus, toasts, and overlay drawers. Data grids, paper, and form sheets stay opaque with alpha at or above 0.96.

On macOS, Glass is native window vibrancy, not just CSS alpha:

- `macOSPrivateApi: true` and `transparent: true` are required. The Tauri shell enables the `macos-private-api` feature.
- `set_window_surface("glass")` applies `NSVisualEffectView` with the UnderWindowBackground material and `FollowsWindowActiveState`, so the desktop wallpaper reads through the chrome tints in both themes.
- The native toolbar does not create its own CSS backdrop root, so floating panels can blur the content beneath them.
- `set_window_surface("solid")` clears window effects.
- The native window appearance follows the resolved light or dark theme so the material matches.
- Light Glass chrome tints: sidebar `rgba(245, 245, 247, 0.32)`, toolbar `rgba(251, 251, 253, 0.52)`.
- Dark Glass chrome tints: sidebar `rgba(28, 28, 31, 0.38)`, toolbar `rgba(32, 32, 36, 0.56)`.
- Native Glass popovers and drawers: `rgba(255, 255, 255, 0.64)` Light and `rgba(34, 34, 40, 0.62)` Dark.
- Glass chrome uses `backdrop-filter: blur(28px) saturate(200%)`, a 1px specular border, and an inner top highlight bevel.

Browser Glass has no OS vibrancy. It uses a frosted tint, a subtle sheen gradient, the same 28px blur, and crisp specular edges so Solid and Glass remain clearly distinct.

Under `prefers-reduced-transparency: reduce`, Glass reverts chrome tokens to Solid, removes the filter and sheen, and sends `set_window_surface("solid")` to clear native effects.

### Accent

Seven presets ship with light and dark values. A custom hex picker accepts `#RGB` and `#RRGGBB`. The hex field displays the resolved preset hex for the active theme, and keeps the custom hex value when the custom accent is active.

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
| Glass surfaces and native tokens | `glass keeps chrome translucent and grids opaque` |
| Accent hex follows the resolved theme | `accent hex follows the resolved theme` |
| Density virtualizer alignment | `density keeps virtual rows aligned` |
| Theme and preference persistence | `appearance controls persist` |
| Native traffic-light reserve | `native chrome reserves traffic lights` |
| Wide and ultrawide containment | `wide table and long values stay inside the grid viewport` |
| Rapid appearance changes | `rapid density, surface, theme, and inspector bursts keep scroll stable` |
| Record history sheet containment | `record time-travel drawer shows diffs and restores a revision` |
| Index Advisor pill containment | `index advisor pill creates an index and refreshes quietly` |
| Portal density alignment | `portal rows keep density alignment during rapid appearance changes` |
| Destructive error and recovery | `dropping a formula column reports an inline error and keeps state`, `relation-protected table drop, relation drop, and undo restore everything` |
| Overlay focus return | `every overlay returns focus to its opener after Escape` |
| Keyboard-only traversal | `keyboard-only navigation, malformed search, and undo routing` |
| IME and bidi safety | `IME composition Enter does not commit inline or inspector editors`, `mixed RTL and LTR text round-trips without clipping` |
| App zoom and scaling | `125% and 150% app zoom keep every chrome surface contained` |
| High contrast tokens | `high contrast preference strengthens borders and text contrast` |
| Ultrawide readable measure | `ultra-wide forms stay readable while grids use full width` |
| Air-gap boot | `air-gapped boot makes zero external http(s) requests` |
| HiDPI parity | `hidpi borders stay crisp and layout does not drift` |
| Date-only timezone invariance | `calendar dates never shift on display or edit` |
| WCAG 2.2 AA | `axe WCAG 2.2 AA scan finds no serious violations` |
| Stale inspector reads | `a stale inspector read cannot replace a newer table selection`, `a delayed row select is cancelled by a table switch` |
| Inspector follows double-click | `double-clicking another row refreshes an open inspector` |
| Draft isolation | `a grid double-click does not steal an open draft` |
| Form Studio edit retention | `unsaved item edits survive a schema refresh` |
