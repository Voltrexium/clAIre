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

  const selected = displays.filter((display) => selectedIds.includes(display.id));
  const available = displays.filter((display) => !selectedIds.includes(display.id));

  return (
    <div className="window-picker">
      {selected.length > 0 && (
        <ul className="window-list selected">
          {selected.map((display) => (
            <li key={`${display.id}-${display.name}-${display.x}-${display.y}`} className="window-row on">
              <button type="button" className="name" onClick={() => onToggle(display.id)}>
                {display.name}
                {display.current ? " · current" : ""}
              </button>
              <button
                type="button"
                className="remove"
                aria-label={`Remove ${display.name}`}
                onClick={() => onToggle(display.id)}
              >
                ×
              </button>
            </li>
          ))}
        </ul>
      )}
      {available.length > 0 && (
        <ul className="window-list available">
          {available.map((display) => (
            <li key={`${display.id}-${display.name}-${display.x}-${display.y}`} className="window-row">
              <button type="button" className="name" onClick={() => onToggle(display.id)}>
                {display.name}
                {display.current ? " · current" : ""}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
