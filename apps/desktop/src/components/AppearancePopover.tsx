import { useEffect, useRef, useState, type CSSProperties } from "react";
import {
  ACCENT_IDS,
  ACCENTS,
  accentHexFor,
  normalizeHex,
  type AppearancePrefs,
  type Density,
  type SurfaceMode,
  type ThemePreference,
} from "../appearance";
import { usePresence } from "../presence";
import { useStore } from "../store";
import { Icon } from "./Icons";

const THEMES: Array<{ value: ThemePreference; label: string }> = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "system", label: "System" },
];

const SURFACES: Array<{ value: SurfaceMode; label: string }> = [
  { value: "solid", label: "Solid" },
  { value: "glass", label: "Glass" },
];

const DENSITIES: Array<{ value: Density; label: string }> = [
  { value: "compact", label: "Compact" },
  { value: "default", label: "Default" },
  { value: "comfortable", label: "Comfortable" },
];

export function AppearancePopover() {
  const open = useStore((state) => state.appearanceOpen);
  const appearance = useStore((state) => state.appearance);
  const resolvedTheme = useStore((state) => state.resolvedTheme);
  const setOpen = useStore((state) => state.setAppearanceOpen);
  const setAppearance = useStore((state) => state.setAppearance);

  const anchorRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const presence = usePresence(open, 110);
  const [hexDraft, setHexDraft] = useState(appearance.customAccent);

  useEffect(() => {
    setHexDraft(appearance.customAccent);
  }, [appearance.customAccent, open]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: MouseEvent) => {
      if (anchorRef.current?.contains(event.target as Node)) return;
      setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      setOpen(false);
      buttonRef.current?.focus();
    };
    window.addEventListener("mousedown", onPointerDown);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("mousedown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [open, setOpen]);

  const commitHex = () => {
    const normalized = normalizeHex(hexDraft);
    if (normalized) {
      setAppearance({ accent: "custom", customAccent: normalized });
      setHexDraft(normalized);
    } else {
      setHexDraft(appearance.customAccent);
    }
  };

  return (
    <div className="appearance-anchor" ref={anchorRef}>
      <button
        ref={buttonRef}
        type="button"
        className={`icon-control ${open ? "active" : ""}`}
        title="Appearance"
        aria-label="Appearance"
        aria-haspopup="dialog"
        aria-expanded={open}
        data-testid="appearance-button"
        onClick={() => setOpen(!open)}
      >
        <Icon name="sliders" size={14} strokeWidth={1.5} />
      </button>
      {presence.mounted && (
        <div
          className={`appearance-popover ${presence.className}`}
          role="dialog"
          aria-label="Appearance"
          data-testid="appearance-popover"
        >
        <header className="appearance-head">
          <span>Appearance</span>
          <button
            type="button"
            className="icon-control micro"
            aria-label="Close appearance"
            onClick={() => setOpen(false)}
          >
            <Icon name="x" size={10} />
          </button>
        </header>

        <section className="appearance-section">
          <span className="appearance-label">Theme</span>
          <div className="segmented appearance-segment" role="group" aria-label="Theme">
            {THEMES.map((theme) => (
              <button
                type="button"
                key={theme.value}
                aria-pressed={appearance.theme === theme.value}
                onClick={() => setAppearance({ theme: theme.value })}
              >
                {theme.label}
              </button>
            ))}
          </div>
        </section>

        <section className="appearance-section">
          <span className="appearance-label">Surface</span>
          <div className="segmented appearance-segment" role="group" aria-label="Surface">
            {SURFACES.map((surface) => (
              <button
                type="button"
                key={surface.value}
                aria-pressed={appearance.surface === surface.value}
                onClick={() => setAppearance({ surface: surface.value })}
              >
                {surface.label}
              </button>
            ))}
          </div>
        </section>

        <section className="appearance-section">
          <span className="appearance-label">Accent</span>
          <div className="accent-grid" role="group" aria-label="Accent">
            {ACCENT_IDS.map((id) => (
              <button
                type="button"
                key={id}
                className="accent-swatch"
                data-accent-option={id}
                aria-label={`${ACCENTS[id].label} accent`}
                aria-pressed={appearance.accent === id}
                title={ACCENTS[id].label}
                style={
                  {
                    "--swatch-color": accentHexFor(
                      { ...appearance, accent: id } as AppearancePrefs,
                      resolvedTheme,
                    ),
                  } as CSSProperties
                }
                onClick={() => setAppearance({ accent: id })}
              />
            ))}
            <button
              type="button"
              className="accent-swatch custom"
              aria-label="Custom accent"
              aria-pressed={appearance.accent === "custom"}
              title="Custom accent"
              data-accent-option="custom"
              style={
                {
                  "--swatch-color": accentHexFor(
                    { ...appearance, accent: "custom" },
                    resolvedTheme,
                  ),
                } as CSSProperties
              }
              onClick={() => setAppearance({ accent: "custom" })}
            />
          </div>
          <div className="accent-custom">
            <input
              type="color"
              aria-label="Custom accent color"
              value={normalizeHex(hexDraft) ?? appearance.customAccent}
              onChange={(event) => {
                const next = normalizeHex(event.target.value);
                if (!next) return;
                setHexDraft(next);
                setAppearance({ accent: "custom", customAccent: next });
              }}
            />
            <input
              type="text"
              className="field-input"
              aria-label="Custom accent hex"
              data-testid="accent-hex"
              value={hexDraft}
              spellCheck={false}
              onChange={(event) => setHexDraft(event.target.value)}
              onBlur={commitHex}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  commitHex();
                }
                if (event.key === "Escape") setHexDraft(appearance.customAccent);
              }}
            />
          </div>
        </section>

        <section className="appearance-section">
          <span className="appearance-label">Row density</span>
          <div className="segmented appearance-segment" role="group" aria-label="Row density">
            {DENSITIES.map((density) => (
              <button
                type="button"
                key={density.value}
                aria-pressed={appearance.density === density.value}
                onClick={() => setAppearance({ density: density.value })}
              >
                {density.label}
              </button>
            ))}
          </div>
        </section>
        </div>
      )}
    </div>
  );
}
