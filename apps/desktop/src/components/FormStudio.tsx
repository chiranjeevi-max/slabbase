import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Layout, LayoutItem, Table } from "../bindings";
import { useStore } from "../store";
import { newItemId } from "../types";
import { Icon } from "./Icons";
import { ItemInspector, StudioItem } from "./StudioItem";
import { StudioPalette } from "./StudioPalette";
import {
  GRID_COLUMNS, gridItemStyle, MAX_GRID_ROW, ROW_HEIGHT, type GridRect, type StudioItemSource,
} from "./studioGeometry";

export function FormStudio() {
  const state = useStudioState();
  const { table, layout, startForm } = state;

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
            onClick={startForm}
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
    schema, inspectorOpen, update, selectedId, setSelectedId, startForm, save, saving,
    setDragItem, relations, portalRelation, setPortalRelation,
  } = state;
  const selected = layout.items.find((item) => item.id === selectedId) ?? null;

  const updateItem = (id: string, patch: Partial<LayoutItem>) => {
    update({
      ...layout,
      items: layout.items.map((item) => (item.id === id ? { ...item, ...patch } : item)),
    });
  };

  const addItem = (source: StudioItemSource, position?: GridRect) => {
    const { kind, w, h } = source;
    const nextRow = Math.max(0, ...layout.items.map((item) => item.y + item.h)) + 1;
    const geometry = position ?? { x: 0, y: Math.min(MAX_GRID_ROW, nextRow), w, h };
    const item = { ...geometry, id: newItemId(kind.kind), kind };
    update({ ...layout, items: [...layout.items, item] });
    setSelectedId(item.id);
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
            onClick={startForm}
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
          onDragItem={setDragItem}
          addItem={addItem}
          portalRelation={portalRelation}
          setPortalRelation={setPortalRelation}
          relations={relations}
        />

        <StudioCanvas
          state={state}
          layout={layout}
          table={table}
          addItem={addItem}
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

}

function useStudioState() {
  const schema = useStore((state) => state.schema);
  const activeTable = useStore((state) => state.activeTable);
  const activeLayoutId = useStore((state) => state.activeLayoutId);
  const inspectorOpen = useStore((state) => state.inspectorOpen);
  const saveLayout = useStore((state) => state.saveLayout);
  const setFormStatus = useStore((state) => state.setFormStatus);
  const [layout, setCurrentLayout] = useState<Layout | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [snap, setSnap] = useState<GridRect | null>(null);
  const [dragItem, setCurrentDragItem] = useState<StudioItemSource | null>(null);
  const [saving, setSaving] = useState(false);
  const [guide, setGuide] = useState<GridRect | null>(null);
  const layoutRef = useRef<Layout | null>(null);
  const loadedLayout = useRef<{ key: string; updatedAt: number } | null>(null);
  const dirtyRef = useRef(false);
  const pendingSave = useRef<Layout | null>(null);
  const setLayout = useCallback((next: Layout | null) => {
    layoutRef.current = next;
    setCurrentLayout(next);
  }, []);

  const table = schema?.tables.find((candidate) => candidate.name === activeTable);
  const relations = useMemo(
    () => schema?.relations.filter((relation) => relation.parent_table === activeTable) ?? [],
    [schema, activeTable],
  );
  const [portalRelation, setPortalRelation] = useState<string>("");
  const key = `${activeTable ?? ""}|${activeLayoutId ?? ""}`;

  const setDragItem = (item: StudioItemSource | null) => {
    setCurrentDragItem(item);
    setSnap(null);
  };

  const update = (next: Layout) => {
    dirtyRef.current = true;
    setLayout(next);
  };

  const startForm = () => {
    if (!table) return;
    pendingSave.current = null;
    setSelectedId(null);
    update(defaultLayout(table));
  };

  const save = async () => {
    const snapshot = layoutRef.current;
    if (!snapshot) return;
    pendingSave.current = snapshot;
    // Keep the live draft mounted through the metadata refresh inside saveLayout.
    dirtyRef.current = true;
    setSaving(true);
    const saved = await saveLayout(snapshot);
    setSaving(false);
    const current = useStore.getState();
    if (pendingSave.current !== snapshot || current.view !== "form"
      || current.activeTable !== activeTable || current.activeLayoutId !== activeLayoutId) return;
    pendingSave.current = null;
    const draft = layoutRef.current;
    if (!saved || !draft) return;

    const editedSinceSave = draft !== snapshot;
    dirtyRef.current = editedSinceSave;
    loadedLayout.current = { key: `${saved.table}|${saved.id}`, updatedAt: saved.updated_at };
    setLayout(editedSinceSave ? { ...draft, id: saved.id, updated_at: saved.updated_at } : saved);
    if (current.activeLayoutId !== saved.id) useStore.setState({ activeLayoutId: saved.id });
  };

  useEffect(() => {
    const existing =
      schema?.layouts.find(
        (candidate) => candidate.id === activeLayoutId && candidate.table === activeTable,
      ) ?? schema?.layouts.find((candidate) => candidate.table === activeTable);
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
    setCurrentDragItem(null);
    setSnap(null);
    setGuide(null);
    setPortalRelation(relations[0]?.name ?? "");
    loadedLayout.current = { key, updatedAt };
    dirtyRef.current = false;
    pendingSave.current = null;
  }, [schema, activeTable, activeLayoutId, key, relations, setLayout]);

  useEffect(() => () => {
    pendingSave.current = null;
  }, []);

  useEffect(() => {
    if (!layout) {
      setFormStatus(null);
      return;
    }
    setFormStatus({ name: layout.name, items: layout.items.length });
    return () => setFormStatus(null);
  }, [layout, setFormStatus]);

  return {
    schema, inspectorOpen, layout, update, startForm, selectedId, setSelectedId,
    snap, setSnap, dragItem, setDragItem, save, saving, guide, setGuide,
    table, relations, portalRelation, setPortalRelation,
  };
}

