export type ThemePreference = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";
export type SurfaceMode = "solid" | "glass";
export type Density = "compact" | "default" | "comfortable";
export type AccentId =
  | "blue"
  | "graphite"
  | "indigo"
  | "teal"
  | "green"
  | "orange"
  | "rose"
  | "custom";

export interface AppearancePrefs {
  version: 1;
  theme: ThemePreference;
  surface: SurfaceMode;
  accent: AccentId;
  customAccent: string;
  density: Density;
}

interface AccentPreset {
  label: string;
  light: string;
  dark: string;
}

export const ACCENTS: Record<Exclude<AccentId, "custom">, AccentPreset> = {
  blue: { label: "Blue", light: "#0066cc", dark: "#0a84ff" },
  graphite: { label: "Graphite", light: "#636366", dark: "#aeaeb2" },
  indigo: { label: "Indigo", light: "#4f46e5", dark: "#7c7cff" },
  teal: { label: "Teal", light: "#007c89", dark: "#40c8e0" },
  green: { label: "Green", light: "#1f8a4c", dark: "#30d158" },
  orange: { label: "Orange", light: "#b54708", dark: "#ff9f0a" },
  rose: { label: "Rose", light: "#be123c", dark: "#ff375f" },
};

export const ACCENT_IDS = Object.keys(ACCENTS) as Array<Exclude<AccentId, "custom">>;

export const DENSITY_ROW_HEIGHT: Record<Density, number> = {
  compact: 26,
  default: 29,
  comfortable: 34,
};

export const DENSITY_LABEL: Record<Density, string> = {
  compact: "Compact",
  default: "Default",
  comfortable: "Comfortable",
};

export const DEFAULT_CUSTOM_ACCENT = "#0a84ff";

export const DEFAULT_APPEARANCE: AppearancePrefs = {
  version: 1,
  theme: "system",
  surface: "solid",
  accent: "blue",
  customAccent: DEFAULT_CUSTOM_ACCENT,
  density: "default",
};

export const APPEARANCE_STORAGE_KEY = "slabbase.appearance.v1";
const LEGACY_THEME_KEY = "slabbase.theme";

const HEX_LONG = /^#[0-9a-f]{6}$/i;
const HEX_SHORT = /^#[0-9a-f]{3}$/i;

export function normalizeHex(value: string): string | null {
  const trimmed = value.trim();
  if (HEX_LONG.test(trimmed)) return trimmed.toLowerCase();
  if (!HEX_SHORT.test(trimmed)) return null;
  const [r, g, b] = trimmed.slice(1);
  return `#${r}${r}${g}${g}${b}${b}`.toLowerCase();
}

function prefersDark(): boolean {
  return (
    typeof window !== "undefined" &&
    window.matchMedia("(prefers-color-scheme: dark)").matches
  );
}

export function resolveTheme(preference: ThemePreference): ResolvedTheme {
  if (preference === "light" || preference === "dark") return preference;
  return prefersDark() ? "dark" : "light";
}

function validTheme(value: unknown): value is ThemePreference {
  return value === "system" || value === "light" || value === "dark";
}

function validSurface(value: unknown): value is SurfaceMode {
  return value === "solid" || value === "glass";
}

function validDensity(value: unknown): value is Density {
  return value === "compact" || value === "default" || value === "comfortable";
}

function validAccent(value: unknown): value is AccentId {
  return (
    value === "custom" ||
    (typeof value === "string" && Object.prototype.hasOwnProperty.call(ACCENTS, value))
  );
}

function readLegacyTheme(): ThemePreference {
  if (typeof window === "undefined") return DEFAULT_APPEARANCE.theme;
  const legacy = window.localStorage.getItem(LEGACY_THEME_KEY);
  return legacy === "light" || legacy === "dark" ? legacy : DEFAULT_APPEARANCE.theme;
}

