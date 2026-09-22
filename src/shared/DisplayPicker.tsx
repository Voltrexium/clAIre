import type { DisplayInfo } from "./types";

export default function DisplayPicker({
  displays,
  selectedIds,
  onToggle,
}: {
  displays: DisplayInfo[];
  selectedIds: number[];
  onToggle: (id: number) => void;
}) {
  if (displays.length === 0) {
    return <p className="hint">No windows found.</p>;
  }

  const selected = new Set(selectedIds);

  return (
    <div className="window-picker">
      <ul className="window-list">
        {displays.map((display) => {
          const on = selected.has(display.id);
          return (
            <li key={display.id} className={on ? "window-row on" : "window-row"}>
              <button type="button" className="name" onClick={() => onToggle(display.id)}>
                {display.name}
                {display.current ? " · current" : ""}
              </button>
              {on && (
                <button
                  type="button"
                  className="remove"
                  aria-label={`Remove ${display.name}`}
                  onClick={() => onToggle(display.id)}
                >
                  ×
                </button>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
