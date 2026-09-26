import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type SetStateAction } from "react";
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
  openUrl,
  previewWindows,
  recapture,
  setCaptureMode as persistCaptureMode,
  setWindowMode,
} from "../shared/api";
import { followAnswer, pinTurn, useOverlayFit } from "./useOverlayFit";
import DisplayPicker, { importantWindows, quarterSize } from "../shared/DisplayPicker";
import SettingsPage from "../settings/Settings";
import { renderLiteMarkdown } from "../shared/markdown";
import type { AskStatus, CaptureMode, CapturePayload, DisplayInfo, SearchSource, WindowPreview } from "../shared/types";

function usableLabel(mode?: string | null) {
  if (!mode || mode === "current window") return "";
  return mode;
}

function stabilizeDisplays(prev: DisplayInfo[], next: DisplayInfo[]) {
  if (prev.length === 0) return next;
  const byId = new Map(next.map((display) => [display.id, display]));
  const seen = new Set<number>();
  const out: DisplayInfo[] = [];
  for (const item of prev) {
    const fresh = byId.get(item.id);
    if (!fresh || seen.has(item.id)) continue;
    out.push(fresh);
    seen.add(item.id);
  }
  for (const item of next) {
    if (seen.has(item.id)) continue;
    out.push(item);
    seen.add(item.id);
  }
  return out;
}

type LogEntry = {
  role: "user" | "assistant";
  content: string;
  image?: string;
  imageAlt?: string;
};

const EMPTY_SOURCES: SearchSource[] = [];

function useMirrored<T>(initial: T) {
  const [value, setValue] = useState(initial);
  const ref = useRef(initial);
  const set = useCallback((action: SetStateAction<T>) => {
    const next = typeof action === "function" ? (action as (value: T) => T)(ref.current) : action;
    ref.current = next;
    setValue(next);
  }, []);
  return [value, set, ref] as const;
}

function setFenceCopied(button: HTMLElement, copied: boolean) {
  button.classList.toggle("is-copied", copied);
  const label = copied ? "Copied" : "Copy";
  button.textContent = label;
  button.title = label;
  button.setAttribute("aria-label", copied ? "Copied" : "Copy code");
}

async function writeClipboard(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.setAttribute("readonly", "");
    ta.style.position = "fixed";
    ta.style.left = "-9999px";
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    ta.remove();
  }
}

type ShotAnchor = { top: number; left: number; right: number; bottom: number };

type ShotPop = {
  live: boolean;
  src: string;
  alt: string;
  anchor: ShotAnchor;
};

const SHOT_POP_MAX_W = 520;
const SHOT_POP_MAX_H = 360;
const SHOT_POP_GAP = 8;

function ShotHover({ src, alt, anchor }: { src: string; alt: string; anchor: ShotAnchor }) {
  const { box, maxW, maxH } = shotPopLayout(anchor);
  return (
    <div className="shot-pop" style={box} role="tooltip">
      <img src={src} alt={alt} style={{ maxWidth: maxW, maxHeight: maxH }} />
    </div>
  );
}

function anchorOf(el: HTMLElement): ShotAnchor {
  const rect = el.getBoundingClientRect();
  return { top: rect.top, left: rect.left, right: rect.right, bottom: rect.bottom };
}

function shotPopLayout(anchor: ShotAnchor) {
  const margin = 8;
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const maxW = Math.min(SHOT_POP_MAX_W, Math.max(120, vw - margin * 2));
  const spaceAbove = Math.max(0, anchor.top - margin - SHOT_POP_GAP);
  const spaceBelow = Math.max(0, vh - anchor.bottom - margin - SHOT_POP_GAP);
  const above = spaceAbove >= spaceBelow;
  const maxH = Math.min(SHOT_POP_MAX_H, above ? spaceAbove : spaceBelow);
  const left = Math.min(Math.max(anchor.left, margin), Math.max(margin, vw - maxW - margin));
  const box: React.CSSProperties = above
    ? { left, bottom: vh - anchor.top + SHOT_POP_GAP, maxWidth: maxW }
    : { left, top: anchor.bottom + SHOT_POP_GAP, maxWidth: maxW };
  return { box, maxW, maxH };
}

type TurnChrome = {
  clampQuery: boolean;
  queryCap: number;
  queryExpanded: boolean;
  showClamp: boolean;
  copied: boolean;
  busy: boolean;
  askStatus: AskStatus | null;
  usedVision: boolean;
  usedSearch: boolean;
  usedSearchApi: string;
  searchSources: SearchSource[];
};

type ChatTurnProps = {
  entry: LogEntry;
  index: number;
  isLastAssistant: boolean;
  isFirstUser: boolean;
  isLatestUser: boolean;
  chrome?: TurnChrome;
  latestUserRef: React.MutableRefObject<HTMLElement | null>;
  latestAssistantRef: React.MutableRefObject<HTMLElement | null>;
  onOpenPreview: (src: string, alt: string) => void;
  onHoverShot: (live: boolean, src: string, alt: string, el: HTMLElement) => void;
  onHideShot: () => void;
  onToggleQuery: () => void;
  onCopyOutput: (index: number, text: string) => void;
  onAnswerClick: (event: React.MouseEvent<HTMLDivElement>) => void;
};