export function loadAppearance(): AppearancePrefs {
  if (typeof window === "undefined") return DEFAULT_APPEARANCE;
  try {
    const raw = window.localStorage.getItem(APPEARANCE_STORAGE_KEY);
    if (!raw) {
      const migrated = { ...DEFAULT_APPEARANCE, theme: readLegacyTheme() };
      saveAppearance(migrated);
      return migrated;
    }
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return DEFAULT_APPEARANCE;
    const candidate = parsed as Partial<AppearancePrefs>;
    return {
      version: 1,
      theme: validTheme(candidate.theme) ? candidate.theme : DEFAULT_APPEARANCE.theme,
      surface: validSurface(candidate.surface)
        ? candidate.surface
        : DEFAULT_APPEARANCE.surface,
      accent: validAccent(candidate.accent) ? candidate.accent : DEFAULT_APPEARANCE.accent,
      customAccent:
        typeof candidate.customAccent === "string" &&
        normalizeHex(candidate.customAccent) !== null
          ? normalizeHex(candidate.customAccent) ?? DEFAULT_CUSTOM_ACCENT
          : DEFAULT_APPEARANCE.customAccent,
      density: validDensity(candidate.density)
        ? candidate.density
        : DEFAULT_APPEARANCE.density,
    };
  } catch {
    return { ...DEFAULT_APPEARANCE, theme: readLegacyTheme() };
  }
}

export function saveAppearance(prefs: AppearancePrefs): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(APPEARANCE_STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    // Storage can be unavailable in private windows. Appearance still applies.
  }
}

export function accentHexFor(prefs: AppearancePrefs, resolved: ResolvedTheme): string {
  if (prefs.accent === "custom") {
    return normalizeHex(prefs.customAccent) ?? DEFAULT_CUSTOM_ACCENT;
  }
  return ACCENTS[prefs.accent][resolved];
}

function linearChannel(value: number): number {
  const channel = value / 255;
  return channel <= 0.04045
    ? channel / 12.92
    : Math.pow((channel + 0.055) / 1.055, 2.4);
}

function relativeLuminance(hex: string): number {
  const normalized = normalizeHex(hex) ?? DEFAULT_CUSTOM_ACCENT;
  const r = Number.parseInt(normalized.slice(1, 3), 16);
  const g = Number.parseInt(normalized.slice(3, 5), 16);
  const b = Number.parseInt(normalized.slice(5, 7), 16);
  return (
    0.2126 * linearChannel(r) + 0.7152 * linearChannel(g) + 0.0722 * linearChannel(b)
  );
}

function contrastRatio(left: string, right: string): number {
  const a = relativeLuminance(left);
  const b = relativeLuminance(right);
  const lighter = Math.max(a, b);
  const darker = Math.min(a, b);
  return (lighter + 0.05) / (darker + 0.05);
}

export function accentForeground(hex: string): string {
  return contrastRatio(hex, "#ffffff") >= contrastRatio(hex, "#000000")
    ? "#ffffff"
    : "#000000";
}

export function applyAppearance(prefs: AppearancePrefs): ResolvedTheme {
  const resolved = resolveTheme(prefs.theme);
  if (typeof document === "undefined") return resolved;
  const root = document.documentElement;
  const accent = accentHexFor(prefs, resolved);
  root.dataset["theme"] = resolved;
  root.dataset["surface"] = prefs.surface;
  root.dataset["accent"] = prefs.accent;
  root.dataset["density"] = prefs.density;
  root.style.setProperty("--accent", accent);
  const foreground = accentForeground(accent);
  root.style.setProperty("--accent-on-solid", foreground);
  root.style.setProperty("--accent-foreground", foreground);
  root.style.setProperty("--row-h", `${DENSITY_ROW_HEIGHT[prefs.density]}px`);
  return resolved;
}

export function mergeAppearance(
  current: AppearancePrefs,
  patch: Partial<Omit<AppearancePrefs, "version">>,
): AppearancePrefs {
  return { ...current, ...patch, version: 1 };
}

export const initialAppearance = loadAppearance();
