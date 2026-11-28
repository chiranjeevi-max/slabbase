import { useEffect, useMemo, useRef, useState } from "react";
import type { Layout, LayoutItem, Relation, Table } from "../bindings";
import { useStore } from "../store";
import { newItemId } from "../types";
import { Icon } from "./Icons";
import { ItemInspector, ROW_HEIGHT, StudioItem } from "./StudioItem";
import { nextRow, StudioPalette } from "./StudioPalette";

type GridRect = { x: number; y: number; w: number; h: number };

export function FormStudio() {
  const state = useStudioState();
  const { table, layout, setLayout } = state;

  if (!table) {
    return <div className="canvas-empty">Select a table to design a form.</div>;
  }

  if (!layout) {
    return (
      <div className="canvas-empty">
        <span className="empty-glyph">
          <Icon name="form" size={32} strokeWidth={1.2} />
        </span>
        <h2>No form for {table.name}</h2>
        <p>Start with every field placed, then drag items on the grid.</p>
        <div className="empty-actions">
          <button
            type="button"
            className="button primary"
            onClick={() => setLayout(defaultLayout(table))}
          >
            Create Form
          </button>
        </div>
      </div>
    );
  }

  return <StudioEditor state={state} table={table} layout={layout} />;
}

function StudioEditor({
  state,
  table,
  layout,
}: {
  state: ReturnType<typeof useStudioState>;
  table: Table;
  layout: Layout;
}) {
  const {
    schema, activeTable, inspectorOpen, saveLayout, setLayout, selectedId, setSelectedId,
    setDragging, saving, setSaving, loadedLayout, dirtyRef, relations, portalRelation, setPortalRelation,
  } = state;
  const selected = layout.items.find((item) => item.id === selectedId) ?? null;
  const update = (next: Layout) => {
    dirtyRef.current = true;
    setLayout({ ...next, items: [...next.items] });
  };

  const updateItem = (id: string, patch: Partial<LayoutItem>) => {
    update({
      ...layout,
      items: layout.items.map((item) => (item.id === id ? { ...item, ...patch } : item)),
    });
  };

  const save = async () => {
    const snapshot = { ...layout, updated_at: Date.now() };
    dirtyRef.current = false;
    setSaving(true);
    const saved = await saveLayout(snapshot);
    setSaving(false);
    if (saved) {
      // Edits made while the save was in flight set dirtyRef again; the effect
      // will preserve them instead of replacing the form with this snapshot.
      loadedLayout.current = {
        key: `${activeTable ?? ""}|${snapshot.id}`,
        updatedAt: snapshot.updated_at,
      };
    } else {
      dirtyRef.current = true;
    }
  };

  return (
    <div className="studio">
      <div className="studio-toolbar">
        <div className="studio-title">
          <span className="studio-caption">{table.name}</span>
          <input
            className="layout-name"
            value={layout.name}
            aria-label="Form name"
            onChange={(event) => update({ ...layout, name: event.target.value })}
          />
        </div>
        <div className="studio-actions">
          <select
            className="select toolbar-select"
            value={layout.id}
            aria-label="Form layout"
            onChange={(event) => useStore.getState().selectLayout(event.target.value)}
          >
            {schema?.layouts
              .filter((candidate) => candidate.table === table.name)
              .map((candidate) => (
                <option key={candidate.id} value={candidate.id}>
                  {candidate.name}
                </option>
              ))}
            {layout.id === "" && <option value="">Unsaved Form</option>}
          </select>
          <button
            type="button"
            className="button compact"
            onClick={() => setLayout(defaultLayout(table))}
          >
            New Form
          </button>
          <button
            type="button"
            className="button primary compact"
            onClick={() => void save()}
            disabled={saving}
          >
            {saving ? "Saving…" : "Save Form"}
          </button>
        </div>
      </div>

      <div className="studio-body">
        <StudioPalette
          table={table}
          layout={layout}
          update={update}
          setSelectedId={setSelectedId}
          setDragging={setDragging}
          addKind={addKind}
          portalRelation={portalRelation}
          setPortalRelation={setPortalRelation}
          relations={relations}
        />

        <StudioCanvas
          state={state}
          layout={layout}
          table={table}
          update={update}
          updateItem={updateItem}
        />

        {inspectorOpen && (
          <aside className="studio-inspector" aria-label="Form item inspector">
            <header className="inspector-header">
              <span>Inspector</span>
            </header>
            {selected ? (
              <ItemInspector
                item={selected}
                table={table}
                relations={relations}
                onChange={(patch) => updateItem(selected.id, patch)}
                onRemove={() => {
                  update({
                    ...layout,
                    items: layout.items.filter((item) => item.id !== selected.id),
                  });
                  setSelectedId(null);
                }}
              />
            ) : (
              <div className="inspector-body">
                <span className="palette-heading">Form</span>
                <div className="inspector-readout">
                  <span>Layout</span>
                  <span>{layout.name || "Unsaved"}</span>
                </div>
                <div className="inspector-readout">
                  <span>Grid</span>
                  <span>{layout.grid.columns} columns</span>
                </div>
                <div className="inspector-readout">
                  <span>Row Height</span>
                  <span>{ROW_HEIGHT} px</span>
                </div>
                <div className="inspector-readout">
                  <span>Items</span>
                  <span>{layout.items.length}</span>
                </div>
                <p className="inspector-hint">Select an item to adjust its grid position.</p>
              </div>
            )}
          </aside>
        )}
      </div>
    </div>
  );

  function addKind(kind: string) {
    const base = layout;
    if (!base) return;
    const item = newKindItem(
      kind,
      { x: 0, y: nextRow(base) },
      relations,
      portalRelation,
    );
    if (!item) return;
    update({ ...base, items: [...base.items, item] });
    setSelectedId(item.id);
  }
}

