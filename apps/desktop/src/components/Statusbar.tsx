import { useEffect, useRef, useState } from "react";
import { useStore } from "../store";
import { formatTimestamp } from "../types";
import { Icon } from "./Icons";

export function Statusbar() {
  const info = useStore((state) => state.info);
  const integrity = useStore((state) => state.integrity);
  const report = useStore((state) => state.integrityReport);
  const checkedAt = useStore((state) => state.integrityCheckedAt);
  const checkIntegrity = useStore((state) => state.checkIntegrity);
  const indexHint = useStore((state) => state.indexHint);
  const applyIndexHint = useStore((state) => state.applyIndexHint);
  const dismissIndexHint = useStore((state) => state.dismissIndexHint);

  const [detailsOpen, setDetailsOpen] = useState(false);
  const anchorRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!detailsOpen) return;
    const close = (event: MouseEvent) => {
      if (!anchorRef.current?.contains(event.target as Node)) setDetailsOpen(false);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [detailsOpen]);

  const healthy = integrity === "ok";

  return (
    <footer className="statusbar" data-testid="status-bar" aria-label="Status">
      <WorkspaceStatus />
      <div className="status-right">
        {indexHint && (
          <div className="index-hint" data-testid="index-hint">
            <button
              type="button"
              className="index-hint-action"
              title={`Create index ${indexHint.suggestedIndex}`}
              onClick={() => void applyIndexHint()}
            >
              Full scan ({Math.round(indexHint.elapsedMs)} ms) · Index &quot;
              {indexHint.column}&quot;
            </button>
            <button
              type="button"
              className="index-hint-dismiss"
              aria-label="Dismiss index suggestion"
              onClick={dismissIndexHint}
            >
              <Icon name="x" size={10} strokeWidth={1.6} />
            </button>
          </div>
        )}
        <div className="status-anchor" ref={anchorRef}>
          <button
            type="button"
            className="integrity-button"
            onClick={() => {
              if (report) setDetailsOpen((open) => !open);
              else
                void checkIntegrity().then(() => {
                  if (useStore.getState().integrityReport) setDetailsOpen(true);
                });
            }}
            title="Run SQLite integrity check"
          >
            <span className={`status-dot ${healthy ? "ok" : "warn"}`} />
            {healthy ? "Integrity OK" : "Integrity issue"}
          </button>
          {detailsOpen && (
            <div className="integrity-popover" role="dialog" aria-label="Integrity details">
              <div className="integrity-popover-head">
                <span>Integrity</span>
                <button
                  type="button"
                  className="icon-control tiny"
                  aria-label="Close"
                  onClick={() => setDetailsOpen(false)}
                >
                  <Icon name="x" size={11} />
                </button>
              </div>
              <dl>
                <div>
                  <dt>SQLite</dt>
                  <dd>{report?.integrity ?? "Not run"}</dd>
                </div>
                <div>
                  <dt>Foreign keys</dt>
                  <dd>{report ? `${report.foreignKeyViolations} violations` : "Not run"}</dd>
                </div>
                <div>
                  <dt>Pages</dt>
                  <dd>{report ? report.pageCount.toLocaleString() : "…"}</dd>
                </div>
                <div>
                  <dt>Audit chain</dt>
                  <dd>
                    {report
                      ? report.auditChain
                        ? `${report.auditEntries.toLocaleString()} entries`
                        : (report.auditTamper ?? "broken")
                      : "Not run"}
                  </dd>
                </div>
                <div>
                  <dt>Journal</dt>
                  <dd>
                    {report
                      ? report.journalMode.toUpperCase()
                      : info?.journalMode.toUpperCase() ?? "WAL"}
                  </dd>
                </div>
                {checkedAt && (
                  <div>
                    <dt>Checked</dt>
                    <dd>{formatTimestamp(checkedAt)}</dd>
                  </div>
                )}
              </dl>
              <button
                type="button"
                className="button"
                onClick={() => void checkIntegrity()}
              >
                Run Again
              </button>
            </div>
          )}
        </div>
        <span className="status-mono">SQLite WAL</span>
      </div>
    </footer>
  );
}

function WorkspaceStatus() {
  const info = useStore((state) => state.info);
  const view = useStore((state) => state.view);
  const schema = useStore((state) => state.schema);
  const activeTable = useStore((state) => state.activeTable);
  const activeLayoutId = useStore((state) => state.activeLayoutId);
  const formStatus = useStore((state) => state.formStatus);
  const auditOpen = useStore((state) => state.auditOpen);
  const audit = useStore((state) => state.audit);
  const rows = useStore((state) => state.rows);
  const elapsedMs = useStore((state) => state.elapsedMs);

  const count = activeTable
    ? (info?.tables.find((table) => table.name === activeTable)?.rowCount ??
      rows.length)
    : 0;
  const showLatency = rows.length > 0 && elapsedMs > 0;
  const formLayout =
    schema?.layouts.find((layout) => layout.id === activeLayoutId) ??
    schema?.layouts.find((layout) => layout.table === activeTable) ??
    null;
  const formName = formStatus?.name ?? formLayout?.name ?? "Form Studio";
  const formItems = formStatus?.items ?? formLayout?.items.length ?? 0;

  return (
    <div className="status-left">
      {!info ? (
        <span>Empty workspace</span>
      ) : auditOpen ? (
        <span>{audit.length.toLocaleString()} audit entries</span>
      ) : view === "form" ? (
        <>
          <span className="status-table">{formName}</span>
          <span className="status-separator" aria-hidden="true">
            ·
          </span>
          <span>{formItems.toLocaleString()} items</span>
        </>
      ) : view === "schema" ? (
        <>
          <span>{(schema?.tables.length ?? 0).toLocaleString()} tables</span>
          <span className="status-separator" aria-hidden="true">
            ·
          </span>
          <span>{(schema?.relations.length ?? 0).toLocaleString()} relations</span>
        </>
      ) : activeTable ? (
        <>
          <span className="status-table">{activeTable}</span>
          <span className="status-separator" aria-hidden="true">
            ·
          </span>
          <span>{count.toLocaleString()} rows</span>
          {showLatency && (
            <>
              <span className="status-separator" aria-hidden="true">
                ·
              </span>
              <span className="status-mono">{elapsedMs.toFixed(1)} ms</span>
            </>
          )}
        </>
      ) : (
        <span>No table selected</span>
      )}
    </div>
  );
}
