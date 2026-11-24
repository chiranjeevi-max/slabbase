import { useEffect, useMemo, useState } from "react";
import type { SearchHit } from "../bindings";
import { api } from "../api";
import { errorText, useStore } from "../store";
import { Icon } from "./Icons";

interface Command {
  id: string;
  label: string;
  hint: string;
  run: () => void;
}

export function Palette() {
  const open = useStore((state) => state.paletteOpen);
  const setOpen = useStore((state) => state.setPaletteOpen);
  const notify = useStore((state) => state.notify);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [index, setIndex] = useState(0);

  useEffect(() => {
    if (!open) {
      setQuery("");
      setHits([]);
      setIndex(0);
    }
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const term = query.trim();
    if (term.length === 0) {
      setHits([]);
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      api
        .search(term, 8)
        .then((results) => {
          if (!cancelled) setHits(results);
        })
        .catch((error) => {
          if (!cancelled) notify("error", errorText(error));
        });
    }, 140);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [query, open, notify]);

  const commands = useMemo<Command[]>(() => {
    const state = useStore.getState();
    return [
      {
        id: "new-record",
        label: "New record",
        hint: "⌘N",
        run: () => state.beginCreate(),
      },
      {
        id: "import",
        label: "Import CSV or JSON",
        hint: "⇧⌘I",
        run: () => state.setImportOpen(true),
      },
      {
        id: "print",
        label: "Print or export PDF",
        hint: "⌘P",
        run: () => state.setPrintOpen(true),
      },
      {
        id: "form",
        label: "Open form studio",
        hint: "",
        run: () => state.setView("form"),
      },
      {
        id: "audit",
        label: "Audit log",
        hint: "",
        run: () => void state.openAudit(),
      },
      {
        id: "new-table",
        label: "New table",
        hint: "",
        run: () => state.openSchema("table"),
      },
      {
        id: "new-relation",
        label: "New relation",
        hint: "",
        run: () => state.openSchema("relation"),
      },
      {
        id: "theme",
        label: "Toggle theme",
        hint: "",
        run: () => state.toggleTheme(),
      },
    ];
  }, []);

  if (!open) return null;

  const filtered = commands.filter((command) =>
    command.label.toLowerCase().includes(query.trim().toLowerCase()),
  );
  const total = filtered.length + hits.length;

  const choose = (position: number) => {
    if (position < filtered.length) {
      const command = filtered[position];
      if (command) {
        setOpen(false);
        command.run();
      }
      return;
    }
    const hit = hits[position - filtered.length];
    if (hit) {
      setOpen(false);
      void useStore.getState().jumpToRecord(hit.table, hit.row_id);
    }
  };

  return (
    <div className="palette-backdrop" onMouseDown={() => setOpen(false)}>
      <div
        className="palette"
        onMouseDown={(event) => event.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
      >
        <div className="palette-input">
          <Icon name="search" size={16} />
          <input
            autoFocus
            value={query}
            placeholder="Search records or run a command…"
            onChange={(event) => {
              setQuery(event.target.value);
              setIndex(0);
            }}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                setOpen(false);
              } else if (event.key === "ArrowDown") {
                event.preventDefault();
                setIndex((current) => (total === 0 ? 0 : (current + 1) % total));
              } else if (event.key === "ArrowUp") {
                event.preventDefault();
                setIndex((current) => (total === 0 ? 0 : (current - 1 + total) % total));
              } else if (event.key === "Enter") {
                event.preventDefault();
                choose(index);
              }
            }}
          />
          <span className="palette-esc">esc</span>
        </div>
        <div className="palette-results">
          {filtered.length > 0 && (
            <>
              <span className="palette-group">Commands</span>
              {filtered.map((command, position) => (
                <button
                  type="button"
                  key={command.id}
                  className={`palette-row ${index === position ? "active" : ""}`}
                  onMouseEnter={() => setIndex(position)}
                  onClick={() => choose(position)}
                >
                  <span>{command.label}</span>
                  <kbd>{command.hint}</kbd>
                </button>
              ))}
            </>
          )}
          {hits.length > 0 && (
            <>
              <span className="palette-group">Records</span>
              {hits.map((hit, hitIndex) => {
                const position = filtered.length + hitIndex;
                return (
                  <button
                    type="button"
                    key={`${hit.table}-${hit.row_id}`}
                    className={`palette-row ${index === position ? "active" : ""}`}
                    onMouseEnter={() => setIndex(position)}
                    onClick={() => choose(position)}
                  >
                    <span className="hit-table">{hit.table}</span>
                    <span className="hit-snippet">
                      <Highlight snippet={hit.snippet} />
                    </span>
                    <span className="hit-id">#{hit.row_id}</span>
                  </button>
                );
              })}
            </>
          )}
          {total === 0 && (
            <div className="palette-empty">
              <Icon name="search" size={18} />
              <p>No records or commands match “{query}”.</p>
            </div>
          )}
        </div>
        <div className="palette-footer">
          <span>
            <kbd>↑</kbd>
            <kbd>↓</kbd> navigate
          </span>
          <span>
            <kbd>↵</kbd> open
          </span>
        </div>
      </div>
    </div>
  );
}

function Highlight({ snippet }: { snippet: string }) {
  const parts = snippet.split(/(<mark>|<\/mark>)/);
  let marked = false;
  return (
    <>
      {parts.map((part, index) => {
        if (part === "<mark>") {
          marked = true;
          return null;
        }
        if (part === "</mark>") {
          marked = false;
          return null;
        }
        return marked ? <mark key={index}>{part}</mark> : <span key={index}>{part}</span>;
      })}
    </>
  );
}
