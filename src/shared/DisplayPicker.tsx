import type { DisplayInfo, WindowPreview } from "./types";

const TILE_CAP = 4;

/** Current window first, then the top of the stack, at most four. */
export function importantWindows(displays: DisplayInfo[], limit = TILE_CAP): DisplayInfo[] {
  const top = displays.slice(-limit);
  const current = displays.find((item) => item.current);
  const picked =
    current && !top.some((item) => item.id === current.id)
      ? [current, ...top.slice(1)]
      : top;
  const focused = picked.filter((item) => item.current);
  const rest = picked.filter((item) => !item.current).reverse();
  return [...focused, ...rest];
}

export function quarterSize(width: number, height: number) {
  const safeW = Math.max(width, 1);
  const safeH = Math.max(height, 1);
  return {
    width: Math.max(1, Math.round(safeW / 4)),
    height: Math.max(1, Math.round(safeH / 4)),
  };
}

export default function DisplayPicker({
  displays,
  selectedIds,
  previews,
  pending,
  onToggle,
  onHover,
  onLeave,
}: {
  displays: DisplayInfo[];
  selectedIds: number[];
  previews: Record<number, WindowPreview>;
  pending: boolean;
  onToggle: (id: number) => void;
  onHover: (src: string, alt: string, el: HTMLElement) => void;
  onLeave: () => void;
}) {
  const shown = displays.slice(0, TILE_CAP);
  if (shown.length === 0) {
    return <p className="hint">No windows found.</p>;
  }

  const selected = new Set(selectedIds);

  return (
    <div className={`window-tiles count-${shown.length}`}>
      {shown.map((display) => {
        const on = selected.has(display.id);
        const preview = previews[display.id];
        const label = `${display.name}${display.current ? " · current" : ""}`;
        const fullW = Math.min(display.width || 1280, 1280);
        const fullH = Math.max(
          1,
          Math.round((display.height || 720) * (fullW / Math.max(display.width || 1280, 1))),
        );
        const box = preview ? quarterSize(preview.width, preview.height) : quarterSize(fullW, fullH);
        return (
          <button
            key={display.id}
            type="button"
            className={on ? "window-tile on" : "window-tile"}
            aria-pressed={on}
            title={on ? `Remove ${display.name}` : `Select ${display.name}`}
            onClick={() => onToggle(display.id)}
            onMouseEnter={(event) => {
              if (preview?.dataUrl) onHover(preview.dataUrl, label, event.currentTarget);
            }}
            onMouseLeave={onLeave}
            onFocus={(event) => {
              if (preview?.dataUrl) onHover(preview.dataUrl, label, event.currentTarget);
            }}
            onBlur={onLeave}
          >
            <span className={pending && preview?.dataUrl ? "window-tile-shot pending" : "window-tile-shot"}>
              {preview?.dataUrl ? (
                <img src={preview.dataUrl} alt={label} style={box} />
              ) : (
                <span className="window-tile-placeholder" style={box} />
              )}
            </span>
            <span className="window-tile-bar" title={label}>
              {label}
            </span>
          </button>
        );
      })}
    </div>
  );
}
