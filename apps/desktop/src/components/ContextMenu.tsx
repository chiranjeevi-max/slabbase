import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { Icon, type IconName } from "./Icons";

export interface ContextMenuItem {
  label: string;
  icon?: IconName;
  danger?: boolean;
  separatorBefore?: boolean;
  onSelect: () => void;
}

interface ContextMenuProps {
  x: number;
  y: number;
  items: ContextMenuItem[];
  onClose: () => void;
}

const EDGE = 8;
const EXIT_MS = 110;

export function ContextMenu({ x, y, items, onClose }: ContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  const restoreRef = useRef<HTMLElement | null>(null);
  const [closing, setClosing] = useState(false);
  const [position, setPosition] = useState({ left: x, top: y });
  const [positioned, setPositioned] = useState(false);

  const dismiss = () => {
    if (closing) return;
    setClosing(true);
    window.setTimeout(onClose, EXIT_MS);
  };

  useEffect(() => {
    restoreRef.current = document.activeElement as HTMLElement | null;
    ref.current?.focus();
    return () => {
      restoreRef.current?.focus?.();
    };
  }, []);

  useEffect(() => {
    const onPointerDown = (event: MouseEvent) => {
      if (ref.current?.contains(event.target as Node)) return;
      dismiss();
    };
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      dismiss();
    };
    const onScroll = () => dismiss();
    window.addEventListener("mousedown", onPointerDown);
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("scroll", onScroll, true);
    return () => {
      window.removeEventListener("mousedown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("scroll", onScroll, true);
    };
  });

  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const rect = element.getBoundingClientRect();
    const left = Math.max(EDGE, Math.min(x, window.innerWidth - rect.width - EDGE));
    const top = Math.max(EDGE, Math.min(y, window.innerHeight - rect.height - EDGE));
    setPosition({ left, top });
    setPositioned(true);
  }, [x, y]);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const buttons = Array.from(
      ref.current?.querySelectorAll<HTMLButtonElement>("button") ?? [],
    );
    if (buttons.length === 0) return;
    const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const direction = event.key === "ArrowDown" ? 1 : -1;
    const next = (current + direction + buttons.length) % buttons.length;
    buttons[next]?.focus();
  };

  return (
    <div
      ref={ref}
      className={`context-menu ${closing ? "is-closing" : ""}`}
      role="menu"
      aria-label="Row actions"
      data-testid="context-menu"
      tabIndex={-1}
      style={{ left: position.left, top: position.top, visibility: positioned ? "visible" : "hidden" }}
      onKeyDown={onKeyDown}
    >
      {items.map((item) => (
        <div key={item.label}>
          {item.separatorBefore && <div className="menu-separator" />}
          <button
            type="button"
            role="menuitem"
            className={item.danger ? "danger" : ""}
            onClick={() => {
              item.onSelect();
              dismiss();
            }}
          >
            {item.icon && <Icon name={item.icon} size={13} />}
            <span>{item.label}</span>
          </button>
        </div>
      ))}
    </div>
  );
}
