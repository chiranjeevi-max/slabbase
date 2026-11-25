import { useEffect, useState } from "react";

export interface Presence {
  mounted: boolean;
  closing: boolean;
  className: string;
}

export function usePresence(open: boolean, exitMs: number): Presence {
  const [mounted, setMounted] = useState(open);
  const [closing, setClosing] = useState(false);

  useEffect(() => {
    if (open) {
      setMounted(true);
      setClosing(false);
      return;
    }
    if (!mounted) return;
    setClosing(true);
    const timer = window.setTimeout(() => {
      setMounted(false);
      setClosing(false);
    }, exitMs);
    return () => window.clearTimeout(timer);
  }, [open, mounted, exitMs]);

  return { mounted, closing, className: closing ? "is-closing" : "" };
}
