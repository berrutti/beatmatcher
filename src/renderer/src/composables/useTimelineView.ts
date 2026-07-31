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

export function useTimelineView(durationMs: () => number, mixerId: () => string) {
  const viewStartMs = ref(0);
  const viewDurationMs = ref(1);
  const scrollY = ref(0);
  let maxScrollY = 0;
  let followSuspended = false;

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

  // Moving the view by hand aims it somewhere the playhead is not, so follow
  // would drag it straight back on the next transport tick.
  function setViewFromUser(next: ViewWindow): void {
    followSuspended = true;
    setView(next);
  }

  // Reset to the whole session whenever the duration becomes known/changes.
  watch(
    () => durationMs(),
    (duration) => {
      followSuspended = false;
      setView({ start: 0, duration: duration || 1 });
    },
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
      mixerId: mixerId(),
      laneOriginY: TICK_H - scrollY.value,
      scrollViewport: { top: TICK_H, bottom: canvasH - OVERVIEW_H - OVERVIEW_GAP }
    };
  }

  function zoomAt(frac: number, deltaY: number): void {
    setViewFromUser(
      zoomAroundCursor(currentView(), frac, deltaY, ZOOM_SENSITIVITY, durationMs(), MIN_VIEW_MS)
    );
  }

  function panByPixels(dxPx: number, trackW: number): void {
    setViewFromUser(
      panByMs(
        currentView(),
        -dxPx * (currentView().duration / (trackW || 1)),
        durationMs(),
        MIN_VIEW_MS
      )
    );
  }

  function panByMsDelta(deltaMs: number): void {
    setViewFromUser(panByMs(currentView(), deltaMs, durationMs(), MIN_VIEW_MS));
  }

  function followPlayhead(ms: number): void {
    if (!Number.isFinite(ms)) return;
    const view = currentView();
    if (followSuspended) {
      if (ms < view.start || ms > view.start + view.duration) return;
      followSuspended = false;
    }
    const next = followTarget(view, ms, FOLLOW_LEAD_IN_FRACTION, durationMs() || 1, MIN_VIEW_MS);
    if (next) setView(next);
  }

  // The renderer reports how tall the content is each frame so scroll can clamp.
  function setContentMetrics(contentHeight: number, viewportHeight: number): void {
    maxScrollY = Math.max(0, contentHeight - viewportHeight);
    scrollY.value = Math.min(maxScrollY, Math.max(0, scrollY.value));
  }

  return {
    viewStartMs,
    viewDurationMs,
    scrollY,
    currentView,
    fullView,
    setView,
    setViewFromUser,
    viewContext,
    zoomAt,
    panByPixels,
    panByMsDelta,
    followPlayhead,
    setContentMetrics,
    maxScrollY: () => maxScrollY
  };
}
