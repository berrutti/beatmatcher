import { describe, it, expect } from 'vitest';
import { useTimelineGestures, type GestureDeps } from '@renderer/composables/useTimelineGestures';
import type { SceneItem, Hit, ViewContext } from '@renderer/utils/timelineEngine';

function fakeItem(hit: Hit | null): SceneItem {
  return {
    bounds: () => ({ x: 0, y: 0, w: 1000, h: 1000 }),
    draw: () => {},
    hitTest: () => hit
  };
}

function cursorFor(hit: Hit | null, editMode = true, shiftKey = false): string {
  const deps: GestureDeps = {
    camera: {} as GestureDeps['camera'],
    emit: () => {},
    getItems: () => (hit ? [fakeItem(hit)] : []),
    getRows: () => [],
    getVc: () => ({}) as ViewContext,
    getClips: () => [],
    getEvents: () => [],
    getDeckLanes: () => ({}),
    laneHeight: () => 64,
    waveformHeight: () => 80,
    isEditMode: () => editMode,
    durationMs: () => 1000,
    nudgeDirectionAt: () => 1,
    nudgeSensitivity: () => 1,
    accentFor: () => '#ffffff',
    requestRender: () => {},
    setCursor: () => {}
  };
  return useTimelineGestures(deps).cursorFor({ x: 50, y: 50 }, shiftKey);
}

describe('cursorFor', () => {
  it('shows grab on a clip body and ew-resize only on its trim edges (edit mode)', () => {
    expect(cursorFor({ target: 'clip', part: 'body' })).toBe('grab');
    expect(cursorFor({ target: 'clip', part: 'start' })).toBe('ew-resize');
    expect(cursorFor({ target: 'clip', part: 'end' })).toBe('ew-resize');
  });

  it('shows no clip cursor outside edit mode', () => {
    expect(cursorFor({ target: 'clip', part: 'body' }, false)).toBe('');
    expect(cursorFor({ target: 'clip', part: 'start' }, false)).toBe('');
  });

  it('shows the draw cursor when Shift is held over a clip in edit mode (nudge paint)', () => {
    expect(cursorFor({ target: 'clip', part: 'body' }, true, true)).toBe('crosshair');
    expect(cursorFor({ target: 'clip', part: 'start' }, true, true)).toBe('crosshair');
    // Shift outside edit mode still paints nothing.
    expect(cursorFor({ target: 'clip', part: 'body' }, false, true)).toBe('');
  });

  it('shows grab on a filter region body and ew-resize on its edges', () => {
    expect(cursorFor({ target: 'filterRegion', part: 'body' })).toBe('grab');
    expect(cursorFor({ target: 'filterRegion', part: 'start' })).toBe('ew-resize');
    expect(cursorFor({ target: 'filterRegion', part: 'end' })).toBe('ew-resize');
  });

  it('falls back to the default cursor with nothing under the pointer', () => {
    expect(cursorFor(null)).toBe('');
  });
});
