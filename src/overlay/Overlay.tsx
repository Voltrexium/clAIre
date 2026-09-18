import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  askClaire,
  captureDisplays,
  clearContext,
  getLatestCapture,
  getSettings,
  hideOverlay,
  listDisplays,
  newChat,
  recapture,
  setCaptureMode as persistCaptureMode,
  setWindowMode,
  fitOverlay,
} from "../shared/api";
import DisplayPicker from "../shared/DisplayPicker";
import SettingsPage from "../settings/Settings";
import { renderLiteMarkdown } from "../shared/markdown";
import type { CaptureMode, CapturePayload, DisplayInfo } from "../shared/types";

function usableLabel(mode?: string | null) {
  if (!mode || mode === "current window") return "";
  return mode;
}

type LogEntry = {
  role: "user" | "assistant";
  content: string;
  image?: string;
  imageAlt?: string;
};

export default function Overlay() {
  const [query, setQuery] = useState("");
  const [log, setLog] = useState<LogEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [capture, setCapture] = useState<CapturePayload | null>(null);
  const [searchOn, setSearchOn] = useState(false);
  const [searchAvailable, setSearchAvailable] = useState(false);
  const [usedSearch, setUsedSearch] = useState(false);
  const [usedVision, setUsedVision] = useState(false);
  const [captureMode, setCaptureMode] = useState<CaptureMode>("current");
  const [displays, setDisplays] = useState<DisplayInfo[]>([]);
  const [selectedIds, setSelectedIds] = useState<number[]>([]);
  const [showSettings, setShowSettings] = useState(false);
  const showSettingsRef = useRef(false);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const cardRef = useRef<HTMLDivElement>(null);
  const captureGen = useRef(0);
  const captureStale = useRef(true);
  const capturingRef = useRef(false);
  const captureWaiters = useRef<Array<() => void>>([]);
  const [capturing, setCapturing] = useState(false);
  const [windowLabel, setWindowLabel] = useState("");
  const [confirmClear, setConfirmClear] = useState(false);
  const [preview, setPreview] = useState<{ src: string; alt: string } | null>(null);
  const previewRef = useRef<{ src: string; alt: string } | null>(null);
  const debounceRef = useRef(0);
  const selectedIdsRef = useRef<number[]>([]);
  const captureModeRef = useRef<CaptureMode>("current");
  const captureRef = useRef<CapturePayload | null>(null);
  const pinToBottomRef = useRef(false);
  const bodyRef = useRef<HTMLDivElement>(null);
  const windowListBusy = useRef(false);
  const [stayOpen, setStayOpen] = useState(false);
  const expanded =
    showSettings || log.length > 0 || captureMode === "all" || stayOpen;

  const closeSettings = useCallback(() => {
    showSettingsRef.current = false;
    setShowSettings(false);
  }, []);

  const openSettingsView = useCallback(() => {
    showSettingsRef.current = true;
    setStayOpen(true);
    setShowSettings(true);
  }, []);

  const focusInput = useCallback(() => {
    requestAnimationFrame(() => inputRef.current?.focus());
  }, []);

  const finishCaptureWait = useCallback(() => {
    capturingRef.current = false;
    setCapturing(false);
    captureWaiters.current.splice(0).forEach((resolve) => resolve());
  }, []);

  const waitForInFlightCapture = useCallback(() => {
    if (!capturingRef.current) return Promise.resolve();
    return new Promise<void>((resolve) => captureWaiters.current.push(resolve));
  }, []);

  const beginCaptureWait = useCallback(() => {
    capturingRef.current = true;
    setCapturing(true);
  }, []);

  const resetAsk = useCallback(() => {
    setLog([]);
    setQuery("");
    setError(null);
    setBusy(false);
    setConfirmClear(false);
    setUsedSearch(false);
    setUsedVision(false);
  }, []);

  function startDrag(event: React.MouseEvent) {
    if ((event.target as HTMLElement).closest("button")) return;
    void getCurrentWindow().startDragging();
  }

  function minimizeWindow() {
    void hideOverlay();
  }

  useEffect(() => {
    void setWindowMode(expanded);
  }, [expanded]);

  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];

    void (async () => {
      try {
        const [settings, latest] = await Promise.all([getSettings(), getLatestCapture()]);
        if (cancelled) return;
        setSearchAvailable(settings.webSearchEnabled);
        setSearchOn(settings.webSearchEnabled);
        captureRef.current = latest;
        setCapture(latest);
        setWindowLabel(usableLabel(latest?.mode));
        setCaptureMode("current");
        captureModeRef.current = "current";
        selectedIdsRef.current = [];
        setSelectedIds([]);
        void persistCaptureMode("current", []);
        captureStale.current = !latest;
      } catch (err) {
        if (!cancelled) setError(String(err));
      }

      const add = async <T,>(event: string, handler: (payload: T) => void) => {
        const unlisten = await listen<T>(event, (e) => handler(e.payload));
        if (cancelled) unlisten();
        else unlisteners.push(unlisten);
      };

      await add<CapturePayload>("claire://capture", (payload) => {
        captureStale.current = false;
        captureRef.current = payload;
        setCapture(payload);
        setWindowLabel(usableLabel(payload.mode));
        setError(null);
        finishCaptureWait();
      });
      await add<string>("claire://token", (token) => {
        setLog((current) => {
          if (current.length === 0) return current;
          const next = current.slice();
          const last = next[next.length - 1];
          if (last.role !== "assistant") return current;
          next[next.length - 1] = { ...last, content: last.content + token };
          return next;
        });
      });
      await add<string>("claire://error", (message) => {
        setError(message);
        setBusy(false);
        if (capturingRef.current) finishCaptureWait();
      });
      await add("claire://cleared", () => {
        captureStale.current = true;
        captureRef.current = null;
        setCapture(null);
        setWindowLabel("");
        resetAsk();
        setPreview(null);
      });
      await add("claire://chat-cleared", () => resetAsk());
      await add("claire://settings", () => {
        openSettingsView();
        focusInput();
      });
      await add<string>("claire://summoned", (label) => {
        closeSettings();
        resetAsk();
        setPreview(null);
        setCaptureMode("current");
        captureModeRef.current = "current";
        selectedIdsRef.current = [];
        setSelectedIds([]);
        setStayOpen(false);
        void persistCaptureMode("current", []);
        captureStale.current = true;
        setWindowLabel(label || "");
        beginCaptureWait();
        focusInput();
      });
    })();

    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        if (previewRef.current) {
          previewRef.current = null;
          setPreview(null);
          return;
        }
        if (showSettingsRef.current) {
          closeSettings();
          focusInput();
          return;
        }
        void hideOverlay();
      }
    };
    window.addEventListener("keydown", onKey);
    focusInput();

    return () => {
      cancelled = true;
      unlisteners.forEach((unlisten) => unlisten());
      window.removeEventListener("keydown", onKey);
      window.clearTimeout(debounceRef.current);
    };
  }, [beginCaptureWait, closeSettings, finishCaptureWait, focusInput, openSettingsView, resetAsk]);

  function openPreview(src: string, alt: string) {
    const next = { src, alt };
    previewRef.current = next;
    setPreview(next);
  }

  function closePreview() {
    previewRef.current = null;
    setPreview(null);
  }

  const refreshWindowList = useCallback((quiet = false) => {
    if (windowListBusy.current) return;
    windowListBusy.current = true;
    void listDisplays()
      .then((next) => {
        setDisplays(next);
        const alive = new Set(next.map((display) => display.id));
        const ids = selectedIdsRef.current.filter((id) => alive.has(id));
        if (ids.length !== selectedIdsRef.current.length) {
          selectedIdsRef.current = ids;
          setSelectedIds(ids);
          if (captureModeRef.current === "all") {
            void persistCaptureMode("all", ids);
          }
        }
      })
      .catch((err) => {
        if (!quiet) {
          setDisplays([]);
          setError(String(err));
        }
      })
      .finally(() => {
        windowListBusy.current = false;
      });
  }, []);

  useEffect(() => {
    if (captureMode !== "all" || showSettings) return;
    let cancelled = false;
    const tick = () => {
      void (async () => {
        try {
          if (!(await getCurrentWindow().isVisible())) return;
        } catch {
          /* still refresh */
        }
        if (!cancelled) refreshWindowList(true);
      })();
    };
    tick();
    const timer = window.setInterval(tick, 1500);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [captureMode, showSettings, refreshWindowList]);

  function setMode(next: CaptureMode) {
    if (next === captureModeRef.current) {
      if (next === "all") refreshWindowList();
      return;
    }
    captureModeRef.current = next;
    setCaptureMode(next);
    window.clearTimeout(debounceRef.current);
    if (next === "current") {
      selectedIdsRef.current = [];
      setSelectedIds([]);
      captureStale.current = true;
      void persistCaptureMode("current", []);
      return;
    }
    setStayOpen(true);
    captureStale.current = true;
    void persistCaptureMode("all", selectedIdsRef.current);
    refreshWindowList();
  }

  useLayoutEffect(() => {
    const el = cardRef.current;
    if (!el) return;
    let lastH = 0;
    let fitted = false;
    let cancelled = false;
    const positions = new Map<Element, { top: number; left: number }>();
    const remember = (node: EventTarget | null) => {
      if (!(node instanceof HTMLElement) || node === el) return;
      positions.set(node, { top: node.scrollTop, left: node.scrollLeft });
    };
    const onScroll = (event: Event) => remember(event.target);
    el.addEventListener("scroll", onScroll, true);

    const restore = () => {
      positions.forEach(({ top, left }, node) => {
        if (!(node instanceof HTMLElement) || !node.isConnected) {
          positions.delete(node);
          return;
        }
        node.scrollTop = top;
        node.scrollLeft = left;
      });
    };

    const sectionHeight = (kid: HTMLElement) => {
      if (!kid.classList.contains("overlay-body")) return kid.offsetHeight;
      const cs = getComputedStyle(kid);
      const gap = parseFloat(cs.rowGap || cs.gap) || 0;
      const pad = (parseFloat(cs.paddingTop) || 0) + (parseFloat(cs.paddingBottom) || 0);
      const kids = Array.from(kid.children) as HTMLElement[];
      let height = pad;
      kids.forEach((child, index) => {
        height += child.offsetHeight;
        if (index < kids.length - 1) height += gap;
      });
      return height;
    };

    const naturalHeight = () => {
      const cs = getComputedStyle(el);
      const gap = parseFloat(cs.rowGap || cs.gap) || 0;
      const pad = (parseFloat(cs.paddingTop) || 0) + (parseFloat(cs.paddingBottom) || 0);
      const kids = Array.from(el.children) as HTMLElement[];
      let height = pad;
      kids.forEach((kid, index) => {
        const kcs = getComputedStyle(kid);
        height += (parseFloat(kcs.marginTop) || 0) + (parseFloat(kcs.marginBottom) || 0);
        height += sectionHeight(kid);
        if (index < kids.length - 1) height += gap;
      });
      return Math.ceil(height);
    };

    const apply = () => {
      if (cancelled) return;
      const height = Math.min(Math.max(naturalHeight(), 140), 800);
      if (el.style.height !== `${height}px`) el.style.height = `${height}px`;
      const sameHeight = Math.abs(height - lastH) < 1;
      lastH = height;
      if (sameHeight && fitted) return;
      fitted = false;
      void fitOverlay(880, height)
        .then(() => {
          fitted = true;
        })
        .catch(() => undefined)
        .finally(() => {
          restore();
          requestAnimationFrame(restore);
        });
    };
    const observer = new ResizeObserver(() => apply());
    observer.observe(el);
    el.querySelectorAll(
      ".titlebar, .overlay-body, .overlay-body > *, .composer-block, .meta, .window-picker, .window-list",
    ).forEach((node) => {
      observer.observe(node);
    });
    apply();
    const retry = window.setTimeout(apply, 50);
    return () => {
      cancelled = true;
      observer.disconnect();
      window.clearTimeout(retry);
      el.removeEventListener("scroll", onScroll, true);
      el.style.height = "";
    };
  }, [expanded, showSettings, log.length, captureMode, displays.length, selectedIds.length, capturing]);

  useLayoutEffect(() => {
    const el = bodyRef.current;
    if (!el || !pinToBottomRef.current) return;
    el.scrollTop = el.scrollHeight;
    pinToBottomRef.current = false;
  }, [log, busy]);

  const continuing = log.length > 0;

  async function sendMessage(text: string) {
    pinToBottomRef.current = true;
    setQuery("");
    setBusy(true);
    setError(null);
    setUsedSearch(false);
    setUsedVision(false);
    const attached = captureRef.current;
    setLog((current) => [
      ...current,
      {
        role: "user",
        content: text,
        image: attached?.dataUrl,
        imageAlt: windowLabel || attached?.mode || "Captured window",
      },
      { role: "assistant", content: "" },
    ]);
    try {
      if (capturingRef.current) {
        await waitForInFlightCapture();
      } else if (captureStale.current) {
        await refreshCapture();
      }
      const attached = captureRef.current;
      if (attached?.dataUrl) {
        setLog((current) => {
          const next = current.map((entry) => ({ ...entry }));
          for (let index = next.length - 1; index >= 0; index -= 1) {
            if (next[index].role === "user") {
              next[index] = {
                ...next[index],
                image: attached.dataUrl,
                imageAlt: windowLabel || attached.mode || "Captured window",
              };
              break;
            }
          }
          return next;
        });
      }
      const result = await askClaire(text, searchOn);
      setLog((current) => {
        if (current.length === 0) return current;
        const next = current.slice();
        const last = next[next.length - 1];
        if (last.role === "assistant") {
          next[next.length - 1] = { ...last, content: result.answer };
        }
        return next;
      });
      setUsedSearch(result.usedSearch);
      setUsedVision(result.usedVision);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
      focusInput();
    }
  }

  async function submit() {
    const text = query.trim();
    if (!text || busy) return;
    await sendMessage(text);
  }

  async function startNewChat() {
    if (busy) return;
    try {
      await newChat();
    } catch (err) {
      setError(String(err));
      return;
    }
    setLog([]);
    setUsedSearch(false);
    setUsedVision(false);
    setError(null);
    focusInput();
  }

  async function onCloseSettings() {
    closeSettings();
    try {
      const settings = await getSettings();
      setSearchAvailable(settings.webSearchEnabled);
      setSearchOn(settings.webSearchEnabled);
    } catch {
      /* keep current toggles */
    }
    focusInput();
  }

  async function onClear() {
    try {
      await clearContext();
      setConfirmClear(false);
    } catch (err) {
      setError(String(err));
    }
  }

  async function refreshCapture() {
    window.clearTimeout(debounceRef.current);
    const gen = ++captureGen.current;
    beginCaptureWait();
    try {
      const ids = selectedIdsRef.current;
      const payload =
        captureModeRef.current === "all" && ids.length > 0
          ? await captureDisplays(ids)
          : await recapture();
      if (gen !== captureGen.current) return;
      captureStale.current = false;
      captureRef.current = payload;
      setCapture(payload);
      setWindowLabel(usableLabel(payload.mode));
      setError(null);
    } catch (err) {
      if (gen === captureGen.current) setError(String(err));
    } finally {
      if (gen === captureGen.current) finishCaptureWait();
    }
  }

  function onToggleScreen(id: number) {
    const ids = selectedIdsRef.current;
    const next = ids.includes(id) ? ids.filter((value) => value !== id) : [...ids, id];
    selectedIdsRef.current = next;
    setSelectedIds(next);
    captureModeRef.current = "all";
    setCaptureMode("all");
    captureStale.current = true;
    void persistCaptureMode("all", next);
    refreshWindowList();
    window.clearTimeout(debounceRef.current);
    debounceRef.current = window.setTimeout(() => {
      void refreshCapture();
    }, 250);
  }

  return (
    <div className="overlay-shell">
      <div
        className={["overlay-card", showSettings ? "settings-open" : "", expanded ? "expanded" : "compact"].join(" ")}
        ref={cardRef}
      >
        <div
          className="titlebar"
          data-tauri-drag-region
          onMouseDown={startDrag}
        >
          <span className="brand titlebar-drag" data-tauri-drag-region>
            cl<span>AI</span>re
          </span>
          <div className="titlebar-actions">
            <button
              className={!showSettings && captureMode === "current" ? "chip on" : "chip"}
              type="button"
              onClick={() => {
                closeSettings();
                setMode("current");
              }}
            >
              {expanded ? "Current window" : "Current"}
            </button>
            <button
              className={!showSettings && captureMode === "all" ? "chip on" : "chip"}
              type="button"
              onClick={() => {
                closeSettings();
                setMode("all");
              }}
            >
              {expanded ? "All windows" : "Windows"}
            </button>
            {searchAvailable && (
              <button
                className={searchOn ? "chip on" : "chip"}
                onClick={() => setSearchOn((value) => !value)}
                type="button"
              >
                Web
              </button>
            )}
            <button
              className={showSettings ? "chip on" : "ghost"}
              type="button"
              onClick={openSettingsView}
            >
              Settings
            </button>
            {confirmClear ? (
              <>
                <span className="clear-confirm">Clear chat and context?</span>
                <button className="danger" type="button" onClick={() => void onClear()}>
                  Clear
                </button>
                <button className="ghost" type="button" onClick={() => setConfirmClear(false)}>
                  Cancel
                </button>
              </>
            ) : (
              <button
                className="danger"
                type="button"
                disabled={busy || (log.length === 0 && !capture)}
                onClick={() => setConfirmClear(true)}
              >
                Clear
              </button>
            )}
          </div>
          <button
            className="ghost window-btn"
            type="button"
            title="Hide"
            aria-label="Hide"
            onClick={() => void minimizeWindow()}
          >
            –
          </button>
        </div>
        {showSettings ? (
          <div className="overlay-body">
            <SettingsPage onClose={() => void onCloseSettings()} />
          </div>
        ) : (
          <>
        {expanded && (
        <div className="overlay-body" ref={bodyRef}>
        {captureMode === "current" && (
          <section className="current-panel">
            {capture ? (
              <button
                className={capturing ? "current-shot capturing" : "current-shot"}
                type="button"
                title="Recapture"
                onClick={() => void refreshCapture()}
              >
                <img src={capture.dataUrl} alt={windowLabel || "Current window"} />
              </button>
            ) : (
              <div className={capturing ? "current-shot placeholder capturing" : "current-shot placeholder"} />
            )}
            <div className="current-meta">
              <strong title={windowLabel || undefined}>{windowLabel || "Current window"}</strong>
              <span>
                {capturing
                  ? windowLabel
                    ? "This window will be sent with your question"
                    : "Finding window…"
                  : capture
                    ? `${capture.width}×${capture.height} will be sent with your question`
                    : "No window yet"}
              </span>
            </div>
          </section>
        )}

        {captureMode === "all" && (
          <section className="window-panel">
            <div className="window-panel-head">
              <span>Windows</span>
              <span>{selectedIds.length} selected</span>
            </div>
            <p className="hint">Click a window to add it. Use × to remove it from the capture.</p>
            <DisplayPicker
              displays={displays}
              selectedIds={selectedIds}
              onToggle={(id) => void onToggleScreen(id)}
            />
          </section>
        )}

        {error && <div className="banner error">{error}</div>}

        {log.length > 0 && (
          <section className="chat-log" aria-live="polite">
            <div className="thread-label">This chat</div>
            {log.map((entry, index) => {
              const lastAssistant = entry.role === "assistant" && index === log.length - 1;
              return (
                <article
                  key={`${entry.role}-${index}`}
                  className={entry.role === "user" ? "turn user" : "turn assistant"}
                >
                  {entry.role === "assistant" && (
                    <div className="turn-avatar" aria-hidden>
                      AI
                    </div>
                  )}
                  <div className="turn-body">
                    {entry.image && (
                      <button
                        className="log-shot"
                        type="button"
                        title="Open screenshot"
                        onClick={() => openPreview(entry.image!, entry.imageAlt || "Captured window")}
                      >
                        <img src={entry.image} alt={entry.imageAlt || "Captured window"} />
                      </button>
                    )}
                    {entry.content ? (
                      <div
                        className="answer-body"
                        dangerouslySetInnerHTML={{
                          __html: renderLiteMarkdown(entry.content),
                        }}
                      />
                    ) : (
                      <div className="typing" aria-label="Generating">
                        <span />
                        <span />
                        <span />
                      </div>
                    )}
                    {lastAssistant && (usedVision || usedSearch) && (
                      <div className="turn-tags">
                        {usedVision && <em>vision</em>}
                        {usedSearch && <em>web</em>}
                      </div>
                    )}
                  </div>
                </article>
              );
            })}
          </section>
        )}
        </div>
        )}

        {!expanded && error && <div className="banner error">{error}</div>}

        <div className={continuing ? "composer-block continuing" : "composer-block fresh"}>
          <div className="thread-bar">
            {continuing ? (
            <>
              <span className="thread-status continue">Continuing this chat</span>
              <button className="ghost" type="button" disabled={busy} onClick={() => void startNewChat()}>
                New chat
              </button>
            </>
          ) : (
            <span className="thread-status fresh">New chat</span>
          )}
        </div>
        <div className="composer">
          <textarea
            ref={inputRef}
            value={query}
            placeholder={
              continuing
                ? "Ask a follow-up in this chat…"
                : capture
                  ? "Start a new chat about this window…"
                  : "Start a new chat…"
            }
            rows={expanded ? 2 : 1}
            disabled={busy}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                void submit();
              }
            }}
          />
          <button
            className="primary send"
            type="button"
            disabled={busy || !query.trim()}
            onClick={() => void submit()}
          >
            {busy ? "…" : continuing ? "Continue" : "Ask"}
          </button>
        </div>
        </div>

        <div className="meta">
          <span className="meta-context" title={windowLabel || undefined}>
            <span className="meta-window">
              {capturing
                ? windowLabel
                  ? `Sending ${windowLabel}`
                  : "Capturing…"
                : windowLabel || (capture ? "Screen context" : "No window context yet")}
            </span>
            {!capturing && capture && (
              <span className="meta-size">
                {capture.width}×{capture.height}
              </span>
            )}
          </span>
          <span>
            {continuing
              ? "Continue sends a follow-up · New chat starts over"
              : "Ask starts a new chat · Esc hides"}
          </span>
        </div>
          </>
        )}
      </div>
      {preview && (
        <button className="lightbox" type="button" onClick={closePreview} aria-label="Close screenshot">
          <img src={preview.src} alt={preview.alt} />
        </button>
      )}
    </div>
  );
}