function useStudioState() {
  const schema = useStore((state) => state.schema);
  const activeTable = useStore((state) => state.activeTable);
  const activeLayoutId = useStore((state) => state.activeLayoutId);
  const inspectorOpen = useStore((state) => state.inspectorOpen);
  const saveLayout = useStore((state) => state.saveLayout);
  const setFormStatus = useStore((state) => state.setFormStatus);
  const [layout, setLayout] = useState<Layout | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [snap, setSnap] = useState<GridRect | null>(null);
  const [dragging, setDragging] = useState(false);
  const [saving, setSaving] = useState(false);
  const [guide, setGuide] = useState<GridRect | null>(
    null,
  );
  const canvasRef = useRef<HTMLDivElement>(null);
  const layoutRef = useRef<Layout | null>(null);
  const loadedLayout = useRef<{ key: string; updatedAt: number } | null>(null);
  const dirtyRef = useRef(false);
  layoutRef.current = layout;

  const table = schema?.tables.find((candidate) => candidate.name === activeTable);
  const relations = useMemo(
    () => schema?.relations.filter((relation) => relation.parent_table === activeTable) ?? [],
    [schema, activeTable],
  );
  const [portalRelation, setPortalRelation] = useState<string>("");

  useEffect(() => {
    const existing =
      schema?.layouts.find(
        (candidate) => candidate.id === activeLayoutId && candidate.table === activeTable,
      ) ?? schema?.layouts.find((candidate) => candidate.table === activeTable);
    const key = `${activeTable ?? ""}|${activeLayoutId ?? ""}`;
    const updatedAt = existing?.updated_at ?? -1;
    const loaded = loadedLayout.current;
    if (
      loaded &&
      loaded.key === key &&
      layoutRef.current !== null &&
      (dirtyRef.current || loaded.updatedAt === updatedAt)
    ) {
      // Keep local form edits and selection when a schema refresh leaves this
      // layout unchanged. Only the portal picker needs the current relations.
      const names = relations.map((relation) => relation.name);
      setPortalRelation((current) =>
        names.includes(current) ? current : (names[0] ?? ""),
      );
      return;
    }
    setLayout(existing ?? null);
    setSelectedId(null);
    setPortalRelation(relations[0]?.name ?? "");
    loadedLayout.current = { key, updatedAt };
    dirtyRef.current = false;
  }, [schema, activeTable, activeLayoutId, relations]);

  useEffect(() => {
    if (!layout) {
      setFormStatus(null);
      return;
    }
    setFormStatus({ name: layout.name, items: layout.items.length });
    return () => setFormStatus(null);
  }, [layout, setFormStatus]);

  return {
    schema, activeTable, inspectorOpen, saveLayout, layout, setLayout, selectedId, setSelectedId,
    snap, setSnap, dragging, setDragging, saving, setSaving, guide, setGuide, canvasRef,
    loadedLayout, dirtyRef, table, relations, portalRelation, setPortalRelation,
  };
}

