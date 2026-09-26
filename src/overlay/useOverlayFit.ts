import { useLayoutEffect, type MutableRefObject, type RefObject } from "react";
import { fitOverlay } from "../shared/api";
import type { CaptureMode } from "../shared/types";

export function pinTurn(scroller: HTMLElement, turn: HTMLElement) {
  const sRect = scroller.getBoundingClientRect();
  const qRect = turn.getBoundingClientRect();
  const cap = Math.max(88, scroller.clientHeight * 0.3);
  if (qRect.height <= cap) {
    scroller.scrollTop += qRect.top - sRect.top;
  } else {
    scroller.scrollTop += qRect.bottom - cap - sRect.top;
  }
}

export function followAnswer(scroller: HTMLElement, answer: HTMLElement) {
  const sRect = scroller.getBoundingClientRect();
  const aRect = answer.getBoundingClientRect();
  if (aRect.bottom > sRect.bottom - 8) {
    scroller.scrollTop += aRect.bottom - (sRect.bottom - 8);
  }
}

type OverlayFit = {
  cardRef: RefObject<HTMLDivElement | null>;
  chatLogRef: RefObject<HTMLElement | null>;
  latestUserRef: RefObject<HTMLElement | null>;
  latestAssistantRef: RefObject<HTMLElement | null>;
  pinQueryRef: MutableRefObject<boolean>;
  followRef: MutableRefObject<boolean>;
  ignoreScrollRef: MutableRefObject<boolean>;
  applyFitRef: MutableRefObject<() => void>;
  expanded: boolean;
  showSettings: boolean;
  captureMode: CaptureMode;
  displaysLength: number;
  selectedCount: number;
  capturing: boolean;
  queryNeedsClamp: boolean;
  queryExpanded: boolean;
};

export function useOverlayFit({
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
  displaysLength,
  selectedCount,
  capturing,
  queryNeedsClamp,
  queryExpanded,
}: OverlayFit) {
  useLayoutEffect(() => {
    const el = cardRef.current;
    if (!el) return;
    let lastH = 0;
    let fitted = false;
    let cancelled = false;
    let raf = 0;
    const positions = new Map<Element, { top: number; left: number }>();
    const remember = (node: EventTarget | null) => {
      if (!(node instanceof HTMLElement) || node === el) return;
      positions.set(node, { top: node.scrollTop, left: node.scrollLeft });
    };
    const onScroll = (event: Event) => remember(event.target);
    el.addEventListener("scroll", onScroll, true);

    const pinLatestQuery = () => {
      const scroller = chatLogRef.current;
      const turn = latestUserRef.current;
      if (!scroller || !turn) return;
      pinTurn(scroller, turn);
    };

    const followLatest = () => {
      const scroller = chatLogRef.current;
      const answer = latestAssistantRef.current;
      if (!scroller || !answer) return;
      followAnswer(scroller, answer);
    };

    const restore = () => {
      positions.forEach(({ top, left }, node) => {
        if (!(node instanceof HTMLElement) || !node.isConnected) {
          positions.delete(node);
          return;
        }
        node.scrollTop = top;
        node.scrollLeft = left;
      });
      ignoreScrollRef.current = true;
      if (pinQueryRef.current) pinLatestQuery();
      else if (followRef.current) followLatest();
      requestAnimationFrame(() => {
        ignoreScrollRef.current = false;
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
        const log = child.classList.contains("chat-log")
          ? child
          : (child.querySelector(".chat-log") as HTMLElement | null);
        if (log) {
          height += Math.max(0, child.offsetHeight - log.offsetHeight) + Math.max(log.scrollHeight, log.offsetHeight);
        } else {
          height += child.offsetHeight;
        }
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
    const schedule = () => {
      if (cancelled || raf) return;
      raf = window.requestAnimationFrame(() => {
        raf = 0;
        apply();
      });
    };
    applyFitRef.current = schedule;
    const observer = new ResizeObserver(() => schedule());
    observer.observe(el);
    el.querySelectorAll(
      ".titlebar, .overlay-body, .overlay-body > *, .chat-log, .composer-block, .meta, .window-picker-wrap, .window-tiles",
    ).forEach((node) => {
      observer.observe(node);
    });
    schedule();
    const retry = window.setTimeout(schedule, 50);
    return () => {
      cancelled = true;
      applyFitRef.current = () => {};
      observer.disconnect();
      window.clearTimeout(retry);
      window.cancelAnimationFrame(raf);
      el.removeEventListener("scroll", onScroll, true);
      if (!el.classList.contains("has-thread")) el.style.height = "";
    };
  }, [
    applyFitRef,
    captureMode,
    cardRef,
    chatLogRef,
    expanded,
    followRef,
    ignoreScrollRef,
    latestAssistantRef,
    latestUserRef,
    pinQueryRef,
    showSettings,
  ]);

  useLayoutEffect(() => {
    applyFitRef.current();
  }, [applyFitRef, displaysLength, selectedCount, capturing, queryNeedsClamp, queryExpanded]);
}
