import { confirm as confirmDialog } from "@tauri-apps/plugin-dialog";

export function isTauri(): boolean {
  const internals = window.__TAURI_INTERNALS__ as
    | { invoke?: unknown; transformCallback?: unknown }
    | undefined;
  return (
    typeof internals?.invoke === "function" &&
    typeof internals?.transformCallback === "function"
  );
}

/**
 * Uses the platform confirmation sheet for irreversible actions. Falls back to
 * the web dialog when the app runs outside the Tauri shell (Vite in a browser).
 */
export async function confirmDestructive(
  message: string,
  title: string,
  okLabel: string,
): Promise<boolean> {
  try {
    return await confirmDialog(message, {
      title,
      kind: "warning",
      okLabel,
      cancelLabel: "Cancel",
    });
  } catch {
    return window.confirm(message);
  }
}
