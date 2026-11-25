import { useEffect, useState } from "react";
import { useStore } from "../store";
import { Icon } from "./Icons";
import { SchemaManage } from "./SchemaManage";
import { RelationGraph } from "./RelationGraph";

export function SchemaView() {
  const schema = useStore((state) => state.schema);
  const activeTable = useStore((state) => state.activeTable);
  const openSchema = useStore((state) => state.openSchema);
  const [selectedTable, setSelectedTable] = useState(activeTable ?? "");

  useEffect(() => {
    if (activeTable) setSelectedTable(activeTable);
  }, [activeTable]);

  useEffect(() => {
    const names = schema?.tables.map((table) => table.name) ?? [];
    if (selectedTable && names.includes(selectedTable)) return;
    setSelectedTable(activeTable ?? names[0] ?? "");
  }, [schema, activeTable, selectedTable]);

  return (
    <div className="schema-view">
      <div className="schema-toolbar">
        <span className="schema-title">
          <Icon name="grid" size={13} strokeWidth={1.5} />
          Relations &amp; Schema
        </span>
        <span className="toolbar-spacer" />
        <button type="button" className="button compact" onClick={() => openSchema("table")}>
          + Table
        </button>
        <button
          type="button"
          className="button compact"
          onClick={() => openSchema("column", selectedTable)}
        >
          + Column
        </button>
        <button
          type="button"
          className="button compact"
          onClick={() => openSchema("relation", selectedTable)}
        >
          Add Relation
        </button>
      </div>
      <div className="schema-body">
        <section className="schema-manage-pane" aria-label="Schema management">
          <SchemaManage
            schema={schema}
            activeTable={selectedTable}
            onTableSelected={setSelectedTable}
          />
        </section>
        <section className="schema-graph-pane" aria-label="Relation graph">
          <div className="schema-pane-head">
            <span>Relation Graph</span>
            <span className="schema-pane-note">Drag tables to arrange</span>
          </div>
          <RelationGraph
            schema={schema}
            activeTable={selectedTable}
            onSelect={setSelectedTable}
          />
        </section>
      </div>
    </div>
  );
}
