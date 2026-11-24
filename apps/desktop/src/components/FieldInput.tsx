import { useEffect, useRef, useState } from "react";
import type { Column, Value } from "../bindings";
import { useStore } from "../store";
import { inputText, isComputedColumn, isNumericColumn, parseInput, valueText } from "../types";

interface FieldInputProps {
  column: Column;
  value: Value | null | undefined;
  onCommit: (value: Value) => Promise<boolean>;
  compact?: boolean;
  autoFocus?: boolean;
  pulse?: boolean;
}

export function FieldInput({
  column,
  value,
  onCommit,
  compact = false,
  autoFocus = false,
  pulse = false,
}: FieldInputProps) {
  const [text, setText] = useState(() => inputText(column, value));
  const committed = useRef(false);
  const inputRef = useRef<HTMLInputElement | HTMLSelectElement>(null);

  useEffect(() => {
    setText(inputText(column, value));
  }, [column, value]);

  if (isComputedColumn(column)) {
    const display = valueText(value);
    return (
      <div className={`field-value readonly ${pulse ? "pulse" : ""}`} title={columnHint(column)}>
        <span className="computed-mark">ƒ</span>
        {display === "" ? <span className="dash">—</span> : display}
      </div>
    );
  }

  const commit = async () => {
    if (committed.current) return;
    if (text === inputText(column, value)) return;
    const parsed = parseInput(column, text);
    if (parsed === null) {
      useStore.getState().notify("error", `${column.name} expects a valid ${column.ty.kind} value`);
      setText(inputText(column, value));
      return;
    }
    committed.current = true;
    const ok = await onCommit(parsed);
    committed.current = false;
    if (!ok) setText(inputText(column, value));
  };

  const handleKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "Enter") {
      event.preventDefault();
      void commit();
      inputRef.current?.blur();
    } else if (event.key === "Escape") {
      event.preventDefault();
      setText(inputText(column, value));
      inputRef.current?.blur();
    }
  };

  if (column.ty.kind === "select") {
    return (
      <select
        ref={inputRef as React.RefObject<HTMLSelectElement>}
        className={`field-input ${compact ? "compact" : ""} ${pulse ? "pulse" : ""}`}
        value={text}
        autoFocus={autoFocus}
        onChange={(event) => {
          const next = event.target.value;
          setText(next);
          if (next === "") {
            void onCommit("Null");
            return;
          }
          const parsed = parseInput(column, next);
          if (parsed !== null) void onCommit(parsed);
        }}
        onKeyDown={handleKeyDown}
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

  if (column.ty.kind === "boolean") {
    const checked = value !== null && value !== undefined && typeof value === "object" && "Boolean" in value
      ? value.Boolean
      : false;
    return (
      <label className={`field-check ${compact ? "compact" : ""} ${pulse ? "pulse" : ""}`}>
        <input
          type="checkbox"
          checked={checked}
          onChange={(event) => {
            void onCommit({ Boolean: event.target.checked });
          }}
        />
        <span>{checked ? "Yes" : "No"}</span>
      </label>
    );
  }

  const type =
    column.ty.kind === "date"
      ? "date"
      : column.ty.kind === "integer"
        ? "number"
        : "text";
  return (
    <span className={`field-shell ${compact ? "compact" : ""}`}>
      <input
        ref={inputRef as React.RefObject<HTMLInputElement>}
        className={`field-input ${compact ? "compact" : ""} ${
          isNumericColumn(column) ? "numeric" : ""
        } ${pulse ? "pulse" : ""}`}
        type={type}
        inputMode={isNumericColumn(column) ? "decimal" : undefined}
        value={text}
        autoFocus={autoFocus}
        placeholder={
          column.ty.kind === "timestamp"
            ? "YYYY-MM-DDTHH:MM:SSZ"
            : column.ty.kind === "currency"
              ? "0.00"
              : undefined
        }
        onChange={(event) => setText(event.target.value)}
        onBlur={() => void commit()}
        onKeyDown={handleKeyDown}
      />
      {column.ty.kind === "currency" && !compact && (
        <span className="field-suffix">{currencyLabel(column)}</span>
      )}
    </span>
  );
}

function currencyLabel(column: Column): string {
  return column.ty.kind === "currency" ? column.ty.currency : "USD";
}

function columnHint(column: Column): string {
  return column.ty.kind === "formula" ? column.ty.expression : "Rollup";
}
