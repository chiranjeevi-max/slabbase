import { useEffect, useRef, useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import type { Layout, PrintOptionsDto, Table } from "../bindings";
import { api } from "../api";
import { useFocusTrap } from "../focus";
import { usePresence } from "../presence";
import { errorText, pageQueryFor, useStore } from "../store";
import { Icon } from "./Icons";

export function PrintDialog() {
  const open = useStore((state) => state.printOpen);
  const schema = useStore((state) => state.schema);
  const activeTable = useStore((state) => state.activeTable);
  const selectedId = useStore((state) => state.selectedId);
  const setOpen = useStore((state) => state.setPrintOpen);
  const notify = useStore((state) => state.notify);

  const [mode, setMode] = useState<"record" | "table">("record");
  const [layoutId, setLayoutId] = useState<string>("");
  const [options, setOptions] = useState<PrintOptionsDto>({
    pageSize: "letter",
    title: null,
    subtitle: null,
    includePortals: true,
    includeEmptyFields: true,
    marginPt: null,
  });
  const [html, setHtml] = useState<string>("");
  const [warning, setWarning] = useState<string>("");
  const [busy, setBusy] = useState(false);

  const layouts = schema?.layouts.filter((layout) => layout.table === activeTable) ?? [];

  useEffect(() => {
    if (!open) return;
    setMode(selectedId !== null ? "record" : "table");
    setWarning("");
    const current =
      schema?.layouts.filter((layout) => layout.table === activeTable) ?? [];
    setLayoutId(current[0]?.id ?? "");
    setHtml("");
  }, [open, selectedId, activeTable, schema]);

  const modalRef = useRef<HTMLDivElement>(null);
  const presence = usePresence(open, 110);
  useFocusTrap(open, modalRef, presence.mounted);

  if (!presence.mounted) return null;

  const table = schema?.tables.find((candidate) => candidate.name === activeTable);

  const renderPreview = async () => {
    if (!activeTable) return;
    setBusy(true);
    try {
      if (mode === "record" && selectedId !== null) {
        const result = await api.renderFormHtml(
          activeTable,
          selectedId,
          layoutId || null,
          options,
        );
        setHtml(result);
        setWarning("");
      } else {
        const result = await api.renderTableHtml(
          activeTable,
          pageQueryFor(useStore.getState(), null, 500),
          options,
        );
        setHtml(result.html);
        setWarning(
          result.truncated
            ? `Preview stopped at the 25,000-row safety ceiling (${result.rows.toLocaleString()} rows). Filter the table to continue.`
            : "",
        );
      }
    } catch (error) {
      notify("error", errorText(error));
    } finally {
      setBusy(false);
    }
  };

  const exportPdf = async () => {
    if (!activeTable) return;
    const path = await saveDialog({
      defaultPath: `${activeTable}.pdf`,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (!path) return;
    setBusy(true);
    try {
      if (mode === "record" && selectedId !== null) {
        await api.exportFormPdf(path, activeTable, selectedId, layoutId || null, options);
        notify("success", "PDF written");
      } else {
        const result = await api.exportTablePdf(
          path,
          activeTable,
          pageQueryFor(useStore.getState(), null, 500),
          options,
        );
        if (result.truncated) {
          setWarning(
            `PDF stopped at the 25,000-row safety ceiling after ${result.rows.toLocaleString()} rows across ${result.pages.toLocaleString()} pages. Filter the table to continue.`,
          );
          notify("error", "PDF written with an explicit 25,000-row ceiling");
        } else {
          notify("success", `PDF written · ${result.rows.toLocaleString()} rows`);
        }
      }
    } catch (error) {
      notify("error", errorText(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className={`modal-backdrop ${presence.closing ? "is-closing" : ""}`}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) setOpen(false);
      }}
    >
      <div
        ref={modalRef}
        className={`modal print-modal ${presence.className}`}
        role="dialog"
        aria-modal="true"
        aria-label="Print"
        data-testid="print-dialog"
        tabIndex={-1}
      >
        <header className="modal-header">
          <div>
            <span className="eyebrow">Print</span>
            <h2>{table?.name ?? "Report"}</h2>
          </div>
          <button
            type="button"
            className="icon-button"
            aria-label="Close print dialog"
            onClick={() => setOpen(false)}
          >
            <Icon name="x" size={15} />
          </button>
        </header>
        <div className="print-body">
          <PrintOptions
            mode={mode}
            setMode={setMode}
            layouts={layouts}
            layoutId={layoutId}
            setLayoutId={setLayoutId}
            options={options}
            setOptions={setOptions}
            busy={busy}
            renderPreview={renderPreview}
            exportPdf={exportPdf}
            selectedId={selectedId}
            table={table}
          />
          <PrintPreview
            warning={warning}
            html={html}
          />
        </div>
      </div>
    </div>
  );
}

function PrintOptions({
  mode,
  setMode,
  layouts,
  layoutId,
  setLayoutId,
  options,
  setOptions,
  busy,
  renderPreview,
  exportPdf,
  selectedId,
  table,
}: {
  mode: "record" | "table";
  setMode: (mode: "record" | "table") => void;
  layouts: Layout[];
  layoutId: string;
  setLayoutId: (id: string) => void;
  options: PrintOptionsDto;
  setOptions: (options: PrintOptionsDto) => void;
  busy: boolean;
  renderPreview: () => Promise<void>;
  exportPdf: () => Promise<void>;
  selectedId: number | null;
  table: Table | undefined;
}) {
  return (
    <aside className="print-options">
      <div className="segmented">
        <button
          type="button"
          aria-pressed={mode === "record"}
          disabled={selectedId === null}
          onClick={() => setMode("record")}
        >
          Record
        </button>
        <button
          type="button"
          aria-pressed={mode === "table"}
          onClick={() => setMode("table")}
        >
          Table
        </button>
      </div>
      {mode === "record" && (
        <label className="field-label">
          Form layout
          <select
            className="select"
            value={layoutId}
            onChange={(event) => setLayoutId(event.target.value)}
          >
            {layouts.length === 0 && <option value="">Default layout</option>}
            {layouts.map((layout) => (
              <option key={layout.id} value={layout.id}>
                {layout.name}
              </option>
            ))}
          </select>
        </label>
      )}
      <label className="field-label">
        Paper
        <select
          className="select"
          value={options.pageSize}
          onChange={(event) => setOptions({ ...options, pageSize: event.target.value })}
        >
          <option value="letter">US Letter</option>
          <option value="a4">A4</option>
        </select>
      </label>
      <label className="field-label">
        Title
        <input
          className="field-input"
          placeholder={table?.name ?? "Report"}
          value={options.title ?? ""}
          onChange={(event) =>
            setOptions({ ...options, title: event.target.value || null })
          }
        />
      </label>
      <label className="field-label">
        Subtitle
        <input
          className="field-input"
          placeholder="Prepared for the client"
          value={options.subtitle ?? ""}
          onChange={(event) =>
            setOptions({ ...options, subtitle: event.target.value || null })
          }
        />
      </label>
      <label className="inline-check">
        <input
          type="checkbox"
          checked={options.includePortals}
          onChange={(event) =>
            setOptions({ ...options, includePortals: event.target.checked })
          }
        />
        Include child portals
      </label>
      <label className="inline-check">
        <input
          type="checkbox"
          checked={options.includeEmptyFields}
          onChange={(event) =>
            setOptions({ ...options, includeEmptyFields: event.target.checked })
          }
        />
        Include empty fields
      </label>
      <div className="print-actions">
        <button
          type="button"
          className="button"
          onClick={() => void renderPreview()}
          disabled={busy}
        >
          {busy ? "Rendering…" : "Preview"}
        </button>
        <button
          type="button"
          className="button primary"
          onClick={() => void exportPdf()}
          disabled={busy}
        >
          <Icon name="download" size={14} /> Export PDF
        </button>
      </div>
      {mode === "table" && (
        <p className="muted print-note">
          Table reports include up to 1,000 filtered rows from the current view.
        </p>
      )}
    </aside>
  );
}

function PrintPreview({
  warning,
  html,
}: {
  warning: string;
  html: string;
}) {
  return (
    <div className="print-preview">
      {warning && <div className="print-warning">{warning}</div>}
      {html ? (
        <iframe title="Print preview" srcDoc={html} sandbox="" />
      ) : (
        <div className="print-placeholder">
          <Icon name="printer" size={24} />
          <p>Preview renders a paginated HTML proof. PDF export uses the same layout.</p>
        </div>
      )}
    </div>
  );
}
