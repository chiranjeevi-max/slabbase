import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { isTauri } from "./native";
import { usePresence } from "./presence";
import { useStore } from "./store";
import { Icon } from "./components/Icons";
import { DataGrid } from "./components/DataGrid";
import { RecordPanel } from "./components/RecordPanel";
import { FormStudio } from "./components/FormStudio";
import { SchemaView } from "./components/SchemaView";
import { SchemaDialog } from "./components/SchemaDialog";
import { ImportDialog } from "./components/ImportDialog";
import { PrintDialog } from "./components/PrintDialog";
import { Palette } from "./components/Palette";
import { AuditDrawer } from "./components/AuditDrawer";
import { Toasts } from "./components/Toasts";
import { Toolbar } from "./components/Toolbar";
import { Sidebar } from "./components/Sidebar";
import { Statusbar } from "./components/Statusbar";
import { runMenuAction } from "./menu";

const SIDEBAR_DEFAULT = 240;
const SIDEBAR_MIN = 200;
const SIDEBAR_MAX = 300;
const INSPECTOR_DEFAULT = 360;
const INSPECTOR_MIN = 320;
const INSPECTOR_MAX = 480;
const WORKSPACE_MIN = 540;
const ZOOM_STEPS = [1, 1.25, 1.5, 1.75, 2] as const;

function readZoom(): number {
  const stored = Number(window.localStorage.getItem("slabbase.zoom.v1"));
  return ZOOM_STEPS.includes(stored as (typeof ZOOM_STEPS)[number]) ? stored : 1;
}

