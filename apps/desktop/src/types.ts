import type {
  Column,
  ColumnType,
  Money,
  Record as SlabRecord,
  Table,
  Value,
} from "./bindings";

export type { RowValues } from "./api";

export function isNull(
  value: Value | null | undefined,
): value is "Null" | null | undefined {
  return value === undefined || value === null || value === "Null";
}

export function valueText(value: Value | null | undefined): string {
  if (isNull(value)) return "";
  if (typeof value === "string") return "";
  if ("Text" in value) return value.Text;
  if ("Integer" in value) return String(value.Integer);
  if ("Decimal" in value) return value.Decimal;
  if ("Currency" in value) return `${value.Currency.currency} ${value.Currency.amount}`;
  if ("Boolean" in value) return value.Boolean ? "Yes" : "No";
  if ("Date" in value) return value.Date;
  if ("Timestamp" in value) return value.Timestamp.replace("T", " ").replace(".000Z", "Z");
  if ("Select" in value) return value.Select;
  if ("AttachmentRef" in value) return value.AttachmentRef;
  return "";
}

export function cellText(value: Value | null | undefined): string {
  if (isNull(value)) return "";
  if (typeof value === "string") return "";
  if ("Text" in value) return value.Text;
  if ("Integer" in value) return value.Integer.toLocaleString();
  if ("Decimal" in value) return value.Decimal;
  if ("Currency" in value) return value.Currency.amount;
  if ("Boolean" in value) return value.Boolean ? "Yes" : "No";
  if ("Date" in value) return value.Date;
  if ("Timestamp" in value) {
    const date = new Date(value.Timestamp);
    if (Number.isNaN(date.getTime())) return value.Timestamp;
    return new Intl.DateTimeFormat(undefined, {
      year: "numeric",
      month: "short",
      day: "2-digit",
    }).format(date);
  }
  if ("Select" in value) return value.Select;
  if ("AttachmentRef" in value) return value.AttachmentRef;
  return "";
}

export function isNumericColumn(column: Column): boolean {
  const kind = column.ty.kind;
  return kind === "integer" || kind === "decimal" || kind === "currency";
}

export function isComputedColumn(column: Column): boolean {
  const kind = column.ty.kind;
  return kind === "formula" || kind === "lookup";
}

const LONG_TEXT_FIELD = /(e-?mail|url|uri|website|address|notes?|description|brief|summary|bio|comment|message|details)/i;
const MULTILINE_TEXT_FIELD = /(notes?|description|brief|summary|bio|address|comment|message|details)/i;

/**
 * True when a text-like field should take the full inspector width instead of
 * being paired in the two-column inspector grid.
 */
export function isLongField(column: Column): boolean {
  const kind = column.ty.kind;
  if (kind !== "text" && kind !== "attachmentRef") return false;
  return LONG_TEXT_FIELD.test(column.name);
}

/**
 * True when the value reads as a paragraph and should use a wrapping textarea.
 */
export function isMultilineField(column: Column): boolean {
  return column.ty.kind === "text" && MULTILINE_TEXT_FIELD.test(column.name);
}

export function columnTypeLabel(ty: ColumnType): string {
  switch (ty.kind) {
    case "text":
      return "Text";
    case "integer":
      return "Integer";
    case "decimal":
      return `Decimal · ${ty.scale}`;
    case "currency":
      return `Currency · ${ty.currency}`;
    case "boolean":
      return "Boolean";
    case "date":
      return "Date";
    case "timestamp":
      return "Timestamp";
    case "select":
      return "Select";
    case "attachmentRef":
      return "Attachment";
    case "formula":
      return "Formula";
    case "lookup":
      return "Rollup";
  }
}

export function columnTypeGlyph(ty: ColumnType): string {
  switch (ty.kind) {
    case "currency":
      return "$";
    case "integer":
    case "decimal":
      return "#";
    case "select":
      return "▾";
    case "date":
    case "timestamp":
      return "◷";
    case "boolean":
      return "✓";
    case "attachmentRef":
      return "↗";
    case "formula":
      return "fx";
    case "lookup":
      return "→";
    default:
      return "Aa";
  }
}

