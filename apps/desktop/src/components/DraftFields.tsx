import { useEffect, useState } from "react";
import type { Column, Value } from "../bindings";
import {
  columnTypeLabel,
  defaultValue,
  isComputedColumn,
  isNumericColumn,
  parseInput,
} from "../types";

export function currencyFor(column: Column): string {
  return column.ty.kind === "currency" ? column.ty.currency : "USD";
}

export function fallbackValue(column: Column): Value {
  if (column.default !== null && column.default !== undefined) return column.default;
  switch (column.ty.kind) {
    case "text":
      return { Text: "" };
    case "integer":
      return { Integer: 0 };
    case "decimal":
      return { Decimal: "0.00" };
    case "currency":
      return { Currency: { currency: column.ty.currency, amount: "0.00" } };
    case "boolean":
      return { Boolean: false };
    case "date":
      return { Date: formatTodayIso() };
    case "timestamp":
      return { Timestamp: new Date().toISOString() };
    case "select":
      return column.ty.options[0] ? { Select: column.ty.options[0] } : "Null";
    case "attachmentRef":
      return "Null";
    default:
      return defaultValue(column);
  }
}

function formatTodayIso(): string {
  const now = new Date();
  const month = String(now.getMonth() + 1).padStart(2, "0");
  const day = String(now.getDate()).padStart(2, "0");
  return `${now.getFullYear()}-${month}-${day}`;
}

export interface DraftFieldProps {
  column: Column;
  value: Value | null;
  onChange: (value: Value) => void;
}

export function DraftField({ column, value, onChange }: DraftFieldProps) {
  const [text, setText] = useState(() => textFor(column, value));

  useEffect(() => {
    setText(textFor(column, value));
  }, [column, value]);

  if (isComputedColumn(column)) return null;
  if (column.ty.kind === "boolean") {
    const checked =
      value !== null && typeof value === "object" && "Boolean" in value ? value.Boolean : false;
    return (
      <label className="draft-check">
        <input
          type="checkbox"
          checked={checked}
          onChange={(event) => onChange({ Boolean: event.target.checked })}
        />
        <span>{checked ? "Yes" : "No"}</span>
      </label>
    );
  }
  if (column.ty.kind === "select") {
    const current =
      value !== null && typeof value === "object" && "Select" in value ? value.Select : "";
    return (
      <select
        className="field-input"
        value={current}
        onChange={(event) =>
          onChange(event.target.value === "" ? "Null" : { Select: event.target.value })
        }
      >
        <option value="">—</option>
        {column.ty.options.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
    );
  }
  const type = column.ty.kind === "date" ? "date" : column.ty.kind === "integer" ? "number" : "text";
  return (
    <span className={`field-shell ${column.ty.kind === "currency" ? "has-suffix" : ""}`}>
      <input
        className={`field-input ${isNumericColumn(column) ? "numeric" : ""}`}
        type={type}
        inputMode={isNumericColumn(column) ? "decimal" : undefined}
        value={text}
        placeholder={column.ty.kind === "timestamp" ? "YYYY-MM-DDTHH:MM:SSZ" : undefined}
        onChange={(event) => {
          const next = event.target.value;
          setText(next);
          const parsed = parseInput(column, next);
          if (parsed !== null) onChange(parsed);
        }}
        onBlur={() => setText(textFor(column, value))}
      />
      {column.ty.kind === "currency" && (
        <span className="field-suffix">{currencyFor(column)}</span>
      )}
    </span>
  );
}

export function DraftFieldLabel({ column }: { column: Column }) {
  return (
    <span className="field-label">
      {column.name}
      <span className="field-type">{columnTypeLabel(column.ty)}</span>
    </span>
  );
}

function textFor(column: Column, value: Value | null): string {
  if (value === null || value === "Null") {
    if (column.ty.kind === "date") return formatTodayIso();
    return "";
  }
  if (typeof value === "string") return "";
  if ("Text" in value) return value.Text;
  if ("Integer" in value) return String(value.Integer);
  if ("Decimal" in value) return value.Decimal;
  if ("Currency" in value) return value.Currency.amount;
  if ("Date" in value) return value.Date;
  if ("Timestamp" in value) return value.Timestamp;
  if ("Select" in value) return value.Select;
  if ("Boolean" in value) return value.Boolean ? "true" : "false";
  if ("AttachmentRef" in value) return value.AttachmentRef;
  return "";
}