export function App() {
  const booting = useStore((state) => state.booting);
  const info = useStore((state) => state.info);
  const view = useStore((state) => state.view);
  const inspectorOpen = useStore((state) => state.inspectorOpen);
  const tables = useStore((state) => state.schema?.tables.length ?? 0);
  const native = isTauri();

  const [sidebarWidth, setSidebarWidth] = useState(() =>
    readWidth("slabbase.sidebarWidth", SIDEBAR_DEFAULT, SIDEBAR_MIN, SIDEBAR_MAX),
  );
  const [inspectorWidth, setInspectorWidth] = useState(() =>
    readWidth(
      "slabbase.inspectorWidth",
      INSPECTOR_DEFAULT,
      INSPECTOR_MIN,
      INSPECTOR_MAX,
    ),
  );
  const [sidebarDocked, setSidebarDocked] = useState(true);
  const [sidebarOverlay, setSidebarOverlay] = useState(false);
  const [viewportWidth, setViewportWidth] = useState(() => window.innerWidth);
  const [zoom, setZoom] = useState(readZoom);
  const narrowQuery = useMediaQuery("(max-width: 1079px)");
  const inspectorQuery = useMediaQuery("(max-width: 1199px)");
  const effectiveWidth = viewportWidth / zoom;
  const isNarrowSidebar = narrowQuery || effectiveWidth < 1080;
  const isInspectorOverlay = inspectorQuery || effectiveWidth < 1200;
  const inspectorPresence = usePresence(view === "table" && inspectorOpen, 110);
  const sidebarToggleRef = useRef<() => void>(() => {});
  const lastMenuUndoRef = useRef(0);

  useEffect(() => {
    if (isNarrowSidebar) setSidebarOverlay(false);
  }, [isNarrowSidebar]);

  useEffect(() => {
    const onResize = () => setViewportWidth(window.innerWidth);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  useWindowPreferences(sidebarWidth, zoom, inspectorWidth);

  useWorkspaceLifecycle();

  useWorkspaceShortcuts({
    isNarrowSidebar,
    sidebarOverlay,
    zoom,
    setZoom,
    setSidebarOverlay,
    sidebarToggleRef,
    lastMenuUndoRef,
  });

  const sidebarVisible = isNarrowSidebar ? sidebarOverlay : sidebarDocked;
  const showSidebar = Boolean(info) && (isNarrowSidebar || sidebarDocked);
  const reserveTrafficLights = native && (!info || isNarrowSidebar || !sidebarDocked);
  const inspectorMaxWidth = isInspectorOverlay
    ? INSPECTOR_MAX
    : Math.max(
        INSPECTOR_MIN,
        Math.min(
          INSPECTOR_MAX,
          viewportWidth - sidebarWidth - WORKSPACE_MIN - 10,
        ),
      );
  const effectiveInspectorWidth = Math.min(inspectorWidth, inspectorMaxWidth);

  const toggleSidebar = () => {
    if (isNarrowSidebar) setSidebarOverlay((open) => !open);
    else setSidebarDocked((open) => !open);
  };
  sidebarToggleRef.current = toggleSidebar;

  useNativeMenu(sidebarToggleRef, lastMenuUndoRef);

  return (
    <div className="app">
      {info && isNarrowSidebar && sidebarOverlay && (
        <div
          className="sidebar-scrim"
          role="presentation"
          onMouseDown={() => setSidebarOverlay(false)}
        />
      )}

      {showSidebar && (
        <Sidebar
          width={sidebarWidth}
          overlay={isNarrowSidebar}
          open={sidebarVisible}
          onToggle={toggleSidebar}
        />
      )}

      {info && !isNarrowSidebar && sidebarDocked && (
        <PaneResizer
          side="left"
          width={sidebarWidth}
          min={SIDEBAR_MIN}
          max={SIDEBAR_MAX}
          onResize={setSidebarWidth}
        />
      )}

      <div
        className="main-column"
        data-sidebar-docked={!isNarrowSidebar && sidebarDocked ? "true" : "false"}
      >
        <Toolbar
          reserveTrafficLights={reserveTrafficLights}
          showSidebarToggle={Boolean(info) && (isNarrowSidebar || !sidebarDocked)}
          onToggleSidebar={toggleSidebar}
        />

        {booting ? (
          <div className="splash">
            <Icon name="database" size={24} />
            <p>Opening workspace…</p>
          </div>
        ) : info ? (
          <>
            <div className="app-body">
              <main className={`workspace ${view === "form" ? "studio-view" : ""}`}>
                {tables === 0 ? (
                  <EmptyWorkspace />
                ) : view === "schema" ? (
                  <SchemaView />
                ) : view === "form" ? (
                  <FormStudio />
                ) : (
                  <DataGrid />
                )}
              </main>
              {view === "table" && inspectorPresence.mounted && (
                <>
                  {inspectorOpen && !isInspectorOverlay && (
                    <PaneResizer
                      side="right"
                      width={effectiveInspectorWidth}
                      min={INSPECTOR_MIN}
                      max={inspectorMaxWidth}
                      onResize={setInspectorWidth}
                    />
                  )}
                  <RecordPanel
                    width={effectiveInspectorWidth}
                    closing={inspectorPresence.closing}
                  />
                </>
              )}
            </div>
            <Statusbar />
          </>
        ) : (
          <Launcher />
        )}
      </div>

      <SchemaDialog />
      <ImportDialog />
      <PrintDialog />
      <Palette />
      <AuditDrawer />
      <Toasts />
    </div>
  );
}

function useWindowPreferences(sidebarWidth: number, zoom: number, inspectorWidth: number) {
  useEffect(() => {
    window.localStorage.setItem("slabbase.sidebarWidth", String(sidebarWidth));
  }, [sidebarWidth]);

  useEffect(() => {
    document.documentElement.style.setProperty("--app-zoom", String(zoom));
    document.documentElement.dataset["zoom"] = String(Math.round(zoom * 100));
    document.documentElement.dataset["zoomTier"] = zoom >= 1.75 ? "tight" : zoom > 1 ? "compact" : "normal";
    window.localStorage.setItem("slabbase.zoom.v1", String(zoom));
  }, [zoom]);

  useEffect(() => {
    window.localStorage.setItem("slabbase.inspectorWidth", String(inspectorWidth));
  }, [inspectorWidth]);
}

function useWorkspaceLifecycle() {
  useEffect(() => {
    const root = document.documentElement;
    const apply = (focused: boolean) => {
      root.dataset["windowFocused"] = focused ? "true" : "false";
    };
    apply(true);
    const onFocus = () => apply(true);
    const onBlur = () => apply(false);
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
    };
  }, []);

  useEffect(() => {
    void useStore.getState().initialize();
  }, []);
}

function useNativeMenu(
  sidebarToggleRef: React.RefObject<() => void>,
  lastMenuUndoRef: React.RefObject<number>,
) {
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<string>("menu-action", (event) => {
      if (event.payload === "edit-undo" || event.payload === "edit-redo") {
        lastMenuUndoRef.current = performance.now();
      }
      void runMenuAction(event.payload, () => sidebarToggleRef.current());
    }).then((off) => {
      if (disposed) off();
      else unlisten = off;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [lastMenuUndoRef, sidebarToggleRef]);
}

function useWorkspaceShortcuts({
  isNarrowSidebar,
  sidebarOverlay,
  zoom,
  setZoom,
  setSidebarOverlay,
  sidebarToggleRef,
  lastMenuUndoRef,
}: {
  isNarrowSidebar: boolean;
  sidebarOverlay: boolean;
  zoom: number;
  setZoom: (zoom: number) => void;
  setSidebarOverlay: (open: boolean) => void;
  sidebarToggleRef: React.RefObject<() => void>;
  lastMenuUndoRef: React.RefObject<number>;
}) {
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      const state = useStore.getState();
      const meta = event.metaKey || event.ctrlKey;
      const alt = event.altKey;
      const key = event.key.toLowerCase();
      const editing = isTextEntry(event.target);

      if (meta && alt && event.code === "KeyS") {
        event.preventDefault();
        sidebarToggleRef.current();
      } else if (meta && alt && event.code === "KeyI") {
        if (!state.info || state.view === "schema") return;
        event.preventDefault();
        state.toggleInspector();
      } else if (meta && ["1", "2", "3", "4"].includes(key)) {
        if (!state.info) return;
        event.preventDefault();
        if (key === "1") state.setView("table");
        else if (key === "2") state.setView("form");
        else if (key === "3") state.setView("schema");
        else void state.openAudit();
      } else if (meta && key === "k") {
        if (!state.info) return;
        event.preventDefault();
        state.setPaletteOpen(true);
      } else if (meta && key === "z") {
        if (editing || !state.info) return;
        if (performance.now() - lastMenuUndoRef.current < 250) return;
        event.preventDefault();
        if (event.shiftKey) void state.redo();
        else void state.undo();
      } else if (meta && (key === "=" || key === "+")) {
        event.preventDefault();
        const index = ZOOM_STEPS.indexOf(zoom as (typeof ZOOM_STEPS)[number]);
        const next = ZOOM_STEPS[Math.min(ZOOM_STEPS.length - 1, index + 1)] ?? 1;
        setZoom(next);
      } else if (meta && key === "-") {
        event.preventDefault();
        const index = ZOOM_STEPS.indexOf(zoom as (typeof ZOOM_STEPS)[number]);
        const next = index <= 0 ? 1 : (ZOOM_STEPS[index - 1] ?? 1);
        setZoom(next);
      } else if (meta && key === "0") {
        event.preventDefault();
        setZoom(1);
      } else if (meta && key === ",") {
        event.preventDefault();
        state.setAppearanceOpen(true);
      } else if (meta && key === "n") {
        if (editing || !state.info) return;
        event.preventDefault();
        state.beginCreate();
      } else if (meta && key === "p") {
        if (!state.info) return;
        event.preventDefault();
        state.setPrintOpen(true);
      } else if (meta && event.shiftKey && key === "i") {
        if (!state.info) return;
        event.preventDefault();
        state.setImportOpen(true);
      } else if (event.key === "Escape") {
        if (state.paletteOpen) state.setPaletteOpen(false);
        else if (state.schemaOpen) state.setSchemaOpen(false);
        else if (state.importOpen) state.setImportOpen(false);
        else if (state.printOpen) state.setPrintOpen(false);
        else if (state.auditOpen) state.closeAudit();
        else if (state.appearanceOpen) state.setAppearanceOpen(false);
        else if (isNarrowSidebar && sidebarOverlay) {
          document.querySelector<HTMLElement>('[data-testid="sidebar-toggle"]')?.focus();
          setSidebarOverlay(false);
        }
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [isNarrowSidebar, sidebarOverlay, zoom, setZoom, setSidebarOverlay, sidebarToggleRef, lastMenuUndoRef]);
}

function EmptyWorkspace() {
  const openSchema = useStore((state) => state.openSchema);
  const loadSample = useStore((state) => state.loadSample);
  const busy = useStore((state) => state.busy);

  return (
    <div className="canvas-empty" data-testid="empty-workspace">
      <span className="empty-glyph">
        <Icon name="table" size={32} strokeWidth={1.2} />
      </span>
      <h2>No tables yet</h2>
      <p>Create a table, or load the sample studio.</p>
      <div className="empty-actions">
        <button type="button" className="button primary" onClick={() => openSchema("table")}>
          New Table
        </button>
        <button
          type="button"
          className="button"
          disabled={busy}
          onClick={() => void loadSample()}
        >
          Load Sample Workspace
        </button>
      </div>
    </div>
  );
}

function Launcher() {
  const busy = useStore((state) => state.busy);
  const openWorkspace = useStore((state) => state.openWorkspace);
  const createWorkspace = useStore((state) => state.createWorkspace);
  const [recent, setRecent] = useState<string | null>(() =>
    window.localStorage.getItem("slabbase.recent"),
  );

  const openExisting = async () => {
    const path = await openDialog({
      multiple: false,
      filters: [{ name: "SlabBase workspace", extensions: ["slab"] }],
    });
    if (typeof path !== "string") return;
    window.localStorage.setItem("slabbase.recent", path);
    setRecent(path);
    await openWorkspace(path);
  };

  const newWorkspace = async () => {
    const path = await saveDialog({
      defaultPath: "Untitled.slab",
      filters: [{ name: "SlabBase workspace", extensions: ["slab"] }],
    });
    if (!path) return;
    const name = basename(path).replace(/\.slab$/i, "") || "Untitled";
    window.localStorage.setItem("slabbase.recent", path);
    setRecent(path);
    await createWorkspace(path, name);
  };

  const newSample = async () => {
    const path = await saveDialog({
      defaultPath: "Design Studio.slab",
      filters: [{ name: "SlabBase workspace", extensions: ["slab"] }],
    });
    if (!path) return;
    window.localStorage.setItem("slabbase.recent", path);
    setRecent(path);
    await createWorkspace(path, "Design Studio");
    await useStore.getState().loadSample();
  };

  return (
    <main className="launcher">
      <div className="launcher-content">
        <span className="launcher-glyph">
          <Icon name="database" size={28} strokeWidth={1.2} />
        </span>
        <span className="launcher-kicker">Local SQLite workspace</span>
        <h1>SlabBase</h1>
        <p>Relational records, visual forms, and reports, all stored in a single database file.</p>
        <div className="launcher-actions">
          <button type="button" className="button primary" onClick={() => void openExisting()}>
            Open Workspace
          </button>
          <button type="button" className="button" onClick={() => void newWorkspace()}>
            New Workspace
          </button>
          <button
            type="button"
            className="button"
            disabled={busy}
            onClick={() => void newSample()}
          >
            Load Sample Workspace
          </button>
        </div>
        {recent && (
          <button
            type="button"
            className="recent-button"
            onClick={() => void openWorkspace(recent)}
            title={recent}
          >
            <Icon name="history" size={13} strokeWidth={1.5} />
            <span>{basename(recent)}</span>
            <span className="recent-folder">{dirname(recent)}</span>
          </button>
        )}
        <p className="launcher-footnote">Apache-2.0 · no account · no cloud</p>
      </div>
    </main>
  );
}

function PaneResizer({
  side,
  width,
  min,
  max,
  onResize,
}: {
  side: "left" | "right";
  width: number;
  min: number;
  max: number;
  onResize: (width: number) => void;
}) {
  const [active, setActive] = useState(false);
  const origin = useRef({ x: 0, width });

  useEffect(() => {
    if (!active) return;
    const move = (event: PointerEvent) => {
      const delta = event.clientX - origin.current.x;
      const next = side === "left" ? origin.current.width + delta : origin.current.width - delta;
      onResize(Math.min(max, Math.max(min, next)));
    };
    const up = () => setActive(false);
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up, { once: true });
    document.body.classList.add("resizing");
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      document.body.classList.remove("resizing");
    };
  }, [active, side, min, max, onResize]);

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={side === "left" ? "Resize sidebar" : "Resize inspector"}
      className={`pane-resizer ${side} ${active ? "active" : ""}`}
      onPointerDown={(event) => {
        event.preventDefault();
        origin.current = { x: event.clientX, width };
        setActive(true);
      }}
    />
  );
}

function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(() =>
    typeof window === "undefined" ? false : window.matchMedia(query).matches,
  );

  useEffect(() => {
    const media = window.matchMedia(query);
    const update = () => setMatches(media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, [query]);

  return matches;
}

function isTextEntry(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || target.isContentEditable;
}

function readWidth(key: string, fallback: number, min: number, max: number): number {
  const raw = Number(window.localStorage.getItem(key));
  if (!Number.isFinite(raw) || raw <= 0) return fallback;
  return Math.min(max, Math.max(min, raw));
}

function basename(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] ?? path;
}

function dirname(path: string): string {
  const parts = path.split(/[\\/]/);
  parts.pop();
  return parts.join("/") || "/";
}