export function columnHint(column: Column): string {
  if (column.ty.kind === "formula") return column.ty.expression;
  if (column.ty.kind === "lookup") {
    return `${column.ty.aggregate}(${column.ty.relation}.${column.ty.column})`;
  }
  if (column.ty.kind === "select") return column.ty.options.join(", ");
  return columnTypeLabel(column.ty);
}

export function defaultValue(column: Column): Value {
  if (column.default !== null && column.default !== undefined) return column.default;
  if (column.ty.kind === "boolean") return { Boolean: false };
  return "Null";
}

export function parseInput(column: Column, raw: string): Value | null {
  const trimmed = raw;
  switch (column.ty.kind) {
    case "text":
      return { Text: trimmed };
    case "attachmentRef":
      return trimmed.trim() === "" ? "Null" : { AttachmentRef: trimmed };
    case "select":
      if (trimmed === "") return "Null";
      if (!column.ty.options.includes(trimmed)) return null;
      return { Select: trimmed };
    case "integer": {
      if (trimmed.trim() === "") return "Null";
      const value = Number.parseInt(trimmed, 10);
      if (!Number.isFinite(value) || !Number.isSafeInteger(value)) return null;
      return { Integer: value };
    }
    case "decimal":
      if (trimmed.trim() === "") return "Null";
      return { Decimal: trimmed.trim() };
    case "currency":
      if (trimmed.trim() === "") return "Null";
      return {
        Currency: {
          currency: column.ty.currency,
          amount: trimmed.trim(),
        },
      };
    case "boolean":
      return { Boolean: trimmed === "true" };
    case "date":
      return trimmed.trim() === "" ? "Null" : { Date: trimmed.trim() };
    case "timestamp":
      return trimmed.trim() === "" ? "Null" : { Timestamp: trimmed.trim() };
    case "formula":
    case "lookup":
      return null;
  }
}

export function inputText(column: Column, value: Value | null | undefined): string {
  if (isNull(value)) return "";
  if (typeof value === "string") return "";
  switch (column.ty.kind) {
    case "currency":
      return "Currency" in value ? value.Currency.amount : "";
    case "decimal":
      return "Decimal" in value ? value.Decimal : "";
    case "boolean":
      return "Boolean" in value && value.Boolean ? "true" : "false";
    case "integer":
      return "Integer" in value ? String(value.Integer) : "";
    case "date":
      return "Date" in value ? value.Date : "";
    case "timestamp":
      return "Timestamp" in value ? value.Timestamp : "";
    case "select":
      if ("Select" in value) return value.Select;
      if ("Text" in value) return value.Text;
      return "";
    default:
      return valueText(value);
  }
}

export function recordTitle(table: Table | undefined, record: SlabRecord): string {
  if (!table) return `#${record.id}`;
  for (const name of ["Number", "Name", "Title", "Description"]) {
    if (table.columns.some((column) => column.name === name)) {
      const value = record.values[name];
      const text = valueText(value);
      if (text) return text;
    }
  }
  return `#${record.id}`;
}

export function newItemId(prefix: string): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return `${prefix}_${crypto.randomUUID()}`;
  }
  return `${prefix}_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}

export function formatMoneyText(money: Money): string {
  return `${money.currency} ${money.amount}`;
}

export function formatTimestamp(millis: number): string {
  const date = new Date(millis);
  if (Number.isNaN(date.getTime())) return String(millis);
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

export function formatToday(): string {
  return new Intl.DateTimeFormat(undefined, {
    year: "numeric",
    month: "long",
    day: "numeric",
  }).format(new Date());
}

export function titleCase(input: string): string {
  return input
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/^./, (letter) => letter.toUpperCase());
}
