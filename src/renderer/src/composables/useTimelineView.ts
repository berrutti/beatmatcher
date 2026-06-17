// The session timeline's camera: the horizontal view window (zoom/pan) plus the
// vertical scroll of the deck rows, and the canvas-pixel projection (ViewContext)
// every scene item draws/hit-tests against. All the view math itself lives in
// timelineView.ts; this owns the state and exposes intent-level actions.

import { ref, watch } from 'vue';
import {
  type ViewWindow,
  clampView,
  zoomAroundCursor,
  panByMs,
  followTarget,
  fracToMs,
  clampFrac
} from '@renderer/utils/timelineView';
import {
  LABEL_W,
  PADDING,
  TICK_H,
  OVERVIEW_H,
  OVERVIEW_GAP,
  makeMsToX
} from '@renderer/utils/timelineDraw';
import type { ViewContext } from '@renderer/utils/timelineEngine';

const MIN_VIEW_MS = 200;
const ZOOM_SENSITIVITY = 0.0015;
const FOLLOW_LEAD_IN_FRACTION = 0.1;

export function useTimelineView(durationMs: () => number) {
  const viewStartMs = ref(0);
  const viewDurationMs = ref(1);
  const scrollY = ref(0);
  let maxScrollY = 0;

  function currentView(): ViewWindow {
    return { start: viewStartMs.value, duration: viewDurationMs.value };
  }

  function fullView(): ViewWindow {
    return { start: 0, duration: durationMs() || 1 };
  }

  function setView(next: ViewWindow): void {
    const clamped = clampView(next.start, next.duration, durationMs() || 1, MIN_VIEW_MS);
    viewStartMs.value = clamped.start;
    viewDurationMs.value = clamped.duration;
  }

  // Reset to the whole session whenever the duration becomes known/changes.
  watch(
    () => durationMs(),
    (d) => setView({ start: 0, duration: d || 1 }),
    { immediate: true }
  );

  // The per-frame projection. `canvasW/H` are CSS pixels; points handed to
  // hit-testing are canvas-local (clientX - rect.left, clientY - rect.top).
  function viewContext(canvasW: number, canvasH: number): ViewContext {
    const trackW = Math.max(0, canvasW - LABEL_W - PADDING);
    const view = currentView();
    const msToX = makeMsToX(view, trackW);
    return {
      view,
      scrollY: scrollY.value,
      canvasW,
      canvasH,
      trackW,
      msToX,
      xToMs: (x: number) => fracToMs(clampFrac((x - LABEL_W) / (trackW || 1)), view),
      laneOriginY: TICK_H - scrollY.value,
      scrollViewport: { top: TICK_H, bottom: canvasH - OVERVIEW_H - OVERVIEW_GAP }
    };
  }

  // ── horizontal ────────────────────────────────────────────────────────────
  function zoomAt(frac: number, deltaY: number): void {
    setView(
      zoomAroundCursor(currentView(), frac, deltaY, ZOOM_SENSITIVITY, durationMs(), MIN_VIEW_MS)
    );
  }

  function panByPixels(dxPx: number, trackW: number): void {
    setView(
      panByMs(
        currentView(),
        -dxPx * (currentView().duration / (trackW || 1)),
        durationMs(),
        MIN_VIEW_MS
      )
    );
  }

  function panByMsDelta(deltaMs: number): void {
    setView(panByMs(currentView(), deltaMs, durationMs(), MIN_VIEW_MS));
  }

  function followPlayhead(ms: number): void {
    const next = followTarget(
      currentView(),
      ms,
      FOLLOW_LEAD_IN_FRACTION,
      durationMs() || 1,
      MIN_VIEW_MS
    );
    if (next) setView(next);
  }

  // ── vertical scroll ─────────────────────────────────────────────────────��─
  // The renderer reports how tall the content is each frame so scroll can clamp.
  function setContentMetrics(contentHeight: number, viewportHeight: number): void {
    maxScrollY = Math.max(0, contentHeight - viewportHeight);
    scrollY.value = Math.min(maxScrollY, Math.max(0, scrollY.value));
  }

  function scrollByPixels(dy: number): void {
    if (maxScrollY <= 0) return;
    scrollY.value = Math.min(maxScrollY, Math.max(0, scrollY.value + dy));
  }

  function scrollToFraction(frac: number): void {
    scrollY.value = Math.min(maxScrollY, Math.max(0, frac * maxScrollY));
  }

  return {
    viewStartMs,
    viewDurationMs,
    scrollY,
    currentView,
    fullView,
    setView,
    viewContext,
    zoomAt,
    panByPixels,
    panByMsDelta,
    followPlayhead,
    setContentMetrics,
    scrollByPixels,
    scrollToFraction,
    maxScrollY: () => maxScrollY
  };
}
