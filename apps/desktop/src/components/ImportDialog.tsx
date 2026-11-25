import { useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { ImportOptionsDto, ImportPreviewDto, ImportValidationDto } from "../bindings";
import { api } from "../api";
import { useFocusTrap } from "../focus";
import { usePresence } from "../presence";
import { errorText, useStore } from "../store";
import { Icon } from "./Icons";

function defaultOptions(): ImportOptionsDto {
  return {
    hasHeaders: true,
    delimiter: ",",
    maxPreviewRows: 20,
    maxIssues: 100,
    skipInvalid: false,
    inferenceLimit: 5_000,
    batchSize: 500,
  };
}

export function ImportDialog() {
  const open = useStore((state) => state.importOpen);
  const schema = useStore((state) => state.schema);
  const activeTable = useStore((state) => state.activeTable);
  const setOpen = useStore((state) => state.setImportOpen);
  const notify = useStore((state) => state.notify);
  const refreshAfterImport = useStore((state) => state.refreshAfterImport);

  const [path, setPath] = useState<string | null>(null);
  const [format, setFormat] = useState<"csv" | "json">("csv");
  const [options, setOptions] = useState<ImportOptionsDto>(defaultOptions);
  const [preview, setPreview] = useState<ImportPreviewDto | null>(null);
  const [target, setTarget] = useState("");
  const [mapping, setMapping] = useState<Record<string, string>>({});
  const [validation, setValidation] = useState<ImportValidationDto | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    setPreview(null);
    setValidation(null);
    setBusy(false);
    setTarget(activeTable ?? schema?.tables[0]?.name ?? "");
  }, [open, activeTable, schema]);

  const modalRef = useRef<HTMLDivElement>(null);
  const presence = usePresence(open, 110);
  useFocusTrap(open, modalRef, presence.mounted);

  if (!presence.mounted) return null;

  const chooseFile = async () => {
    const chosen = await openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "Data", extensions: ["csv", "tsv", "json"] }],
    });
    if (typeof chosen !== "string") return;
    setPath(chosen);
    if (chosen.toLowerCase().endsWith(".json")) {
      setFormat("json");
      setOptions((current) => ({ ...current, delimiter: "," }));
    } else {
      setFormat("csv");
      setOptions((current) => ({
        ...current,
        delimiter: chosen.toLowerCase().endsWith(".tsv") ? "\t" : ",",
      }));
    }
    setPreview(null);
    setValidation(null);
  };

  const autoMap = (sourceColumns: string[], tableName: string) => {
    const table = schema?.tables.find((candidate) => candidate.name === tableName);
    const next: Record<string, string> = {};
    if (!table) return next;
    for (const source of sourceColumns) {
      const match = table.columns.find(
        (column) => column.name.toLowerCase() === source.toLowerCase(),
      );
      if (match) next[source] = match.name;
    }
    return next;
  };

  const runPreview = async () => {
    if (!path) return;
    setBusy(true);
    try {
      const result = await api.previewImport(path, format, options);
      setPreview(result);
      setValidation(null);
      const nextTarget = target || schema?.tables[0]?.name || "";
      setTarget(nextTarget);
      setMapping(autoMap(result.columns.map((column) => column.name), nextTarget));
    } catch (error) {
      notify("error", errorText(error));
    } finally {
      setBusy(false);
    }
  };

  const runValidate = async () => {
    if (!path || !target) return;
    setBusy(true);
    try {
      const report = await api.validateImport(path, format, target, mapping, options);
      setValidation(report);
    } catch (error) {
      notify("error", errorText(error));
    } finally {
      setBusy(false);
    }
  };

  const runImport = async () => {
    if (!path || !target) return;
    setBusy(true);
    try {
      const summary = await api.applyImport(path, format, target, mapping, options);
      notify(
        "success",
        `Imported ${summary.inserted} row${summary.inserted === 1 ? "" : "s"}${
          summary.skipped ? `, skipped ${summary.skipped}` : ""
        }`,
      );
      await refreshAfterImport();
      setOpen(false);
    } catch (error) {
      notify("error", errorText(error));
    } finally {
      setBusy(false);
    }
  };

  const targetDefinition = schema?.tables.find((table) => table.name === target);
  const mappedCount = Object.values(mapping).filter(Boolean).length;

  return (
    <div
      className={`modal-backdrop ${presence.closing ? "is-closing" : ""}`}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) setOpen(false);
      }}
    >
      <div
        ref={modalRef}
        className={`modal import-modal ${presence.className}`}
        role="dialog"
        aria-modal="true"
        aria-label="Import"
        data-testid="import-dialog"
        tabIndex={-1}
      >
        <header className="modal-header">
          <div>
            <span className="eyebrow">Import</span>
            <h2>Stream in records</h2>
          </div>
          <button type="button" className="icon-button" onClick={() => setOpen(false)}>
            <Icon name="x" size={15} />
          </button>
        </header>
        <div className="import-steps">
          <span className={path ? "done" : "current"}>1 · Source</span>
          <span className={preview ? "done" : path ? "current" : ""}>2 · Match</span>
          <span className={validation ? "done" : preview ? "current" : ""}>3 · Import</span>
        </div>
        <div className="modal-body import-body">
          <div className="import-source">
            <div className="import-source-row">
              <button type="button" className="button ghost" onClick={() => void chooseFile()}>
                <Icon name="folder" size={14} /> Choose file
              </button>
              <span className="file-path">{path ? basename(path) : "No file chosen"}</span>
            </div>
            <div className="import-source-options">
              <label className="compact-field">
                <span>Format</span>
                <select
                  className="select"
                  value={format}
                  onChange={(event) => setFormat(event.target.value as "csv" | "json")}
                >
                  <option value="csv">CSV</option>
                  <option value="json">JSON</option>
                </select>
              </label>
              <label className="inline-check">
                <input
                  type="checkbox"
                  checked={options.hasHeaders}
                  onChange={(event) =>
                    setOptions({ ...options, hasHeaders: event.target.checked })
                  }
                />
                First row is headers
              </label>
              <label className="compact-field">
                <span>Delimiter</span>
                <input
                  className="field-input small"
                  maxLength={2}
                  value={options.delimiter === "\t" ? "\\t" : options.delimiter}
                  onChange={(event) =>
                    setOptions({
                      ...options,
                      delimiter:
                        event.target.value === "\\t" ? "\t" : event.target.value.slice(0, 1),
                    })
                  }
                />
              </label>
              <label className="compact-field">
                <span>Infer Rows</span>
                <input
                  className="field-input small"
                  type="number"
                  min={1}
                  max={100_000}
                  value={options.inferenceLimit}
                  onChange={(event) =>
                    setOptions({
                      ...options,
                      inferenceLimit: Math.max(1, Number(event.target.value) || 1),
                    })
                  }
                />
              </label>
              <label className="compact-field">
                <span>Batch</span>
                <input
                  className="field-input small"
                  type="number"
                  min={1}
                  max={10_000}
                  value={options.batchSize}
                  onChange={(event) =>
                    setOptions({
                      ...options,
                      batchSize: Math.max(1, Number(event.target.value) || 1),
                    })
                  }
                />
              </label>
              <span className="toolbar-spacer" />
              <button
                type="button"
                className="button primary"
                disabled={!path || busy}
                onClick={() => void runPreview()}
              >
                {busy ? "Reading…" : "Preview"}
              </button>
            </div>
          </div>

          {preview && (
            <>
              <div className="import-summary-line">
                <span>
                  {preview.rowsScanned.toLocaleString()} rows · {preview.columns.length} columns
                  · {preview.format.toUpperCase()}
                  {preview.inferenceLimited
                    ? ` · inferred from first ${preview.inferenceRows.toLocaleString()}`
                    : ""}
                </span>
                {preview.issues.length > 0 && (
                  <span className="badge warning">{preview.issues.length} structural issues</span>
                )}
              </div>
              <div className="import-preview-wrap">
                <table className="import-preview">
                  <thead>
                    <tr>
                      {preview.columns.map((column) => (
                        <th key={column.name}>
                          <span>{column.name}</span>
                          <small>{column.ty}</small>
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {preview.sample.map((row) => (
                      <tr key={row.rowNumber}>
                        {row.values.map((value, index) => (
                          <td key={index}>{value ?? <span className="dash">null</span>}</td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>

              <div className="import-map">
                <label className="field-label">
                  Target table
                  <select
                    className="select"
                    value={target}
                    onChange={(event) => {
                      const next = event.target.value;
                      setTarget(next);
                      setMapping(
                        autoMap(
                          preview.columns.map((column) => column.name),
                          next,
                        ),
                      );
                      setValidation(null);
                    }}
                  >
                    {schema?.tables.map((table) => (
                      <option key={table.name} value={table.name}>
                        {table.name}
                      </option>
                    ))}
                  </select>
                </label>
                <div className="mapping-list">
                  {preview.columns.map((column) => (
                    <label className="mapping-row" key={column.name}>
                      <span className="mapping-source">
                        {column.name}
                        <small>{column.ty}</small>
                      </span>
                      <Icon name="chevronRight" size={12} />
                      <select
                        className="select"
                        value={mapping[column.name] ?? ""}
                        onChange={(event) => {
                          const next = { ...mapping };
                          if (event.target.value) next[column.name] = event.target.value;
                          else delete next[column.name];
                          setMapping(next);
                          setValidation(null);
                        }}
                      >
                        <option value="">Skip</option>
                        {targetDefinition?.columns
                          .filter((candidate) => candidate.ty.kind !== "formula")
                          .map((candidate) => (
                            <option key={candidate.name} value={candidate.name}>
                              {candidate.name}
                            </option>
                          ))}
                      </select>
                    </label>
                  ))}
                </div>
                <div className="import-actions">
                  <span className="muted">{mappedCount} mapped columns</span>
                  <button
                    type="button"
                    className="button ghost"
                    disabled={busy || mappedCount === 0}
                    onClick={() => void runValidate()}
                  >
                    Validate rows
                  </button>
                  <button
                    type="button"
                    className="button primary"
                    disabled={busy || mappedCount === 0 || validation === null}
                    onClick={() => void runImport()}
                  >
                    {busy ? "Working…" : "Import records"}
                  </button>
                </div>
                {validation && (
                  <div className={`validation-box ${validation.issues.length ? "bad" : "good"}`}>
                    <strong>
                      {validation.validRows.toLocaleString()} of{" "}
                      {validation.rows.toLocaleString()} rows valid
                    </strong>
                    {validation.issues.length > 0 && (
                      <ul className="issue-list">
                        {validation.issues.slice(0, 8).map((issue, index) => (
                          <li key={index}>
                            Row {issue.rowNumber}
                            {issue.column ? ` · ${issue.column}` : ""}: {issue.message}
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                )}
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function basename(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] ?? path;
}