function StudioCanvas({
  state,
  layout,
  table,
  addItem,
  updateItem,
}: {
  state: ReturnType<typeof useStudioState>;
  layout: Layout;
  table: Table;
  addItem: (source: StudioItemSource, position: GridRect) => void;
  updateItem: (id: string, patch: Partial<LayoutItem>) => void;
}) {
  const {
    dragItem, setDragItem, snap, setSnap, guide, setGuide,
    selectedId, setSelectedId, schema, relations,
  } = state;
  const canvasHeight = Math.max(
    520,
    Math.max(0, ...layout.items.map((item) => item.y + item.h)) * ROW_HEIGHT + 80,
  );

  const dropPosition = (event: React.DragEvent<HTMLDivElement>, { w, h }: StudioItemSource) => {
    const rect = event.currentTarget.getBoundingClientRect();
    if (!(rect.width > 0)) return null;
    const rawX = ((event.clientX - rect.left) / rect.width) * GRID_COLUMNS;
    const rawY = (event.clientY - rect.top) / ROW_HEIGHT;
    if (!Number.isFinite(rawX) || !Number.isFinite(rawY)) return null;
    return {
      x: Math.max(0, Math.min(GRID_COLUMNS - w, Math.round(rawX))),
      y: Math.max(0, Math.min(MAX_GRID_ROW, Math.floor(rawY))),
      w,
      h,
    };
  };

  const handleDrop = (event: React.DragEvent<HTMLDivElement>) => {
    if (!dragItem) return;
    event.preventDefault();
    setDragItem(null);
    const position = dropPosition(event, dragItem);
    if (!position) return;
    if (dragItem.id) {
      updateItem(dragItem.id, { x: position.x, y: position.y });
    } else {
      addItem(dragItem, position);
    }
  };

  return (
    <div className="canvas-wrap">
      <div
        className={`canvas ${dragItem ? "dragging" : ""}`}
        style={{ height: canvasHeight }}
        onDragOver={(event) => {
          if (!dragItem) return;
          event.preventDefault();
          setSnap(dropPosition(event, dragItem));
        }}
        onDragLeave={(event) => {
          if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) {
            setSnap(null);
          }
        }}
        onDrop={handleDrop}
        onClick={() => setSelectedId(null)}
      >
        {snap && <div className="snap-preview" style={gridItemStyle(snap)} />}
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
            onDragState={setDragItem}
            onResize={(patch) => updateItem(item.id, patch)}
            onGuide={setGuide}
          />
        ))}
        {guide && (
          <div className="resize-guide-box" style={gridItemStyle(guide)}>
            <span>
              {guide.w} × {guide.h}
            </span>
          </div>
        )}
      </div>
    </div>
  );
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
