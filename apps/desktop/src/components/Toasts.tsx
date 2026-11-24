import { useStore } from "../store";
import { Icon } from "./Icons";

export function Toasts() {
  const toast = useStore((state) => state.toast);
  const clear = useStore((state) => state.clearToast);
  if (!toast) return null;
  return (
    <div className={`toast ${toast.tone}`} role="status">
      <span className="toast-dot" />
      <div>
        {toast.code && <span className="toast-code">{toast.code}</span>}
        <span>{toast.message}</span>
      </div>
      <button type="button" className="icon-button tiny" onClick={clear} aria-label="Dismiss">
        <Icon name="x" size={12} />
      </button>
    </div>
  );
}
