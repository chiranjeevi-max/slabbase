import type { Layout, Relation, Table } from "../bindings";
import { columnTypeLabel } from "../types";
import { GRID_COLUMNS, type StudioItemSource } from "./studioGeometry";

export function StudioPalette({
  table,
  layout,
  addItem,
  onDragItem,
  portalRelation,
  setPortalRelation,
  relations,
}: {
  table: Table;
  layout: Layout;
  addItem: (item: StudioItemSource) => void;
  onDragItem: (item: StudioItemSource | null) => void;
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
  const elements: { name: string; description: string; item: StudioItemSource }[] = [
    { name: "Section", description: "Heading", item: { kind: { kind: "section", title: "Section" }, w: GRID_COLUMNS, h: 1 } },
    { name: "Label", description: "Static text", item: { kind: { kind: "label", text: "Label" }, w: 6, h: 2 } },
  ];
  const portal: StudioItemSource = {
    kind: { kind: "portal", relation: portalRelation, columns: [], rows: 5 },
    w: GRID_COLUMNS,
    h: 6,
  };
  const portalAvailable = relations.some((relation) => relation.name === portalRelation);

  const sourceActions = (item: StudioItemSource) => ({
    draggable: true,
    onDragStart: (event: React.DragEvent) => {
      const kind = item.kind;
      const payload = kind.kind === "field" ? `slab-column:${kind.column}` : `slab-kind:${kind.kind}`;
      event.dataTransfer.setData("text/plain", payload);
      onDragItem(item);
    },
    onDragEnd: () => onDragItem(null),
    onClick: () => addItem(item),
  });

  return (
    <aside className="studio-palette" aria-label="Form elements">
      <span className="palette-heading">Fields</span>
      {available.length === 0 && <p className="studio-palette-empty">All fields placed</p>}
      {available.map((column) => (
        <button
          type="button"
          key={column.name}
          className="studio-source-item"
          {...sourceActions({ kind: { kind: "field", column: column.name }, w: 6, h: 2 })}
          title={`Add ${column.name}`}
        >
          <span className="palette-item-name">{column.name}</span>
          <small>{columnTypeLabel(column.ty)}</small>
        </button>
      ))}

      <span className="palette-heading">Elements</span>
      {elements.map(({ item, name, description }) => (
        <button
          type="button"
          key={item.kind.kind}
          className="studio-source-item"
          {...sourceActions(item)}
        >
          <span className="palette-item-name">{name}</span>
          <small>{description}</small>
        </button>
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
      <button
        type="button"
        className="studio-source-item"
        disabled={!portalAvailable}
        {...(portalAvailable ? sourceActions(portal) : {})}
      >
        <span className="palette-item-name">Portal</span>
        <small>Child table</small>
      </button>
    </aside>
  );
}
