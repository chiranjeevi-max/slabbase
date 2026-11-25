import { useEffect, useRef, type RefObject } from "react";

const FOCUSABLE = [
  "a[href]",
  "button:not([disabled])",
  "textarea:not([disabled])",
  "input:not([disabled]):not([type='hidden'])",
  "select:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

function focusables(node: HTMLElement): HTMLElement[] {
  return Array.from(node.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (element) => element.getClientRects().length > 0,
  );
}

export function useFocusTrap(
  open: boolean,
  ref: RefObject<HTMLElement | null>,
  active = open,
): void {
  const restoreRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!open) return;
    restoreRef.current = document.activeElement as HTMLElement | null;
    return () => {
      restoreRef.current?.focus?.();
      restoreRef.current = null;
    };
  }, [open]);

  useEffect(() => {
    if (!open || !active) return;
    const node = ref.current;
    if (!node) return;

    const timer = window.requestAnimationFrame(() => {
      const preferred = node.querySelector<HTMLElement>(
        "[autofocus], [data-autofocus], input:not([type='hidden']), textarea, select",
      );
      (preferred ?? focusables(node)[0] ?? node).focus();
    });

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Tab") return;
      const list = focusables(node);
      if (list.length === 0) {
        event.preventDefault();
        node.focus();
        return;
      }
      const first = list[0];
      const last = list[list.length - 1];
      if (!first || !last) return;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      window.cancelAnimationFrame(timer);
      document.removeEventListener("keydown", onKeyDown, true);
    };
  }, [open, active, ref]);
}
