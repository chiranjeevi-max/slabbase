import { useEffect, useMemo, useRef, useState } from "react";
import type {
  Column,
  Layout,
  LayoutItem,
  LayoutItemKind,
  Relation,
  Schema,
  Table,
} from "../bindings";
import { useStore } from "../store";
import { columnTypeLabel, isComputedColumn, newItemId } from "../types";
import { Icon } from "./Icons";

const ROW_HEIGHT = 30;

export function FormStudio() {
  const schema = useStore((state) => state.schema);
  const activeTable = useStore((state) => state.activeTable);
  const activeLayoutId = useStore((state) => state.activeLayoutId);
  const inspectorOpen = useStore((state) => state.inspectorOpen);
  const saveLayout = useStore((state) => state.saveLayout);
  const setFormStatus = useStore((state) => state.setFormStatus);
  const [layout, setLayout] = useState<Layout | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [snap, setSnap] = useState<{ x: number; y: number; w: number; h: number } | null>(null);
  const [dragging, setDragging] = useState(false);
  const [saving, setSaving] = useState(false);
  const [guide, setGuide] = useState<{ x: number; y: number; w: number; h: number } | null>(
    null,
  );
  const canvasRef = useRef<HTMLDivElement>(null);
  const layoutRef = useRef<Layout | null>(null);
  const loadedLayout = useRef<{ key: string; updatedAt: number } | null>(null);
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
      loaded.updatedAt === updatedAt &&
      layoutRef.current !== null
    ) {
      // A schema refresh (undo, column add, relation change) replaces the
      // schema object but not this persisted layout. Keep the in-progress form
      // and its selection instead of discarding unsaved drags.
      return;
    }
    setLayout(existing ?? null);
    setSelectedId(null);
    setPortalRelation(relations[0]?.name ?? "");
    loadedLayout.current = { key, updatedAt };
  }, [schema, activeTable, activeLayoutId, relations]);

  useEffect(() => {
    if (!layout) {
      setFormStatus(null);
      return;
    }
    setFormStatus({ name: layout.name, items: layout.items.length });
    return () => setFormStatus(null);
  }, [layout, setFormStatus]);

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

  const selected = layout.items.find((item) => item.id === selectedId) ?? null;
  const placed = new Set(
    layout.items
      .filter((item) => item.kind.kind === "field")
      .map((item) => (item.kind.kind === "field" ? item.kind.column : "")),
  );
  const available = table.columns.filter((column) => !placed.has(column.name));
  const canvasHeight = Math.max(
    520,
    Math.max(0, ...layout.items.map((item) => item.y + item.h)) * ROW_HEIGHT + 80,
  );

  const update = (next: Layout) => setLayout({ ...next, items: [...next.items] });

  const updateItem = (id: string, patch: Partial<LayoutItem>) => {
    update({
      ...layout,
      items: layout.items.map((item) => (item.id === id ? { ...item, ...patch } : item)),
    });
  };

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

  const save = async () => {
    setSaving(true);
    await saveLayout({ ...layout, updated_at: Date.now() });
    setSaving(false);
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
        <aside className="studio-palette" aria-label="Form elements">
          <span className="palette-heading">Fields</span>
          {available.length === 0 && <p className="studio-palette-empty">All fields placed</p>}
          {available.map((column) => (
            <div
              key={column.name}
              className="studio-source-item"
              draggable
              onDragStart={(event) => {
                event.dataTransfer.setData("text/plain", `slab-column:${column.name}`);
                setDragging(true);
              }}
              onDragEnd={() => setDragging(false)}
              onClick={() => {
                const item: LayoutItem = {
                  id: newItemId("field"),
                  kind: { kind: "field", column: column.name },
                  x: 0,
                  y: nextRow(layout),
                  w: 6,
                  h: 2,
                };
                update({ ...layout, items: [...layout.items, item] });
                setSelectedId(item.id);
              }}
              title={`Add ${column.name}`}
            >
              <span className="palette-item-name">{column.name}</span>
              <small>{columnTypeLabel(column.ty)}</small>
            </div>
          ))}

          <span className="palette-heading">Elements</span>
          <div
            className="studio-source-item"
            draggable
            onDragStart={(event) => {
              event.dataTransfer.setData("text/plain", "slab-kind:section");
              setDragging(true);
            }}
            onDragEnd={() => setDragging(false)}
            onClick={() => addKind("section")}
          >
            <span className="palette-item-name">Section</span>
            <small>Heading</small>
          </div>
          <div
            className="studio-source-item"
            draggable
            onDragStart={(event) => {
              event.dataTransfer.setData("text/plain", "slab-kind:label");
              setDragging(true);
            }}
            onDragEnd={() => setDragging(false)}
            onClick={() => addKind("label")}
          >
            <span className="palette-item-name">Label</span>
            <small>Static text</small>
          </div>

          <span className="palette-heading">Portal</span>
          <select
            className="select palette-select"
            value={portalRelation}
            aria-label="Child relation"
            onChange={(event) => setPortalRelation(event.target.value)}
          >
            {relations.length === 0 && <option value="">No child relation</option>}
            {relations.map((relation) => (
              <option key={relation.name} value={relation.name}>
                {relation.name}
              </option>
            ))}
          </select>
          <div
            className={`studio-source-item ${!portalRelation ? "disabled" : ""}`}
            draggable={Boolean(portalRelation)}
            onDragStart={(event) => {
              if (!portalRelation) return;
              event.dataTransfer.setData("text/plain", "slab-kind:portal");
              setDragging(true);
            }}
            onDragEnd={() => setDragging(false)}
            onClick={() => portalRelation && addKind("portal")}
          >
            <span className="palette-item-name">Portal</span>
            <small>Child table</small>
          </div>
        </aside>

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

function StudioItem({
  item,
  table,
  schemaRelations,
  schema,
  selected,
  onSelect,
  onDragState,
  onResize,
  onGuide,
}: {
  item: LayoutItem;
  table: Table;
  schemaRelations: Relation[];
  schema: Schema | null;
  selected: boolean;
  onSelect: () => void;
  onDragState: (dragging: boolean) => void;
  onResize: (patch: Partial<LayoutItem>) => void;
  onGuide: (guide: { x: number; y: number; w: number; h: number } | null) => void;
}) {
  const resizeRef = useRef(false);
  const resizeOrigin = useRef<{
    handle: ResizeHandle;
    startX: number;
    startY: number;
    rect: DOMRect;
    item: LayoutItem;
  } | null>(null);
  const rounded = {
    left: `${(item.x / 12) * 100}%`,
    top: item.y * ROW_HEIGHT,
    width: `${(item.w / 12) * 100}%`,
    height: item.h * ROW_HEIGHT - 4,
  };

  const beginResize = (event: React.PointerEvent, handle: ResizeHandle) => {
    event.preventDefault();
    event.stopPropagation();
    const canvas = event.currentTarget.closest(".canvas");
    if (!(canvas instanceof HTMLElement)) return;
    resizeRef.current = true;
    event.currentTarget.setPointerCapture(event.pointerId);
    resizeOrigin.current = {
      handle,
      startX: event.clientX,
      startY: event.clientY,
      rect: canvas.getBoundingClientRect(),
      item: { ...item },
    };
    onGuide({ x: item.x, y: item.y, w: item.w, h: item.h });
  };

  const moveResize = (event: React.PointerEvent) => {
    const origin = resizeOrigin.current;
    if (!origin || !(origin.rect.width > 0)) return;
    const rawDx = ((event.clientX - origin.startX) / origin.rect.width) * 12;
    const rawDy = (event.clientY - origin.startY) / ROW_HEIGHT;
    if (!Number.isFinite(rawDx) || !Number.isFinite(rawDy)) return;
    const dx = Math.round(rawDx);
    const dy = Math.round(rawDy);
    let { x, y, w, h } = origin.item;
    if (origin.handle.includes("e")) {
      w = Math.max(1, Math.min(12 - x, origin.item.w + dx));
    }
    if (origin.handle.includes("w")) {
      const nextX = Math.max(0, Math.min(origin.item.x + origin.item.w - 1, origin.item.x + dx));
      x = nextX;
      w = origin.item.w + (origin.item.x - nextX);
    }
    if (origin.handle.includes("s")) {
      h = Math.max(1, origin.item.h + dy);
    }
    if (origin.handle.includes("n")) {
      const nextY = Math.max(0, Math.min(origin.item.y + origin.item.h - 1, origin.item.y + dy));
      y = nextY;
      h = origin.item.h + (origin.item.y - nextY);
    }
    onResize({ x, y, w, h });
    onGuide({ x, y, w, h });
  };

  const endResize = (event: React.PointerEvent) => {
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    resizeRef.current = false;
    resizeOrigin.current = null;
    onGuide(null);
  };

  return (
    <div
      className={`studio-item ${selected ? "selected" : ""} ${
        resizeRef.current ? "resizing" : ""
      }`}
      style={rounded}
      draggable
      onDragStart={(event) => {
        if (resizeRef.current) {
          event.preventDefault();
          return;
        }
        event.dataTransfer.setData("text/plain", `slab-move:${item.id}`);
        onDragState(true);
      }}
      onDragEnd={() => onDragState(false)}
      onClick={(event) => {
        event.stopPropagation();
        onSelect();
      }}
    >
      {item.kind.kind === "section" && (
        <div className="studio-section">
          <span>{item.kind.title || "Section"}</span>
          <span className="form-rule" />
        </div>
      )}
      {item.kind.kind === "label" && (
        <div className="studio-label">{item.kind.text || "Label"}</div>
      )}
      {item.kind.kind === "field" &&
        (() => {
          const columnName = item.kind.column;
          const column = table.columns.find((candidate) => candidate.name === columnName);
          if (!column) return null;
          return (
            <div className="studio-field">
              <span className="field-label">
                {column.name}
                {isComputedColumn(column) && <span className="computed-mark"> ƒ</span>}
              </span>
              <div className="studio-value">{columnTypeLabel(column.ty)}</div>
            </div>
          );
        })()}
      {item.kind.kind === "portal" &&
        (() => {
          const portal = item.kind;
          const relation = schemaRelations.find(
            (candidate) => candidate.name === portal.relation,
          );
          const child = schema?.tables.find(
            (candidate) => candidate.name === relation?.child_table,
          );
          const columns: Column[] =
            portal.columns.length > 0
              ? portal.columns
                  .map((name) => child?.columns.find((column) => column.name === name))
                  .filter((column): column is Column => column !== undefined)
              : (child?.columns ?? []).slice(0, 5);
          return (
            <div className="studio-portal">
              <div className="portal-header">
                <span>{portal.relation}</span>
                <span className="muted">Portal</span>
              </div>
              <div className="portal-preview">
                {columns.map((column) => (
                  <span key={column.name} className="portal-preview-cell">
                    {column.name}
                  </span>
                ))}
              </div>
            </div>
          );
        })()}
      {selected &&
        (["nw", "n", "ne", "e", "se", "s", "sw", "w"] as ResizeHandle[]).map((handle) => (
          <span
            key={handle}
            className={`resize-handle ${handle}`}
            onPointerDown={(event) => beginResize(event, handle)}
            onPointerMove={moveResize}
            onPointerUp={endResize}
            onPointerCancel={endResize}
          />
        ))}
    </div>
  );
}

type ResizeHandle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";

function ItemInspector({
  item,
  table,
  relations,
  onChange,
  onRemove,
}: {
  item: LayoutItem;
  table: Table;
  relations: Relation[];
  onChange: (patch: Partial<LayoutItem>) => void;
  onRemove: () => void;
}) {
  const kind = item.kind;
  return (
    <div className="inspector-body">
      <span className="palette-heading">Item</span>
      <label className="inspector-field">
        <span>Type</span>
        <input className="field-input" value={kindLabel(kind)} readOnly />
      </label>
      {kind.kind === "section" && (
        <label className="inspector-field">
          <span>Title</span>
          <input
            className="field-input"
            value={kind.title}
            onChange={(event) => onChange({ kind: { kind: "section", title: event.target.value } })}
          />
        </label>
      )}
      {kind.kind === "label" && (
        <label className="inspector-field">
          <span>Text</span>
          <input
            className="field-input"
            value={kind.text}
            onChange={(event) => onChange({ kind: { kind: "label", text: event.target.value } })}
          />
        </label>
      )}
      {kind.kind === "field" && (
        <label className="inspector-field">
          <span>Column</span>
          <select
            className="select"
            value={kind.column}
            onChange={(event) =>
              onChange({ kind: { kind: "field", column: event.target.value } })
            }
          >
            {table.columns.map((column) => (
              <option key={column.name} value={column.name}>
                {column.name}
              </option>
            ))}
          </select>
        </label>
      )}
      {kind.kind === "portal" && (
        <>
          <label className="inspector-field">
            <span>Relation</span>
            <select
              className="select"
              value={kind.relation}
              onChange={(event) =>
                onChange({
                  kind: { ...kind, relation: event.target.value },
                })
              }
            >
              {relations.map((relation) => (
                <option key={relation.name} value={relation.name}>
                  {relation.name}
                </option>
              ))}
            </select>
          </label>
          <label className="inspector-field">
            <span>Visible Rows</span>
            <input
              className="field-input inspector-number"
              type="number"
              value={kind.rows}
              onChange={(event) => {
                const parsed = Number(event.target.value);
                if (!Number.isFinite(parsed)) return;
                const rows = Math.max(1, Math.min(50, Math.trunc(parsed)));
                onChange({ kind: { ...kind, rows } });
              }}
            />
          </label>
        </>
      )}
      <span className="palette-heading">Position</span>
      <div className="inspector-grid">
        {(["x", "y", "w", "h"] as const).map((key) => (
          <label className="inspector-field" key={key}>
            <span>{key.toUpperCase()}</span>
            <input
              className="field-input inspector-number"
              type="number"
              min={key === "w" || key === "h" ? 1 : 0}
              max={key === "x" ? 11 : key === "w" ? 12 : 40}
              value={item[key]}
              onChange={(event) => {
                const parsed = Number(event.target.value);
                if (!Number.isFinite(parsed)) return;
                const minimum = key === "w" || key === "h" ? 1 : 0;
                const maximum = key === "x" ? 11 : key === "w" ? 12 : 40;
                const value = Math.max(minimum, Math.min(maximum, Math.trunc(parsed)));
                onChange({ [key]: value });
              }}
            />
          </label>
        ))}
      </div>
      <button type="button" className="button danger-ghost compact" onClick={onRemove}>
        <Icon name="trash" size={13} strokeWidth={1.5} /> Remove Item
      </button>
    </div>
  );
}

function kindLabel(kind: LayoutItemKind): string {
  if (kind.kind === "field") return "Field";
  if (kind.kind === "portal") return "Portal";
  if (kind.kind === "label") return "Label";
  return "Section";
}

function nextRow(layout: Layout): number {
  return Math.max(0, ...layout.items.map((item) => item.y + item.h)) + 1;
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
