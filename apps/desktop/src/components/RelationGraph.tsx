import { useEffect, useRef, useState } from "react";
import type { Schema } from "../bindings";

export function RelationGraph({
  schema,
  activeTable,
  onSelect,
}: {
  schema: Schema | null;
  activeTable: string;
  onSelect: (name: string) => void;
}) {
  const canvasRef = useRef<HTMLDivElement>(null);
  const [positions, setPositions] = useState<Record<string, { x: number; y: number }>>({});
  const [drag, setDrag] = useState<{
    name: string;
    pointerId: number;
    offsetX: number;
    offsetY: number;
  } | null>(null);

  useEffect(() => {
    if (!schema) return;
    const next: Record<string, { x: number; y: number }> = {};
    schema.tables.forEach((table, index) => {
      next[table.name] = {
        x: 5 + (index % 3) * 30,
        y: 7 + Math.floor(index / 3) * 30,
      };
    });
    setPositions(next);
  }, [schema]);

  if (!schema || schema.tables.length === 0) {
    return <p className="muted pad">Create a table to see the relation graph.</p>;
  }

  const move = (event: React.PointerEvent) => {
    if (!drag || drag.pointerId !== event.pointerId) return;
    const rect = canvasRef.current?.getBoundingClientRect();
    if (!rect) return;
    const x = Math.max(
      0,
      Math.min(76, ((event.clientX - rect.left) / rect.width) * 100 - drag.offsetX),
    );
    const y = Math.max(
      0,
      Math.min(82, ((event.clientY - rect.top) / rect.height) * 100 - drag.offsetY),
    );
    setPositions((current) => ({ ...current, [drag.name]: { x, y } }));
  };

  return (
    <div className="schema-map-wrap">
      <div
        ref={canvasRef}
        className="schema-map"
        onPointerMove={move}
        onPointerUp={() => setDrag(null)}
        onPointerLeave={() => setDrag(null)}
      >
        <svg className="schema-lines" viewBox="0 0 100 100" preserveAspectRatio="none">
          <defs>
            <marker
              id="schema-arrow"
              markerWidth="6"
              markerHeight="6"
              refX="5"
              refY="3"
              orient="auto"
            >
              <path d="M0,0 L6,3 L0,6 Z" fill="var(--text-3)" />
            </marker>
          </defs>
          {schema.relations.map((relation) => {
            const parent = positions[relation.parent_table];
            const child = positions[relation.child_table];
            if (!parent || !child) return null;
            const x1 = parent.x + 23;
            const y1 = parent.y + 9;
            const x2 = child.x;
            const y2 = child.y + 9;
            const midX = (x1 + x2) / 2;
            return (
              <path
                key={relation.name}
                d={`M${x1},${y1} C${midX},${y1} ${midX},${y2} ${x2},${y2}`}
                className="schema-line"
                markerEnd="url(#schema-arrow)"
                vectorEffect="non-scaling-stroke"
              >
                <title>{relation.name}</title>
              </path>
            );
          })}
        </svg>
        {schema.tables.map((table) => {
          const position = positions[table.name] ?? { x: 5, y: 7 };
          const isActive = table.name === activeTable;
          return (
            <button
              type="button"
              key={table.id}
              className={`schema-node ${isActive ? "active" : ""} ${
                drag?.name === table.name ? "dragging" : ""
              }`}
              style={{ left: `${position.x}%`, top: `${position.y}%` }}
              onPointerDown={(event) => {
                event.currentTarget.setPointerCapture(event.pointerId);
                const canvas = canvasRef.current?.getBoundingClientRect();
                if (!canvas) return;
                const pointerX = ((event.clientX - canvas.left) / canvas.width) * 100;
                const pointerY = ((event.clientY - canvas.top) / canvas.height) * 100;
                setDrag({
                  name: table.name,
                  pointerId: event.pointerId,
                  offsetX: pointerX - position.x,
                  offsetY: pointerY - position.y,
                });
              }}
              onPointerMove={move}
              onPointerUp={(event) => {
                event.currentTarget.releasePointerCapture(event.pointerId);
                setDrag(null);
              }}
              onClick={() => onSelect(table.name)}
            >
              <span className="schema-node-name">{table.name}</span>
              <span className="schema-node-meta">
                <span>{table.columns.length} columns</span>
                <span className="schema-node-links">
                  {schema.relations.filter((relation) => relation.parent_table === table.name).length} out
                  <span className="dot-sep" />
                  {schema.relations.filter((relation) => relation.child_table === table.name).length} in
                </span>
              </span>
            </button>
          );
        })}
        {schema.relations.length === 0 && (
          <p className="schema-map-empty">No relations yet</p>
        )}
      </div>
    </div>
  );
}
