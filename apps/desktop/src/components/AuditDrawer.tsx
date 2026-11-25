import { useRef } from "react";
import { useFocusTrap } from "../focus";
import { usePresence } from "../presence";
import { useStore } from "../store";
import { formatTimestamp } from "../types";
import { Icon } from "./Icons";

export function AuditDrawer() {
  const open = useStore((state) => state.auditOpen);
  const audit = useStore((state) => state.audit);
  const undoState = useStore((state) => state.undoState);
  const close = useStore((state) => state.closeAudit);
  const drawerRef = useRef<HTMLElement>(null);
  const presence = usePresence(open, 110);
  useFocusTrap(open, drawerRef, presence.mounted);

  if (!presence.mounted) return null;

  return (
    <div
      className={`drawer-backdrop ${presence.closing ? "is-closing" : ""}`}
      onMouseDown={close}
    >
      <aside
        ref={drawerRef}
        className={`drawer audit-drawer ${presence.className}`}
        onMouseDown={(event) => event.stopPropagation()}
        aria-label="Audit log"
        data-testid="audit-drawer"
        tabIndex={-1}
      >
        <header className="drawer-header">
          <div>
            <span className="eyebrow">Local history</span>
            <h2>Audit log</h2>
          </div>
          <button
            type="button"
            className="icon-button"
            aria-label="Close audit log"
            onClick={close}
          >
            <Icon name="x" size={15} />
          </button>
        </header>
        <div className="audit-actions">
          <button
            type="button"
            className="button ghost"
            disabled={!undoState.can_undo}
            onClick={() => void useStore.getState().undo()}
          >
            <Icon name="undo" size={14} /> Undo
          </button>
          <button
            type="button"
            className="button ghost"
            disabled={!undoState.can_redo}
            onClick={() => void useStore.getState().redo()}
          >
            <Icon name="redo" size={14} /> Redo
          </button>
          <span className="muted">{undoState.depth} undoable</span>
        </div>
        <div className="audit-list">
          {audit.length === 0 && <p className="muted pad">No audit entries yet.</p>}
          {audit.map((entry) => (
            <div className="audit-entry" key={entry.id}>
              <div className="audit-top">
                <span className={`badge ${entry.schema_change ? "info" : "neutral"}`}>
                  {entry.operation}
                </span>
                <span className="audit-time">{formatTimestamp(entry.timestamp_ms)}</span>
              </div>
              <p>{entry.summary}</p>
              <div className="audit-meta">
                {entry.table && <span>{entry.table}</span>}
                {entry.row_id !== null && <span>#{entry.row_id}</span>}
                <span title={entry.hlc}>{entry.actor}</span>
              </div>
            </div>
          ))}
        </div>
        <footer className="drawer-footer">
          <span>Append-only deltas committed with each write.</span>
        </footer>
      </aside>
    </div>
  );
}