function StudioCanvas({
  state,
  layout,
  table,
  update,
  updateItem,
}: {
  state: ReturnType<typeof useStudioState>;
  layout: Layout;
  table: Table;
  update: (layout: Layout) => void;
  updateItem: (id: string, patch: Partial<LayoutItem>) => void;
}) {
  const {
    canvasRef, dragging, setDragging, snap, setSnap, guide, setGuide,
    selectedId, setSelectedId, schema, relations, portalRelation,
  } = state;
  const canvasHeight = Math.max(
    520,
    Math.max(0, ...layout.items.map((item) => item.y + item.h)) * ROW_HEIGHT + 80,
  );

  const dropPosition = (event: React.DragEvent) => {
    const canvas = canvasRef.current;
    if (!canvas) return null;
    const rect = canvas.getBoundingClientRect();
    if (!(rect.width > 0)) return null;
    const columns = 12;
    const rawX = ((event.clientX - rect.left) / rect.width) * columns;
    const rawY = Math.max(0, Math.floor((event.clientY - rect.top) / ROW_HEIGHT));
    if (!Number.isFinite(rawX) || !Number.isFinite(rawY)) return null;
    return { x: Math.max(0, Math.min(columns - 1, Math.round(rawX))), y: rawY };
  };

  const handleDrop = (event: React.DragEvent) => {
    event.preventDefault();
    setSnap(null);
    setDragging(false);
    const position = dropPosition(event);
    if (!position) return;
    const payload = event.dataTransfer.getData("text/plain");
    if (payload.startsWith("slab-column:")) {
      const name = payload.slice("slab-column:".length);
      const column = table.columns.find((candidate) => candidate.name === name);
      if (!column) return;
      const width = 6;
      const x = Math.min(position.x, 12 - width);
      const item: LayoutItem = {
        id: newItemId("field"),
        kind: { kind: "field", column: name },
        x,
        y: position.y,
        w: width,
        h: 2,
      };
      update({ ...layout, items: [...layout.items, item] });
      setSelectedId(item.id);
    } else if (payload.startsWith("slab-move:")) {
      const id = payload.slice("slab-move:".length);
      const item = layout.items.find((candidate) => candidate.id === id);
      if (!item) return;
      const x = Math.min(position.x, 12 - item.w);
      updateItem(id, { x: Math.max(0, x), y: position.y });
    } else if (payload.startsWith("slab-kind:")) {
      const kind = payload.slice("slab-kind:".length);
      const item = newKindItem(kind, position, relations, portalRelation);
      if (!item) return;
      update({ ...layout, items: [...layout.items, item] });
      setSelectedId(item.id);
    }
  };

  return (
    <div className="canvas-wrap">
      <div
        ref={canvasRef}
        className={`canvas ${dragging ? "dragging" : ""}`}
        style={{ height: canvasHeight }}
        onDragOver={(event) => {
          event.preventDefault();
          const position = dropPosition(event);
          if (position) setSnap({ x: position.x, y: position.y, w: 6, h: 2 });
        }}
        onDragLeave={() => setSnap(null)}
        onDrop={handleDrop}
        onClick={() => setSelectedId(null)}
      >
        {snap && (
          <div
            className="snap-preview"
            style={{
              left: `${(snap.x / 12) * 100}%`,
              top: snap.y * ROW_HEIGHT,
              width: `${(snap.w / 12) * 100}%`,
              height: snap.h * ROW_HEIGHT,
            }}
          />
        )}
        {layout.items.map((item) => (
          <StudioItem
            key={item.id}
            item={item}
            table={table}
            schemaRelations={relations}
            schema={schema}
            selected={item.id === selectedId}
            onSelect={() => {
              setSelectedId(item.id);
              useStore.getState().setInspectorOpen(true);
            }}
            onDragState={setDragging}
            onResize={(patch) => updateItem(item.id, patch)}
            onGuide={setGuide}
          />
        ))}
        {guide && (
          <div
            className="resize-guide-box"
            style={{
              left: `${(guide.x / 12) * 100}%`,
              top: guide.y * ROW_HEIGHT,
              width: `${(guide.w / 12) * 100}%`,
              height: guide.h * ROW_HEIGHT - 4,
            }}
          >
            <span>
              {guide.w} × {guide.h}
            </span>
          </div>
        )}
      </div>
    </div>
  );
}

function newKindItem(
  kind: string,
  position: { x: number; y: number },
  relations: Relation[],
  portalRelation: string,
): LayoutItem | null {
  if (kind === "section") {
    return {
      id: newItemId("section"),
      kind: { kind: "section", title: "Section" },
      x: 0,
      y: position.y,
      w: 12,
      h: 1,
    };
  }
  if (kind === "label") {
    return {
      id: newItemId("label"),
      kind: { kind: "label", text: "Label" },
      x: Math.min(position.x, 6),
      y: position.y,
      w: 6,
      h: 2,
    };
  }
  if (kind === "portal") {
    const relation = relations.find((candidate) => candidate.name === portalRelation);
    if (!relation) return null;
    return {
      id: newItemId("portal"),
      kind: {
        kind: "portal",
        relation: relation.name,
        columns: [],
        rows: 5,
      },
      x: 0,
      y: position.y,
      w: 12,
      h: 6,
    };
  }
  return null;
}

function defaultLayout(table: Table): Layout {
  const items: LayoutItem[] = [
    {
      id: newItemId("section"),
      kind: { kind: "section", title: table.name },
      x: 0,
      y: 0,
      w: 12,
      h: 1,
    },
  ];
  let y = 2;
  table.columns.forEach((column, index) => {
    items.push({
      id: newItemId("field"),
      kind: { kind: "field", column: column.name },
      x: index % 2 === 0 ? 0 : 6,
      y,
      w: 6,
      h: 2,
    });
    if (index % 2 === 1) y += 3;
  });
  return {
    id: "",
    name: `${table.name} Form`,
    table: table.name,
    grid: { columns: 12, row_height: 28 },
    items,
    updated_at: Date.now(),
  };
}
