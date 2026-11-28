import type { Column, Filter, Value } from "./bindings";
import { fixtureError } from "./fixtureState";
import { checkedInteger, currencyScale, decimalText, minorUnits } from "./fixtureValues";
import { isComputedColumn, isNull, valueText } from "./types";

function canonicalTimestamp(text: string): string | null {
  const match = /^(\d{4}-\d\d-\d\d)[T ](\d\d:\d\d:\d\d)(?:\.(\d{3}))?[Zz]?$/.exec(text.trim());
  if (!match || match[1]?.startsWith("0000-")) return null;
  const [, date, time, fraction = "000"] = match;
  const iso = `${date}T${time}.${fraction}Z`;
  const parsed = new Date(iso);
  return Number.isFinite(parsed.valueOf()) && parsed.toISOString() === iso ? iso : null;
}

type StoredValue = string | bigint;

function storedValue(column: Column, value: Exclude<Value, "Null">): StoredValue | null {
  const text = "Text" in value ? value.Text : null;
  const trimmed = text?.trim();
  switch (column.ty.kind) {
    case "text":
      return text;
    case "select":
      return "Select" in value ? value.Select : text;
    case "attachmentRef":
      return "AttachmentRef" in value ? value.AttachmentRef : text;
    case "integer":
      if ("Integer" in value && Number.isInteger(value.Integer)) {
        return checkedInteger(BigInt(value.Integer));
      }
      return trimmed && /^[+-]?\d+$/.test(trimmed) ? checkedInteger(BigInt(trimmed)) : null;
    case "decimal": {
      let amount = trimmed;
      if ("Decimal" in value) amount = value.Decimal;
      if ("Integer" in value) amount = String(value.Integer);
      return amount ? minorUnits(amount, column.ty.scale) : null;
    }
    case "currency": {
      const currency = column.ty.currency;
      const amount = "Currency" in value && value.Currency.currency === currency
        ? value.Currency.amount
        : trimmed;
      if (!amount) return null;
      const prefix = /^([A-Za-z]{3})\s+(.+)$/.exec(amount);
      if (prefix && prefix[1]?.toUpperCase() !== currency) return null;
      return minorUnits(prefix?.[2] ?? amount, currencyScale(currency));
    }
    case "boolean":
      if ("Boolean" in value) return value.Boolean ? 1n : 0n;
      switch (trimmed?.toLowerCase()) {
        case "yes":
        case "true":
        case "1":
          return 1n;
        case "no":
        case "false":
        case "0":
          return 0n;
        default:
          return null;
      }
    case "date": {
      const date = "Date" in value ? value.Date : trimmed;
      return date && canonicalTimestamp(`${date}T00:00:00Z`) ? date : null;
    }
    case "timestamp": {
      const timestamp = "Timestamp" in value ? value.Timestamp : trimmed;
      return timestamp ? canonicalTimestamp(timestamp) : null;
    }
    case "formula":
    case "lookup":
      return null;
  }
}

function displayText(column: Column, value: Exclude<Value, "Null">): string {
  const stored = storedValue(column, value);
  if (typeof stored === "bigint") {
    if (column.ty.kind === "boolean") return stored === 0n ? "No" : "Yes";
    if (column.ty.kind === "decimal") return decimalText(stored, column.ty.scale);
    if (column.ty.kind === "currency") {
      return decimalText(stored, currencyScale(column.ty.currency));
    }
  }
  return stored?.toString() ?? valueText(value);
}

function textNeedle(column: Column, value: Value): string {
  if (value !== "Null") {
    if ("Boolean" in value && column.ty.kind !== "boolean") return String(value.Boolean);
    if ("Timestamp" in value) return canonicalTimestamp(value.Timestamp) ?? value.Timestamp;
  }
  return valueText(value);
}

function asciiLower(text: string): string {
  return text.replace(/[A-Z]/g, (letter) => letter.toLowerCase());
}

export function compileFixtureFilter(
  column: Column,
  filter: Filter,
): (value: Value | undefined) => boolean {
  if (isComputedColumn(column)) {
    fixtureError("SLAB_E_QUERY", "computed columns cannot be filtered in a paginated grid");
  }
  const { op, value } = filter;
  if (op === "isNull") return isNull;
  if (op === "isNotNull") return (candidate) => !isNull(candidate);
  if (value === null) fixtureError("SLAB_E_QUERY", "filter requires a value");

  if (op === "contains" || op === "startsWith" || op === "endsWith") {
    const needle = asciiLower(textNeedle(column, value));
    return (candidate) => {
      if (isNull(candidate)) return false;
      const display = asciiLower(displayText(column, candidate));
      if (op === "contains") return display.includes(needle);
      if (op === "startsWith") return display.startsWith(needle);
      // SQLite's suffix expression starts at zero when the needle is empty.
      return needle ? display.endsWith(needle) : display === "";
    };
  }

  if (value === "Null") {
    if (op === "eq") return isNull;
    if (op === "notEq") return (candidate) => !isNull(candidate);
    fixtureError("SLAB_E_QUERY", "null is only valid with = or !=");
  }
  const right = storedValue(column, value);
  if (right === null) {
    fixtureError("SLAB_E_QUERY", `filter value does not fit column ${column.name}`);
  }

  return (candidate) => {
    if (isNull(candidate)) return op === "notEq";
    const left = storedValue(column, candidate);
    if (left === null) return false;
    switch (op) {
      case "eq":
        return left === right;
      case "notEq":
        return left !== right;
      case "gt":
        return left > right;
      case "gte":
        return left >= right;
      case "lt":
        return left < right;
      case "lte":
        return left <= right;
    }
  };
}
