import type { Layout, LayoutItem, Relation, Table } from "../bindings";
import { columnTypeLabel, newItemId } from "../types";

export function StudioPalette({
  table,
  layout,
  update,
  setSelectedId,
  setDragging,
  addKind,
  portalRelation,
  setPortalRelation,
  relations,
}: {
  table: Table;
  layout: Layout;
  update: (layout: Layout) => void;
  setSelectedId: (id: string) => void;
  setDragging: (dragging: boolean) => void;
  addKind: (kind: string) => void;
  portalRelation: string;
  setPortalRelation: (name: string) => void;
  relations: Relation[];
}) {
  const placed = new Set(
    layout.items
      .filter((item) => item.kind.kind === "field")
      .map((item) => (item.kind.kind === "field" ? item.kind.column : "")),
  );
  const available = table.columns.filter((column) => !placed.has(column.name));

  return (
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
      {([
        { kind: "section", name: "Section", description: "Heading" },
        { kind: "label", name: "Label", description: "Static text" },
      ] as const).map(({ kind, name, description }) => (
        <div
          key={kind}
          className="studio-source-item"
          draggable
          onDragStart={(event) => {
            event.dataTransfer.setData("text/plain", `slab-kind:${kind}`);
            setDragging(true);
          }}
          onDragEnd={() => setDragging(false)}
          onClick={() => addKind(kind)}
        >
          <span className="palette-item-name">{name}</span>
          <small>{description}</small>
        </div>
      ))}

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
  );
}

export function nextRow(layout: Layout): number {
  return Math.max(0, ...layout.items.map((item) => item.y + item.h)) + 1;
}
