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

  function list(items: DisplayInfo[], selectedList: boolean) {
    return (
      <ul className={selectedList ? "window-list selected" : "window-list available"}>
        {items.map((display) => (
          <li
            key={`${display.id}-${display.name}-${display.x}-${display.y}`}
            className={selectedList ? "window-row on" : "window-row"}
          >
            <button type="button" className="name" onClick={() => onToggle(display.id)}>
              {display.name}
              {display.current ? " · current" : ""}
            </button>
            {selectedList && (
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
        ))}
      </ul>
    );
  }

  return (
    <div className="window-picker">
      {selected.length > 0 && list(selected, true)}
      {available.length > 0 && list(available, false)}
    </div>
  );
}
