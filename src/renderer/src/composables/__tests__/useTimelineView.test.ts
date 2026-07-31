import { describe, it, expect } from 'vitest';
import { useTimelineView } from '@renderer/composables/useTimelineView';

const SESSION_MS = 100_000;

function camera() {
  const view = useTimelineView(
    () => SESSION_MS,
    () => 'classic-3band'
  );
  view.setView({ start: 0, duration: 10_000 });
  return view;
}

describe('useTimelineView follow', () => {
  it('re-frames when the playhead runs off the edge of the view', () => {
    const view = camera();
    view.followPlayhead(40_000);
    expect(view.viewStartMs.value).toBe(39_000);
  });

  it('leaves the view alone while the playhead is already inside it', () => {
    const view = camera();
    view.followPlayhead(5_000);
    expect(view.viewStartMs.value).toBe(0);
  });

  it('keeps a view the user moved away from the playhead', () => {
    const view = camera();
    view.followPlayhead(5_000);
    view.setViewFromUser({ start: 60_000, duration: 10_000 });
    view.followPlayhead(5_100);
    expect(view.viewStartMs.value).toBe(60_000);
  });

  it('keeps a user view across many playhead ticks', () => {
    const view = camera();
    view.setViewFromUser({ start: 60_000, duration: 10_000 });
    for (let ms = 5_000; ms < 9_000; ms += 100) view.followPlayhead(ms);
    expect(view.viewStartMs.value).toBe(60_000);
  });

  it('follows again once playback reaches the view the user moved to', () => {
    const view = camera();
    view.setViewFromUser({ start: 60_000, duration: 10_000 });
    view.followPlayhead(65_000);
    expect(view.viewStartMs.value).toBe(60_000);
    view.followPlayhead(75_000);
    expect(view.viewStartMs.value).toBe(74_000);
  });

  it('keeps a wheel zoom the user aimed away from the playhead', () => {
    const view = camera();
    view.followPlayhead(5_000);
    view.zoomAt(1, -2_000);
    const zoomedStart = view.viewStartMs.value;
    view.followPlayhead(5_100);
    expect(view.viewStartMs.value).toBe(zoomedStart);
  });

  it('follows at the zoom the user chose once the playhead re-enters the view', () => {
    const view = camera();
    view.zoomAt(0.5, -100);
    const zoomed = view.viewDurationMs.value;
    view.followPlayhead(5_000);
    view.followPlayhead(90_000);
    expect(view.viewDurationMs.value).toBe(zoomed);
    expect(view.viewStartMs.value).toBe(90_000 - zoomed * 0.1);
  });
});
