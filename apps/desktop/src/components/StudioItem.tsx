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

export const ROW_HEIGHT = 30;

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
              max={
                key === "x"
                  ? Math.max(0, 12 - item.w)
                  : key === "w"
                    ? Math.max(1, 12 - item.x)
                    : 40
              }
              value={item[key]}
              onChange={(event) => {
                const parsed = Number(event.target.value);
                if (!Number.isFinite(parsed)) return;
                const minimum = key === "w" || key === "h" ? 1 : 0;
                const maximum =
                  key === "x"
                    ? Math.max(0, 12 - item.w)
                    : key === "w"
                      ? Math.max(1, 12 - item.x)
                      : 40;
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
