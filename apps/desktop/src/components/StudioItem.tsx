import { useRef } from "react";
import type {
  Column,
  LayoutItem,
  LayoutItemKind,
  Relation,
  Schema,
  Table,
} from "../bindings";
import { columnTypeLabel, isComputedColumn } from "../types";
import { Icon } from "./Icons";
import { GRID_COLUMNS, gridItemStyle, MAX_GRID_ROW, ROW_HEIGHT, type GridRect } from "./studioGeometry";

export function StudioItem({
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
  onDragState: (item: LayoutItem | null) => void;
  onResize: (patch: Partial<LayoutItem>) => void;
  onGuide: (guide: GridRect | null) => void;
}) {
  const resizeRef = useRef(false);
  const resizeOrigin = useRef<{
    handle: ResizeHandle;
    startX: number;
    startY: number;
    rect: DOMRect;
    item: LayoutItem;
  } | null>(null);

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
    const rawDx = ((event.clientX - origin.startX) / origin.rect.width) * GRID_COLUMNS;
    const rawDy = (event.clientY - origin.startY) / ROW_HEIGHT;
    if (!Number.isFinite(rawDx) || !Number.isFinite(rawDy)) return;
    const dx = Math.round(rawDx);
    const dy = Math.round(rawDy);
    let { x, y, w, h } = origin.item;
    if (origin.handle.includes("e")) {
      w = Math.max(1, Math.min(GRID_COLUMNS - x, origin.item.w + dx));
    }
    if (origin.handle.includes("w")) {
      const nextX = Math.max(0, Math.min(origin.item.x + origin.item.w - 1, origin.item.x + dx));
      x = nextX;
      w = origin.item.w + (origin.item.x - nextX);
    }
    if (origin.handle.includes("s")) {
      h = Math.max(1, Math.min(MAX_GRID_ROW, origin.item.h + dy));
    }
    if (origin.handle.includes("n")) {
      const bottom = origin.item.y + origin.item.h;
      const minimum = Math.max(0, bottom - MAX_GRID_ROW);
      const maximum = Math.min(MAX_GRID_ROW, bottom - 1);
      y = Math.max(minimum, Math.min(maximum, origin.item.y + dy));
      h = bottom - y;
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
      style={gridItemStyle(item)}
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      draggable
      onDragStart={(event) => {
        if (resizeRef.current) {
          event.preventDefault();
          return;
        }
        event.dataTransfer.setData("text/plain", `slab-move:${item.id}`);
        onDragState(item);
      }}
      onDragEnd={() => onDragState(null)}
      onClick={(event) => {
        event.stopPropagation();
        onSelect();
      }}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget || (event.key !== "Enter" && event.key !== " ")) return;
        event.preventDefault();
        if (!event.repeat) onSelect();
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

export function ItemInspector({
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
                  kind: { ...kind, relation: event.target.value, columns: [] },
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
        {([
          ["x", 0, Math.max(0, GRID_COLUMNS - item.w)],
          ["y", 0, MAX_GRID_ROW],
          ["w", 1, Math.max(1, GRID_COLUMNS - item.x)],
          ["h", 1, MAX_GRID_ROW],
        ] as const).map(([key, min, max]) => (
          <label className="inspector-field" key={key}>
            <span>{key.toUpperCase()}</span>
            <input
              className="field-input inspector-number"
              type="number"
              min={min}
              max={max}
              value={item[key]}
              onChange={(event) => {
                const parsed = Number(event.target.value);
                if (!Number.isFinite(parsed)) return;
                onChange({ [key]: Math.max(min, Math.min(max, Math.trunc(parsed))) });
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
