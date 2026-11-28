import type { Column, Value } from "./bindings";
import { fixtureError } from "./fixtureState";

const MIN_I64 = -(2n ** 63n);
const MAX_I64 = 2n ** 63n - 1n;
const MAX_SCALE = 9;

export function checkedInteger(value: bigint): bigint | null {
  return value < MIN_I64 || value > MAX_I64 ? null : value;
}

export function currencyScale(currency: string): number {
  if (["JPY", "KRW", "VND", "CLP", "ISK"].includes(currency)) return 0;
  if (["BHD", "KWD", "OMR", "TND", "JOD"].includes(currency)) return 3;
  return 2;
}

interface ScaledInteger {
  units: bigint;
  scale: number;
}

function parseDecimal(text: string): ScaledInteger | null {
  const match = /^([+-]?)(\d*)(?:\.(\d*))?$/.exec(text.trim());
  if (!match) return null;
  const [, sign = "", integer = "", fraction = ""] = match;
  if (!(integer || fraction) || fraction.length > MAX_SCALE) return null;
  const units = checkedInteger(BigInt(`${sign}${integer}${fraction}`));
  return units === null ? null : { units, scale: fraction.length };
}

function rescale(decimal: ScaledInteger, scale: number, mode: "exact" | "halfEven"): bigint | null {
  if (!Number.isInteger(scale) || scale < 0 || scale > MAX_SCALE) return null;
  const shift = scale - decimal.scale;
  if (shift >= 0) return checkedInteger(decimal.units * 10n ** BigInt(shift));

  const divisor = 10n ** BigInt(-shift);
  const whole = decimal.units / divisor;
  const remainder = decimal.units % divisor;
  if (remainder === 0n) return whole;
  if (mode === "exact") return null;

  const twiceRemainder = 2n * (remainder < 0n ? -remainder : remainder);
  if (twiceRemainder > divisor || (twiceRemainder === divisor && whole % 2n !== 0n)) {
    return whole + (decimal.units < 0n ? -1n : 1n);
  }
  return whole;
}

export function minorUnits(text: string, scale: number): bigint | null {
  const parsed = parseDecimal(text);
  return parsed && rescale(parsed, scale, "exact");
}

export function decimalText(units: bigint, scale: number): string {
  if (scale === 0) return units.toString();
  const magnitude = (units < 0n ? -units : units).toString().padStart(scale + 1, "0");
  return `${units < 0n ? "-" : ""}${magnitude.slice(0, -scale)}.${magnitude.slice(-scale)}`;
}

function roundedDecimalText(text: string, scale: number): string {
  const parsed = parseDecimal(text);
  if (!parsed) fixtureError("SLAB_E_DECIMAL", `invalid decimal ${text}`);
  const units = rescale(parsed, scale, "halfEven");
  if (units === null) fixtureError("SLAB_E_NUMERIC_OVERFLOW", "rescaling a decimal");
  return decimalText(units, scale);
}

export function normalizeFixtureValue(column: Column, value: Value): Value {
  if (value === "Null") return value;
  const kind = column.ty.kind;
  if (kind === "decimal") {
    if ("Decimal" in value) return { Decimal: roundedDecimalText(value.Decimal, column.ty.scale) };
    if ("Integer" in value && Number.isInteger(value.Integer)) {
      return { Decimal: roundedDecimalText(String(value.Integer), column.ty.scale) };
    }
    fixtureError("SLAB_E_QUERY", `expected decimal for ${column.table}.${column.name}`);
  }
  if (kind === "currency") {
    const currency = column.ty.currency;
    if (!("Currency" in value) || value.Currency.currency !== currency) {
      fixtureError("SLAB_E_QUERY", `expected ${currency} for ${column.table}.${column.name}`);
    }
    const amount = roundedDecimalText(value.Currency.amount, currencyScale(currency));
    return { Currency: { currency, amount } };
  }
  return value;
}