const ChatTurn = memo(function ChatTurn({
  entry,
  index,
  isLastAssistant,
  isFirstUser,
  isLatestUser,
  chrome,
  latestUserRef,
  latestAssistantRef,
  onOpenPreview,
  onHoverShot,
  onHideShot,
  onToggleQuery,
  onCopyOutput,
  onAnswerClick,
}: ChatTurnProps) {
  const {
    clampQuery = false,
    queryCap = 0,
    queryExpanded = false,
    showClamp = false,
    copied = false,
    busy = false,
    askStatus = null,
    usedVision = false,
    usedSearch = false,
    usedSearchApi = "",
    searchSources = EMPTY_SOURCES,
  } = chrome ?? {};
  const html = useMemo(() => (entry.content ? renderLiteMarkdown(entry.content) : ""), [entry.content]);
  return (
    <article
      className={entry.role === "user" ? "turn user" : "turn assistant"}
      ref={(node) => {
        if (isLatestUser) latestUserRef.current = node;
        else if (latestUserRef.current === node) latestUserRef.current = null;
        if (isLastAssistant) latestAssistantRef.current = node;
        else if (latestAssistantRef.current === node) latestAssistantRef.current = null;
      }}
    >
      {entry.role === "assistant" && (
        <div className="turn-avatar" aria-hidden>
          AI
        </div>
      )}
      <div className="turn-body">
        {isFirstUser && entry.image && (
          <button
            className="log-shot"
            type="button"
            title="Open screenshot"
            onMouseEnter={(event) =>
              onHoverShot(false, entry.image!, entry.imageAlt || "Captured window", event.currentTarget)
            }
            onMouseLeave={onHideShot}
            onFocus={(event) =>
              onHoverShot(false, entry.image!, entry.imageAlt || "Captured window", event.currentTarget)
            }
            onBlur={onHideShot}
            onClick={() => onOpenPreview(entry.image!, entry.imageAlt || "Captured window")}
          >
            <img src={entry.image} alt={entry.imageAlt || "Captured window"} />
          </button>
        )}
        {html ? (
          entry.role === "assistant" ? (
            <div className="assistant-output">
              <div className="answer-body" onClick={onAnswerClick} dangerouslySetInnerHTML={{ __html: html }} />
              <button
                className="copy-output"
                type="button"
                title={copied ? "Copied" : "Copy response"}
                aria-label={copied ? "Copied" : "Copy response"}
                onClick={() => void onCopyOutput(index, entry.content)}
              >
                {copied ? (
                  <svg viewBox="0 0 24 24" aria-hidden>
                    <path d="M20 6 9 17l-5-5" />
                  </svg>
                ) : (
                  <svg viewBox="0 0 24 24" aria-hidden>
                    <rect x="9" y="9" width="13" height="13" rx="2" />
                    <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
                  </svg>
                )}
              </button>
            </div>
          ) : (
            <div
              className={clampQuery ? "answer-body clamped" : "answer-body"}
              style={clampQuery ? { maxHeight: queryCap } : undefined}
              onClick={onAnswerClick}
              dangerouslySetInnerHTML={{ __html: html }}
            />
          )
        ) : null}
        {showClamp && (
          <button className="show-more" type="button" onClick={onToggleQuery}>
            {queryExpanded ? "Show less" : "Show full question"}
          </button>
        )}
        {busy && isLastAssistant && (
          <div className="ask-status" aria-live="polite">
            <span className="ask-status-orb" />
            <strong>{askStatus?.api || "clAIre"}</strong>
            <span>{askStatus?.detail || (entry.content ? "streaming" : "thinking")}</span>
            <div className="typing" aria-hidden="true">
              <span />
              <span />
              <span />
            </div>
          </div>
        )}
        {isLastAssistant && (usedVision || usedSearch) && (
          <div className="turn-tags">
            {usedVision && <em>vision</em>}
            {usedSearch && <em>{usedSearchApi || "web"}</em>}
          </div>
        )}
        {isLastAssistant && usedSearch && searchSources.length > 0 && (
          <ol className="search-cites">
            {searchSources.map((source) => (
              <li key={`${source.index}-${source.url}`}>
                <button type="button" className="cite-link" title={source.url} onClick={() => void openUrl(source.url)}>
                  {source.title}
                </button>
              </li>
            ))}
          </ol>
        )}
      </div>
    </article>
  );
});

export default function Overlay() {
  const [query, setQuery] = useState("");
  const [log, setLog] = useState<LogEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [capture, setCapture] = useState<CapturePayload | null>(null);
  const [searchOn, setSearchOn] = useState(false);
  const [searchAvailable, setSearchAvailable] = useState(false);
  const [usedSearch, setUsedSearch] = useState(false);
  const [usedSearchApi, setUsedSearchApi] = useState("");
  const [searchSources, setSearchSources] = useState<SearchSource[]>([]);
  const [usedVision, setUsedVision] = useState(false);
  const [askStatus, setAskStatus] = useState<AskStatus | null>(null);
  const [captureMode, setCaptureMode, captureModeRef] = useMirrored<CaptureMode>("current");
  const [displays, setDisplays] = useState<DisplayInfo[]>([]);
  const [previews, setPreviews] = useState<Record<number, WindowPreview>>({});
  const [selectedIds, setSelectedIds, selectedIdsRef] = useMirrored<number[]>([]);
  const [showSettings, setShowSettings, showSettingsRef] = useMirrored(false);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const cardRef = useRef<HTMLDivElement>(null);
  const captureGen = useRef(0);
  const captureStale = useRef(true);
  const capturingRef = useRef(false);
  const captureWaiters = useRef<Array<() => void>>([]);
  const [capturing, setCapturing] = useState(false);
  const [windowLabel, setWindowLabel] = useState("");
  const [confirmClear, setConfirmClear] = useState(false);
  const [passwordNotice, setPasswordNotice, passwordNoticeRef] = useMirrored(false);
  const [preview, setPreview, previewRef] = useMirrored<{ src: string; alt: string } | null>(null);
  const [shotPop, setShotPop, shotPopRef] = useMirrored<ShotPop | null>(null);
  const debounceRef = useRef(0);
  const captureRef = useRef<CapturePayload | null>(null);
  const pinWindowIdRef = useRef<number | null>(null);
  const lastCurrentIdRef = useRef<number | null>(null);
  const [lastCurrentId, setLastCurrentId] = useState<number | null>(null);
  const [singleTargetId, setSingleTargetId] = useState<number | null>(null);
  const singleTargetIdRef = useRef<number | null>(null);
  const [thumbStale, setThumbStale] = useState(false);
  const [tileOrder, setTileOrder] = useState<number[]>([]);
  const [previewsPending, setPreviewsPending] = useState(false);
  const previewGen = useRef(0);
  const pinQueryRef = useRef(false);
  const followRef = useRef(false);
  const ignoreScrollRef = useRef(false);
  const latestUserRef = useRef<HTMLElement | null>(null);
  const latestAssistantRef = useRef<HTMLElement | null>(null);
  const chatLogRef = useRef<HTMLElement | null>(null);
  const scrollerWrapRef = useRef<HTMLDivElement>(null);
  const windowListBusy = useRef(false);
  const [overlayFocused, setOverlayFocused] = useMirrored(false);
  const previewBusy = useRef(false);
  const tokenBuf = useRef("");
  const tokenTimer = useRef(0);
  const tokenGen = useRef(0);
  const applyFitRef = useRef<() => void>(() => {});
  const [stayOpen, setStayOpen] = useState(false);
  const [showJump, setShowJump] = useState(false);
  const [queryExpanded, setQueryExpanded] = useState(false);
  const [queryNeedsClamp, setQueryNeedsClamp] = useState(false);
  const [queryCap, setQueryCap] = useState(120);
  const [copiedIndex, setCopiedIndex] = useState<number | null>(null);
  const copiedTimer = useRef(0);
  const copiedSnippetTimer = useRef(0);
  const copiedFenceBtn = useRef<HTMLElement | null>(null);
  const expanded = showSettings || log.length > 0 || stayOpen;

  const closeSettings = useCallback(() => {
    setShowSettings(false);
  }, [setShowSettings]);

  const openSettingsView = useCallback(() => {
    setStayOpen(true);
    setShowSettings(true);
  }, [setShowSettings]);

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

  const dropTokens = useCallback(() => {
    tokenGen.current += 1;
    tokenBuf.current = "";
    if (tokenTimer.current) {
      window.clearTimeout(tokenTimer.current);
      tokenTimer.current = 0;
    }
  }, []);

  const clearTurnMeta = useCallback(() => {
    dropTokens();
    followRef.current = false;
    pinQueryRef.current = false;
    setShowJump(false);
    scrollerWrapRef.current?.classList.remove("fade-top", "fade-bottom");
    setQueryExpanded(false);
    setQueryNeedsClamp(false);
    setError(null);
    setUsedSearch(false);
    setUsedSearchApi("");
    setSearchSources([]);
    setUsedVision(false);
    setAskStatus(null);
    window.clearTimeout(copiedSnippetTimer.current);
    copiedFenceBtn.current = null;
  }, [dropTokens]);

  const resetAsk = useCallback(() => {
    clearTurnMeta();
    setLog([]);
    setQuery("");
    setBusy(false);
    setConfirmClear(false);
  }, [clearTurnMeta]);

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
        setSelectedIds([]);
        pinWindowIdRef.current = null;
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
        if (captureModeRef.current === "none") {
          finishCaptureWait();
          return;
        }
        captureStale.current = false;
        captureRef.current = payload;
        setCapture(payload);
        setThumbStale(false);
        setWindowLabel(usableLabel(payload.mode));
        setError(null);
        finishCaptureWait();
      });
      await add<string>("claire://token", (token) => {
        tokenBuf.current += token;
        if (tokenTimer.current) return;
        const gen = tokenGen.current;
        tokenTimer.current = window.setTimeout(() => {
          tokenTimer.current = 0;
          if (gen !== tokenGen.current) return;
          const chunk = tokenBuf.current;
          tokenBuf.current = "";
          if (!chunk) return;
          setLog((current) => {
            if (current.length === 0) return current;
            const next = current.slice();
            const last = next[next.length - 1];
            if (last.role !== "assistant") return current;
            next[next.length - 1] = { ...last, content: last.content + chunk };
            return next;
          });
        }, 40);
      });
      await add<AskStatus>("claire://ask-status", (status) => {
        setAskStatus(status.phase === "idle" ? null : status);
      });
      await add<string>("claire://error", (message) => {
        dropTokens();
        setError(message);
        setBusy(false);
        if (capturingRef.current) finishCaptureWait();
      });
      await add("claire://cleared", () => {
        captureStale.current = true;
        captureRef.current = null;
        setCapture(null);
        setThumbStale(false);
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
        setSelectedIds([]);
        pinWindowIdRef.current = null;
        setStayOpen(false);
        void persistCaptureMode("current", []);
        captureStale.current = true;
        setWindowLabel(label || "");
        beginCaptureWait();
        focusInput();
      });
      await add<{ id?: number | null; label?: string; recapturing?: boolean }>("claire://target", (hint) => {
        if (captureModeRef.current !== "current") return;
        if (hint.label) setWindowLabel(hint.label);
        if (typeof hint.id === "number") {
          const prev = singleTargetIdRef.current;
          const changed = prev !== hint.id;
          singleTargetIdRef.current = hint.id;
          setSingleTargetId(hint.id);
          if (changed && (prev != null || hint.recapturing)) {
            captureGen.current += 1;
            setThumbStale(true);
          }
        }
        captureStale.current = true;
        if (hint.recapturing) beginCaptureWait();
      });
    })();

    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        if (previewRef.current) {
          setPreview(null);
          return;
        }
        if (shotPopRef.current) {
          setShotPop(null);
          return;
        }
        if (showSettingsRef.current) {
          closeSettings();
          focusInput();
          return;
        }
        if (passwordNoticeRef.current) {
          setPasswordNotice(false);
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
      dropTokens();
      unlisteners.forEach((unlisten) => unlisten());
      window.removeEventListener("keydown", onKey);
      window.clearTimeout(debounceRef.current);
      window.clearTimeout(copiedTimer.current);
      window.clearTimeout(copiedSnippetTimer.current);
    };
  }, [beginCaptureWait, captureModeRef, closeSettings, dropTokens, finishCaptureWait, focusInput, openSettingsView, passwordNoticeRef, previewRef, resetAsk, setCaptureMode, setPasswordNotice, setPreview, setSelectedIds, setShotPop, shotPopRef, showSettingsRef]);

  function closePreview() {
    setPreview(null);
  }

  const refreshWindowList = useCallback((quiet = false) => {
    if (windowListBusy.current) return;
    windowListBusy.current = true;
    void listDisplays()
      .then((next) => {
        setDisplays((prev) => stabilizeDisplays(prev, next));
        const alive = new Set(next.map((display) => display.id));
        const ids = selectedIdsRef.current.filter((id) => alive.has(id));
        if (ids.length !== selectedIdsRef.current.length) {
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
  }, [captureModeRef, selectedIdsRef, setSelectedIds]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    const apply = (focused: boolean) => {
      setOverlayFocused(focused);
    };
    void getCurrentWindow()
      .onFocusChanged((event) => apply(event.payload))
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch(() => undefined);
    void getCurrentWindow()
      .isFocused()
      .then((focused) => {
        if (!cancelled) apply(focused);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [setOverlayFocused]);

  useEffect(() => {
    if (showSettings || captureMode === "none") return;
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
  }, [showSettings, captureMode, refreshWindowList]);

  function setMode(next: CaptureMode) {
    if (next === captureModeRef.current) {
      if (next === "all") refreshWindowList();
      return;
    }
    setCaptureMode(next);
    window.clearTimeout(debounceRef.current);
    if (next === "none") {
      captureGen.current += 1;
      setSelectedIds([]);
      pinWindowIdRef.current = null;
      singleTargetIdRef.current = null;
      setSingleTargetId(null);
      setThumbStale(false);
      captureStale.current = false;
      captureRef.current = null;
      setCapture(null);
      setWindowLabel("");
      finishCaptureWait();
      void persistCaptureMode("none", []);
      return;
    }
    if (next === "current") {
      const picked = selectedIdsRef.current;
      const specific = picked.length === 1 ? picked[0] : null;
      const targetId = specific ?? lastCurrentIdRef.current;
      if (specific != null) pinWindowIdRef.current = specific;
      singleTargetIdRef.current = targetId;
      setSingleTargetId(targetId);
      setThumbStale(false);
      const previewReady = targetId != null && !!previews[targetId]?.dataUrl;
      const kept =
        specific == null &&
        !!captureRef.current?.dataUrl &&
        captureRef.current.mode !== "selected windows";
      setSelectedIds([]);
      if (!kept) {
        captureStale.current = !previewReady;
        captureRef.current = null;
        setCapture(null);
      } else {
        captureStale.current = false;
      }
      const named =
        (targetId != null && displays.find((display) => display.id === targetId)?.name) ||
        (targetId != null && previews[targetId]?.name) ||
        "";
      if (named) setWindowLabel(named);
      else if (!kept && !previewReady) setWindowLabel("");
      void persistCaptureMode("current", []);
      if (!kept && !previewReady) void refreshCapture();
      return;
    }
    captureStale.current = true;
    setTileOrder(importantWindows(displays).map((item) => item.id));
    void persistCaptureMode("all", selectedIdsRef.current);
    refreshWindowList();
  }

  useOverlayFit({
    cardRef,
    chatLogRef,
    latestUserRef,
    latestAssistantRef,
    pinQueryRef,
    followRef,
    ignoreScrollRef,
    applyFitRef,
    expanded,
    showSettings,
    captureMode,
    displaysLength: displays.length,
    selectedCount: selectedIds.length,
    capturing,
    queryNeedsClamp,
    queryExpanded,
  });

  const continuing = log.length > 0;
  const firstUserIndex = log.findIndex((item) => item.role === "user");
  let latestUserIndex = -1;
  for (let i = log.length - 1; i >= 0; i -= 1) {
    if (log[i].role === "user") {
      latestUserIndex = i;
      break;
    }
  }

  const updateChatChrome = useCallback(() => {
    const scroller = chatLogRef.current;
    const wrap = scrollerWrapRef.current;
    if (!scroller || !wrap) {
      setShowJump(false);
      return;
    }
    const top = scroller.scrollTop > 6;
    const bottom = scroller.scrollTop + scroller.clientHeight < scroller.scrollHeight - 6;
    wrap.classList.toggle("fade-top", top);
    wrap.classList.toggle("fade-bottom", bottom);
    const nextJump = !followRef.current && bottom;
    setShowJump((current) => (current === nextJump ? current : nextJump));
  }, []);

  const alignThread = useCallback(
    (mode: "pin" | "follow") => {
      const scroller = chatLogRef.current;
      if (!scroller) return;
      ignoreScrollRef.current = true;
      if (mode === "pin") {
        const turn = latestUserRef.current;
        if (turn) pinTurn(scroller, turn);
      }
      const answer = latestAssistantRef.current;
      if (answer && followRef.current) followAnswer(scroller, answer);
      requestAnimationFrame(() => {
        ignoreScrollRef.current = false;
        updateChatChrome();
      });
    },
    [updateChatChrome],
  );

  useLayoutEffect(() => {
    const scroller = chatLogRef.current;
    const body = latestUserRef.current?.querySelector(".answer-body") as HTMLElement | null;
    if (!scroller || !body) {
      setQueryNeedsClamp(false);
      return;
    }
    const cap = Math.max(88, scroller.clientHeight * 0.3);
    setQueryCap(cap);
    body.style.maxHeight = "none";
    const overflows = body.scrollHeight > cap + 4;
    body.style.maxHeight = "";
    setQueryNeedsClamp(overflows);
  }, [log, queryExpanded, continuing]);

  useLayoutEffect(() => {
    if (pinQueryRef.current) {
      alignThread("pin");
      const first = log.find((entry) => entry.role === "user");
      const waitingForShot =
        capturing ||
        Boolean(captureRef.current && first && !first.image && firstUserIndex === latestUserIndex);
      if (!waitingForShot) pinQueryRef.current = false;
      return;
    }
    if (followRef.current) alignThread("follow");
    else updateChatChrome();
  }, [log, capturing, firstUserIndex, latestUserIndex, alignThread, updateChatChrome]);

  useEffect(() => {
    const card = cardRef.current;
    if (!card || !continuing) return;
    const onWheel = (event: WheelEvent) => {
      const scroller = chatLogRef.current;
      if (!scroller || event.deltaY === 0) return;
      const target = event.target as HTMLElement | null;
      if (target?.closest(".lightbox") || target?.closest(".chat-log") === scroller) return;
      scroller.scrollTop += event.deltaY;
      event.preventDefault();
    };
    card.addEventListener("wheel", onWheel, { passive: false });
    return () => card.removeEventListener("wheel", onWheel);
  }, [continuing]);

  function onChatScroll() {
    if (shotPopRef.current) {
      setShotPop(null);
    }
    if (ignoreScrollRef.current) {
      updateChatChrome();
      return;
    }
    followRef.current = false;
    pinQueryRef.current = false;
    updateChatChrome();
  }

  function jumpToLatest() {
    followRef.current = true;
    pinQueryRef.current = true;
    setShowJump(false);
    alignThread("pin");
  }

  const copyOutput = useCallback(async (index: number, text: string) => {
    await writeClipboard(text);
    setCopiedIndex(index);
    window.clearTimeout(copiedTimer.current);
    copiedTimer.current = window.setTimeout(() => setCopiedIndex(null), 1500);
  }, []);

  const onAnswerClick = useCallback((event: React.MouseEvent<HTMLDivElement>) => {
    const target = event.target as HTMLElement;
    const copyBtn = target.closest<HTMLElement>(".copy-code");
    if (copyBtn) {
      event.preventDefault();
      const text = copyBtn.closest(".md-code")?.querySelector("code")?.textContent ?? "";
      void (async () => {
        await writeClipboard(text);
        const prev = copiedFenceBtn.current;
        if (prev && prev !== copyBtn && prev.isConnected) setFenceCopied(prev, false);
        setFenceCopied(copyBtn, true);
        copiedFenceBtn.current = copyBtn;
        window.clearTimeout(copiedSnippetTimer.current);
        copiedSnippetTimer.current = window.setTimeout(() => {
          const button = copiedFenceBtn.current;
          copiedFenceBtn.current = null;
          if (button?.isConnected) setFenceCopied(button, false);
        }, 1500);
      })();
      return;
    }
    const link = target.closest("a");
    if (!link) return;
    event.preventDefault();
    const href = link.getAttribute("href");
    if (href) void openUrl(href);
  }, []);

  const openPreview = useCallback((src: string, alt: string) => {
    setPreview({ src, alt });
    setShotPop(null);
  }, [setPreview, setShotPop]);

  const showShotPop = useCallback((live: boolean, src: string, alt: string, el: HTMLElement) => {
    setShotPop({ live, src, alt, anchor: anchorOf(el) });
  }, [setShotPop]);

  const hideShotPop = useCallback(() => {
    if (!shotPopRef.current) return;
    setShotPop(null);
  }, [setShotPop, shotPopRef]);

  const toggleQueryExpanded = useCallback(() => {
    setQueryExpanded((open) => !open);
  }, []);

  async function sendMessage(text: string) {
    pinQueryRef.current = true;
    followRef.current = true;
    setShowJump(false);
    setQueryExpanded(false);
    setQuery("");
    setBusy(true);
    setError(null);
    setUsedSearch(false);
    setUsedSearchApi("");
    setSearchSources([]);
    setUsedVision(false);
    setAskStatus({ phase: "llm", api: "clAIre", detail: "starting" });
    setLog((current) => [
      ...current,
      { role: "user", content: text },
      { role: "assistant", content: "" },
    ]);
    try {
      if (captureModeRef.current !== "none" && capturingRef.current) {
        await waitForInFlightCapture();
      }
      if (
        captureModeRef.current !== "none" &&
        (captureStale.current ||
          (captureModeRef.current === "current" && captureRef.current?.mode === "selected windows"))
      ) {
        await refreshCapture();
      }
      const attached = captureModeRef.current === "none" ? null : captureRef.current;
      if (attached?.dataUrl && attached.passwordsBlurred) {
        setPasswordNotice(true);
      }
      if (attached?.dataUrl) {
        setLog((current) => {
          const firstUser = current.findIndex((entry) => entry.role === "user");
          if (firstUser < 0) return current;
          const followUp = current.some((entry, index) => entry.role === "user" && index !== firstUser);
          if (followUp || current[firstUser].image) return current;
          const next = current.slice();
          next[firstUser] = {
            ...current[firstUser],
            image: attached.dataUrl,
            imageAlt: windowLabel || attached.mode || "Captured window",
          };
          return next;
        });
      }
      const result = await askClaire(text, searchOn);
      dropTokens();
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
      setUsedSearchApi(result.searchProvider || "");
      setSearchSources(result.searchSources || []);
      setUsedVision(result.usedVision);
    } catch (err) {
      dropTokens();
      setError(String(err));
    } finally {
      setBusy(false);
      setAskStatus(null);
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
    clearTurnMeta();
    focusInput();
  }

  async function onCloseSettings() {
    closeSettings();
    try {
      const settings = await getSettings();
      const enabled = settings.webSearchEnabled;
      setSearchOn((on) => (enabled ? (searchAvailable ? on : true) : false));
      setSearchAvailable(enabled);
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

  const refreshCapture = useCallback(
    async (forceCurrent = false) => {
      if (captureModeRef.current === "none") {
        finishCaptureWait();
        return;
      }
      window.clearTimeout(debounceRef.current);
      const gen = ++captureGen.current;
      beginCaptureWait();
      try {
        const ids = selectedIdsRef.current;
        const useAll = !forceCurrent && captureModeRef.current === "all" && ids.length > 0;
        const pinId = useAll || forceCurrent ? undefined : pinWindowIdRef.current ?? undefined;
        if (!useAll) pinWindowIdRef.current = null;
        const payload = useAll
          ? await captureDisplays(ids)
          : await recapture(pinId, forceCurrent);
        const modeAfterCapture = captureModeRef.current as CaptureMode;
        if (gen !== captureGen.current || modeAfterCapture === "none") return;
        captureStale.current = false;
        captureRef.current = payload;
        setCapture(payload);
        setThumbStale(false);
        setWindowLabel(usableLabel(payload.mode));
        setError(null);
      } catch (err) {
        if (gen === captureGen.current) setError(String(err));
      } finally {
        if (gen === captureGen.current) finishCaptureWait();
      }
    },
    [beginCaptureWait, captureModeRef, finishCaptureWait, selectedIdsRef],
  );

  useEffect(() => {
    if (!overlayFocused || showSettings) return;
    const timer = window.setTimeout(() => {
      if (captureModeRef.current === "none" || capturingRef.current) return;
      void refreshCapture();
    }, 200);
    return () => window.clearTimeout(timer);
  }, [captureModeRef, overlayFocused, showSettings, refreshCapture]);

  const resolvedTileOrder = useMemo(() => {
    if (captureMode !== "all" || displays.length === 0) return tileOrder;
    if (tileOrder.length === 0) return importantWindows(displays).map((item) => item.id);
    const alive = new Set(displays.map((item) => item.id));
    const kept = tileOrder.filter((id) => alive.has(id));
    return kept.length === tileOrder.length ? tileOrder : kept;
  }, [captureMode, displays, tileOrder]);

  const shownDisplays = useMemo(() => {
    if (captureMode !== "all") return [];
    const byId = new Map(displays.map((item) => [item.id, item]));
    const locked = resolvedTileOrder
      .map((id) => byId.get(id))
      .filter((item): item is DisplayInfo => item != null);
    if (locked.length > 0) return locked;
    return importantWindows(displays);
  }, [captureMode, displays, resolvedTileOrder]);
  const shownKey = shownDisplays.map((item) => item.id).join(",");
  const previewKey = useMemo(() => {
    const ids =
      captureMode === "all" && resolvedTileOrder.length > 0
        ? [...resolvedTileOrder]
        : importantWindows(displays).map((item) => item.id);
    if (captureMode === "current" && singleTargetId != null && !ids.includes(singleTargetId)) {
      ids.push(singleTargetId);
    }
    return [...ids].sort((a, b) => a - b).join(",");
  }, [captureMode, resolvedTileOrder, displays, singleTargetId]);
  useEffect(() => {
    const current = displays.find((item) => item.current);
    if (!current && lastCurrentIdRef.current != null) return;
    const nextId = current?.id ?? importantWindows(displays)[0]?.id;
    if (nextId == null || nextId === lastCurrentIdRef.current) return;
    lastCurrentIdRef.current = nextId;
    setLastCurrentId(nextId);
  }, [displays]);

  const currentPreview = useMemo(() => {
    const focusedId = displays.find((item) => item.current)?.id ?? null;
    const id = singleTargetId ?? focusedId ?? lastCurrentId;
    if (id == null) return undefined;
    const exact = previews[id];
    if (exact?.dataUrl) return exact;
    const app = displays.find((item) => item.id === id)?.app?.trim().toLowerCase();
    if (!app) return undefined;
    const sibling = displays.find(
      (item) => item.id !== id && item.app?.trim().toLowerCase() === app && previews[item.id]?.dataUrl,
    );
    return sibling ? previews[sibling.id] : undefined;
  }, [singleTargetId, displays, previews, lastCurrentId]);
  const singleThumb = useMemo(() => {
    const preview = currentPreview?.dataUrl ? currentPreview : null;
    if (thumbStale) {
      if (!preview) return null;
      return {
        src: preview.dataUrl,
        alt: preview.name || windowLabel || "Current window",
        width: preview.width,
        height: preview.height,
        live: false,
      };
    }
    if (capture?.dataUrl) {
      return {
        src: capture.dataUrl,
        alt: windowLabel || "Current window",
        width: capture.width,
        height: capture.height,
        live: true,
      };
    }
    if (preview) {
      return {
        src: preview.dataUrl,
        alt: preview.name || windowLabel || "Current window",
        width: preview.width,
        height: preview.height,
        live: false,
      };
    }
    return null;
  }, [capture, currentPreview, thumbStale, windowLabel]);

  useEffect(() => {
    if (captureMode !== "all" || !shownKey) return;
    const allowed = new Set(shownKey.split(",").map(Number));
    const current = selectedIdsRef.current;
    const next = current.filter((id) => allowed.has(id));
    if (next.length === current.length) return;
    setSelectedIds(next);
    captureStale.current = true;
    void persistCaptureMode("all", next);
    void refreshCapture();
  }, [captureMode, refreshCapture, selectedIdsRef, setSelectedIds, shownKey]);

  useEffect(() => {
    if (showSettings || !previewKey) return;
    let cancelled = false;
    let wait = 0;
    const ids = previewKey.split(",").map(Number);
    const load = () => {
      if (cancelled) return;
      if (previewBusy.current) {
        window.clearTimeout(wait);
        wait = window.setTimeout(load, 200);
        return;
      }
      window.clearTimeout(wait);
      previewBusy.current = true;
      const gen = ++previewGen.current;
      setPreviewsPending(true);
      void previewWindows(ids)
        .then((rows) => {
          if (cancelled) return;
          setPreviews((prev) => {
            const next = { ...prev };
            for (const row of rows) next[row.id] = row;
            return next;
          });
        })
        .catch(() => undefined)
        .finally(() => {
          previewBusy.current = false;
          if (previewGen.current === gen) setPreviewsPending(false);
        });
    };
    load();
    return () => {
      cancelled = true;
      window.clearTimeout(wait);
    };
  }, [showSettings, previewKey]);

  function onToggleScreen(id: number) {
    const ids = selectedIdsRef.current;
    const adding = !ids.includes(id);
    const next = adding ? [...ids, id] : ids.filter((value) => value !== id);
    if (adding) pinWindowIdRef.current = id;
    else if (pinWindowIdRef.current === id) {
      pinWindowIdRef.current = next.length === 1 ? next[0] : null;
    }
    setSelectedIds(next);
    setCaptureMode("all");
    captureStale.current = true;
    void persistCaptureMode("all", next);
    window.clearTimeout(debounceRef.current);
    debounceRef.current = window.setTimeout(() => {
      void refreshCapture();
    }, 250);
  }

  return (
    <div className="overlay-shell">
      <div
        className={[
          "overlay-card",
          expanded ? "expanded" : "compact",
          continuing ? "has-thread" : "",
        ].join(" ")}
        ref={cardRef}
      >
        {passwordNotice && (
          <div className="notice-pop" role="dialog" aria-modal="true" aria-labelledby="password-notice-title">
            <strong id="password-notice-title">Password blurred</strong>
            <p>One or more password fields in this screenshot were blurred before it was sent.</p>
            <button
              className="ghost"
              type="button"
              autoFocus
              onClick={() => {
                setPasswordNotice(false);
                focusInput();
              }}
            >
              OK
            </button>
          </div>
        )}
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
              className={showSettings ? "chip on" : "ghost"}
              type="button"
              onClick={openSettingsView}
            >
              Settings
            </button>
            {continuing &&
              (confirmClear ? (
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
                  disabled={busy}
                  onClick={() => setConfirmClear(true)}
                >
                  Clear
                </button>
              ))}
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
        <div className="overlay-body">
        {error && <div className="banner error">{error}</div>}

        {log.length > 0 && (
          <div className="chat-scroller" ref={scrollerWrapRef}>
          <section className="chat-log" aria-live="polite" ref={chatLogRef} onScroll={onChatScroll}>
            <div className="thread-label">This chat</div>
            {log.map((entry, index) => {
              const lastAssistant = entry.role === "assistant" && index === log.length - 1;
              const latestUser = index === latestUserIndex;
              const chrome: TurnChrome | undefined =
                latestUser || lastAssistant
                  ? {
                      clampQuery: latestUser && queryNeedsClamp && !queryExpanded,
                      queryCap: latestUser ? queryCap : 0,
                      queryExpanded: latestUser ? queryExpanded : false,
                      showClamp: latestUser && queryNeedsClamp,
                      copied: copiedIndex === index,
                      busy: lastAssistant && busy,
                      askStatus: lastAssistant ? askStatus : null,
                      usedVision: lastAssistant && usedVision,
                      usedSearch: lastAssistant && usedSearch,
                      usedSearchApi: lastAssistant ? usedSearchApi : "",
                      searchSources: lastAssistant ? searchSources : EMPTY_SOURCES,
                    }
                  : undefined;
              return (
                <ChatTurn
                  key={`${entry.role}-${index}`}
                  entry={entry}
                  index={index}
                  isLastAssistant={lastAssistant}
                  isFirstUser={index === firstUserIndex}
                  isLatestUser={latestUser}
                  chrome={chrome}
                  latestUserRef={latestUserRef}
                  latestAssistantRef={latestAssistantRef}
                  onOpenPreview={openPreview}
                  onHoverShot={showShotPop}
                  onHideShot={hideShotPop}
                  onToggleQuery={toggleQueryExpanded}
                  onCopyOutput={copyOutput}
                  onAnswerClick={onAnswerClick}
                />
              );
            })}
          </section>
          {showJump && (
            <button className="jump-latest" type="button" onClick={jumpToLatest}>
              Latest
            </button>
          )}
          </div>
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
        <div className="composer-stack">
        {captureMode === "current" && (
          <div className="window-picker-wrap">
            <div className="window-tiles count-1">
              <button
                className="window-tile on"
                type="button"
                title="Recapture"
                onMouseEnter={(event) => {
                  if (singleThumb) {
                    showShotPop(singleThumb.live, singleThumb.src, singleThumb.alt, event.currentTarget);
                  }
                }}
                onMouseLeave={hideShotPop}
                onFocus={(event) => {
                  if (singleThumb) {
                    showShotPop(singleThumb.live, singleThumb.src, singleThumb.alt, event.currentTarget);
                  }
                }}
                onBlur={hideShotPop}
                onClick={() => void refreshCapture()}
              >
                <span className={capturing && singleThumb ? "window-tile-shot pending" : "window-tile-shot"}>
                  {singleThumb ? (
                    <img
                      src={singleThumb.src}
                      alt={singleThumb.alt}
                      style={quarterSize(singleThumb.width, singleThumb.height)}
                    />
                  ) : (
                    <span
                      className="window-tile-placeholder"
                      style={quarterSize(capture?.width || 1280, capture?.height || 720)}
                    />
                  )}
                </span>
                <span className="window-tile-bar" title={windowLabel || currentPreview?.name || undefined}>
                  {capturing && !windowLabel && !currentPreview
                    ? "Finding window…"
                    : windowLabel || currentPreview?.name || "Current window"}
                </span>
              </button>
            </div>
          </div>
        )}
        {captureMode === "all" && (
          <div className="window-picker-wrap">
            <DisplayPicker
              displays={shownDisplays}
              selectedIds={selectedIds}
              previews={previews}
              pending={previewsPending}
              onToggle={(id) => void onToggleScreen(id)}
              onHover={(src, alt, el) => showShotPop(false, src, alt, el)}
              onLeave={hideShotPop}
            />
          </div>
        )}
        <div className="composer">
          <textarea
            ref={inputRef}
            value={query}
            placeholder={
              busy
                ? askStatus?.api
                  ? `${askStatus.api} is ${askStatus.detail}…`
                  : "Type the next message while clAIre answers…"
                : continuing
                  ? "Ask a follow-up in this chat…"
                  : capture
                    ? "Start a new chat about this window…"
                    : "Start a new chat…"
            }
            rows={expanded ? 2 : 1}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                void submit();
              }
            }}
          />
          <div className="capture-toggle" role="group" aria-label="Capture mode">
            <button
              type="button"
              className={captureMode === "none" ? "on" : ""}
              title="No window"
              aria-label="No window"
              onClick={() => setMode("none")}
            >
              <svg viewBox="0 0 24 24" aria-hidden>
                <path
                  fillRule="evenodd"
                  d="M4.5 7h15a1.5 1.5 0 0 1 1.5 1.5v7a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 15.5v-7A1.5 1.5 0 0 1 4.5 7zM7.2 15.4 16.4 8.2l1.4 1.4-9.2 7.2z"
                />
              </svg>
            </button>
            <button
              type="button"
              className={captureMode === "current" ? "on" : ""}
              title="Current window"
              aria-label="Current window"
              onClick={() => setMode("current")}
            >
              <svg viewBox="0 0 24 24" aria-hidden>
                <rect x="3" y="7" width="18" height="10" rx="1.5" />
              </svg>
            </button>
            <button
              type="button"
              className={captureMode === "all" ? "on" : ""}
              title="Multiple windows"
              aria-label="Multiple windows"
              onClick={() => setMode("all")}
            >
              <svg viewBox="0 0 24 24" aria-hidden>
                <rect x="2" y="9" width="15" height="9" rx="1.5" />
                <rect x="7" y="4" width="15" height="9" rx="1.5" opacity="0.45" />
              </svg>
            </button>
          </div>
          {searchAvailable && (
            <button
              className={searchOn ? "web-toggle on" : "web-toggle"}
              type="button"
              role="switch"
              aria-checked={searchOn}
              title={searchOn ? "Web search on" : "Web search off"}
              onClick={() => setSearchOn((value) => !value)}
            >
              <span className="web-toggle-track" aria-hidden>
                <span className="web-toggle-knob">
                  <svg viewBox="0 0 24 24">
                    <circle cx="12" cy="12" r="9" />
                    <path d="M3 12h18" />
                    <path d="M12 3c2.6 3.2 2.6 14.8 0 18M12 3c-2.6 3.2-2.6 14.8 0 18" />
                  </svg>
                </span>
              </span>
              <span className="web-toggle-label">Web</span>
            </button>
          )}
          <button
            className="primary send"
            type="button"
            disabled={busy || !query.trim()}
            title="Send with Enter"
            aria-label="Enter, send message"
            onClick={() => void submit()}
          >
            {busy ? "…" : "Enter"}
          </button>
        </div>
        </div>
        </div>

        <div className="meta">
          <span className="meta-context" title={windowLabel || undefined}>
            <span className="meta-window">
              {captureMode === "none"
                ? "No window"
                : capturing
                  ? windowLabel
                    ? `Sending ${windowLabel}`
                    : "Capturing…"
                  : windowLabel || (capture ? "Screen context" : "No window context yet")}
            </span>
            {captureMode !== "none" && !capturing && capture && (
              <span className="meta-size">
                {capture.width}×{capture.height}
              </span>
            )}
          </span>
          <span>
            {continuing
              ? "Enter sends a follow-up · New chat starts over"
              : "Enter sends · Esc hides"}
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
      {shotPop && !preview && (shotPop.live ? capture?.dataUrl : shotPop.src) && (
        <ShotHover
          src={shotPop.live ? capture!.dataUrl : shotPop.src}
          alt={shotPop.live ? windowLabel || shotPop.alt : shotPop.alt}
          anchor={shotPop.anchor}
        />
      )}
    </div>
  );
}
