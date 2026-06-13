import { describe, it, expect, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));
vi.mock('@tauri-apps/plugin-store', () => ({ load: vi.fn() }));

import {
  laneSpecFor,
  normalizeGestureSamples,
  decimateSteps,
  originalValueAt,
  spliceLaneEvents,
  filterActiveAt,
  toggleFilterActiveRange,
  nudgeValueAt,
  paintNudgeRange,
  deleteNudgeRange,
  relocateEventPaths,
  formatLaneValue,
  MIN_GESTURE_MS
} from '../sessionEditOps';
import type { SessionEvent } from '@renderer/stores/session';

function ev(overrides: Partial<SessionEvent> & { elapsed_ms: number; type: string }): SessionEvent {
  return overrides as SessionEvent;
}

describe('normalizeGestureSamples', () => {
  it('sorts samples by ms', () => {
    const out = normalizeGestureSamples([
      { ms: 300, value: 0.3 },
      { ms: 100, value: 0.1 },
      { ms: 200, value: 0.2 }
    ]);
    expect(out.map((p) => p.ms)).toEqual([100, 200, 300]);
  });

  it('keeps the last value written at a timestamp (back-and-forth drag)', () => {
    const out = normalizeGestureSamples([
      { ms: 100, value: 0.1 },
      { ms: 200, value: 0.2 },
      { ms: 100, value: 0.9 }
    ]);
    expect(out).toEqual([
      { ms: 100, value: 0.9 },
      { ms: 200, value: 0.2 }
    ]);
  });
});

describe('decimateSteps', () => {
  it('drops sub-epsilon wiggle', () => {
    const out = decimateSteps(
      [
        { ms: 0, value: 0.5 },
        { ms: 10, value: 0.502 },
        { ms: 20, value: 0.498 },
        { ms: 30, value: 0.5 }
      ],
      0.01
    );
    expect(out).toEqual([{ ms: 0, value: 0.5 }]);
  });

  it('keeps steps of at least epsilon', () => {
    const out = decimateSteps(
      [
        { ms: 0, value: 0 },
        { ms: 10, value: 0.005 },
        { ms: 20, value: 0.011 },
        { ms: 30, value: 0.3 },
        { ms: 40, value: 0.3 }
      ],
      0.01
    );
    expect(out).toEqual([
      { ms: 0, value: 0 },
      { ms: 20, value: 0.011 },
      { ms: 30, value: 0.3 }
    ]);
  });

  it('always keeps the final value when it differs from the last kept point', () => {
    const out = decimateSteps(
      [
        { ms: 0, value: 0 },
        { ms: 10, value: 0.5 },
        { ms: 20, value: 0.504 }
      ],
      0.01
    );
    expect(out).toEqual([
      { ms: 0, value: 0 },
      { ms: 10, value: 0.5 },
      { ms: 20, value: 0.504 }
    ]);
  });

  it('handles single-point input', () => {
    expect(decimateSteps([{ ms: 5, value: 0.3 }], 0.01)).toEqual([{ ms: 5, value: 0.3 }]);
  });
});

describe('originalValueAt', () => {
  const spec = laneSpecFor('gain');

  it('returns the last matching value at or before ms', () => {
    const events = [
      ev({ elapsed_ms: 100, type: 'set_volume', deck: 'A', gain: 0.4 }),
      ev({ elapsed_ms: 500, type: 'set_volume', deck: 'A', gain: 0.8 })
    ];
    expect(originalValueAt(events, spec, 'A', 300)).toBe(0.4);
    expect(originalValueAt(events, spec, 'A', 500)).toBe(0.8);
  });

  it('falls back to the lane default before any event', () => {
    const events = [ev({ elapsed_ms: 1000, type: 'set_volume', deck: 'A', gain: 0.4 })];
    expect(originalValueAt(events, spec, 'A', 500)).toBe(1);
  });

  it('ignores other decks', () => {
    const events = [ev({ elapsed_ms: 100, type: 'set_volume', deck: 'B', gain: 0.2 })];
    expect(originalValueAt(events, spec, 'A', 300)).toBe(1);
  });

  it('reads the rate from deck_snapshot playback_rate', () => {
    const rateSpec = laneSpecFor('rate');
    const events = [ev({ elapsed_ms: 0, type: 'deck_snapshot', deck: 'A', playback_rate: 0.95 })];
    expect(originalValueAt(events, rateSpec, 'A', 100)).toBe(0.95);
  });
});

describe('spliceLaneEvents', () => {
  const spec = laneSpecFor('gain');

  it('deletes only matching events inside the range and inserts drawn points', () => {
    const events = [
      ev({ elapsed_ms: 500, type: 'set_volume', deck: 'A', gain: 0.9 }),
      ev({ elapsed_ms: 1500, type: 'set_volume', deck: 'A', gain: 0.5 }),
      ev({ elapsed_ms: 1600, type: 'set_volume', deck: 'B', gain: 0.2 }),
      ev({ elapsed_ms: 1700, type: 'set_eq', deck: 'A', band: 'low', db: -3 }),
      ev({ elapsed_ms: 3000, type: 'set_volume', deck: 'A', gain: 0.7 })
    ];
    const out = spliceLaneEvents(events, spec, 'A', 1000, 2000, [
      { ms: 1200, value: 0.3 },
      { ms: 1800, value: 0.6 }
    ]);

    const gainsA = out.filter((e) => e.type === 'set_volume' && e.deck === 'A');
    expect(gainsA.map((e) => [e.elapsed_ms, e.gain])).toEqual([
      [500, 0.9],
      [1200, 0.3],
      [1800, 0.6],
      [2000, 0.5],
      [3000, 0.7]
    ]);
    expect(out.some((e) => e.type === 'set_volume' && e.deck === 'B')).toBe(true);
    expect(out.some((e) => e.type === 'set_eq')).toBe(true);
  });

  it('appends a restore event at t1 with the original value', () => {
    const events = [ev({ elapsed_ms: 500, type: 'set_volume', deck: 'A', gain: 0.9 })];
    const out = spliceLaneEvents(events, spec, 'A', 1000, 2000, [{ ms: 1500, value: 0.2 }]);
    const last = out[out.length - 1];
    expect(last).toMatchObject({ elapsed_ms: 2000, type: 'set_volume', deck: 'A', gain: 0.9 });
  });

  it('omits the restore event when the last drawn value equals the original', () => {
    const events = [ev({ elapsed_ms: 500, type: 'set_volume', deck: 'A', gain: 0.9 })];
    const out = spliceLaneEvents(events, spec, 'A', 1000, 2000, [
      { ms: 1500, value: 0.2 },
      { ms: 1900, value: 0.9 }
    ]);
    const inRange = out.filter((e) => e.elapsed_ms >= 1000 && e.elapsed_ms <= 2000);
    expect(inRange).toHaveLength(2);
  });

  it('restores the lane default when drawing on an empty lane', () => {
    const out = spliceLaneEvents([], spec, 'A', 1000, 2000, [{ ms: 1500, value: 0.2 }]);
    expect(out[out.length - 1]).toMatchObject({ elapsed_ms: 2000, gain: 1 });
  });

  it('never deletes deck_snapshot or set_filter_active events', () => {
    const events = [
      ev({ elapsed_ms: 1200, type: 'deck_snapshot', deck: 'A', playback_rate: 1 }),
      ev({ elapsed_ms: 1300, type: 'set_filter_active', deck: 'A', active: true })
    ];
    const rateSpec = laneSpecFor('rate');
    const filterSpec = laneSpecFor('filter');
    for (const s of [spec, rateSpec, filterSpec]) {
      const out = spliceLaneEvents(events, s, 'A', 1000, 2000, [{ ms: 1500, value: 1 }]);
      expect(out.some((e) => e.type === 'deck_snapshot')).toBe(true);
      expect(out.some((e) => e.type === 'set_filter_active')).toBe(true);
    }
  });

  it('keeps pre-existing events ahead of inserted ones at identical timestamps', () => {
    const events = [ev({ elapsed_ms: 1500, type: 'set_eq', deck: 'A', band: 'low', db: -3 })];
    const out = spliceLaneEvents(events, spec, 'A', 1000, 2000, [{ ms: 1500, value: 0.4 }]);
    const atMs = out.filter((e) => e.elapsed_ms === 1500);
    expect(atMs[0].type).toBe('set_eq');
    expect(atMs[1].type).toBe('set_volume');
  });

  it('does not mutate the input array or its events', () => {
    const events = [ev({ elapsed_ms: 1500, type: 'set_volume', deck: 'A', gain: 0.5 })];
    const snapshot = JSON.parse(JSON.stringify(events));
    spliceLaneEvents(events, spec, 'A', 1000, 2000, [{ ms: 1200, value: 0.3 }]);
    expect(events).toEqual(snapshot);
  });

  it('returns the input unchanged for an empty gesture', () => {
    const events = [ev({ elapsed_ms: 1500, type: 'set_volume', deck: 'A', gain: 0.5 })];
    expect(spliceLaneEvents(events, spec, 'A', 1000, 2000, [])).toBe(events);
  });

  it('rejects gestures shorter than the minimum duration', () => {
    const events = [ev({ elapsed_ms: 500, type: 'set_volume', deck: 'A', gain: 0.9 })];
    const t1 = 1000 + MIN_GESTURE_MS - 1;
    const out = spliceLaneEvents(events, spec, 'A', 1000, t1, [{ ms: 1000, value: 0.2 }]);
    expect(out).toBe(events);
  });

  it('accepts gestures at exactly the minimum duration', () => {
    const t1 = 1000 + MIN_GESTURE_MS;
    const out = spliceLaneEvents([], spec, 'A', 1000, t1, [{ ms: 1000, value: 0.2 }]);
    expect(out.length).toBeGreaterThan(0);
  });

  it('clamps drawn values to the lane range', () => {
    const out = spliceLaneEvents([], spec, 'A', 0, 100, [{ ms: 50, value: 1.7 }]);
    expect(out[0]).toMatchObject({ gain: 1 });
  });

  it('snaps filter values inside the dead zone to exactly 0', () => {
    const filterSpec = laneSpecFor('filter');
    const out = spliceLaneEvents([], filterSpec, 'A', 0, 100, [{ ms: 50, value: 0.03 }]);
    expect(out[0]).toMatchObject({ type: 'set_filter', value: 0 });
  });

  it('master lane ignores deck on match and insert', () => {
    const masterSpec = laneSpecFor('masterGain');
    const events = [ev({ elapsed_ms: 1500, type: 'set_master_gain', gain: 0.6 })];
    const out = spliceLaneEvents(events, masterSpec, '', 1000, 2000, [{ ms: 1200, value: 0.4 }]);
    const masters = out.filter((e) => e.type === 'set_master_gain');
    expect(masters.map((e) => [e.elapsed_ms, e.gain])).toEqual([
      [1200, 0.4],
      [2000, 0.6]
    ]);
    expect(masters.every((e) => e.deck === undefined)).toBe(true);
  });

  it('replaces only the named EQ band', () => {
    const eqSpec = laneSpecFor('eqMid');
    const events = [
      ev({ elapsed_ms: 1500, type: 'set_eq', deck: 'A', band: 'low', db: -3 }),
      ev({ elapsed_ms: 1500, type: 'set_eq', deck: 'A', band: 'mid', db: 2 })
    ];
    const out = spliceLaneEvents(events, eqSpec, 'A', 1000, 2000, [{ ms: 1200, value: -6 }]);
    expect(out.some((e) => e.band === 'low' && e.db === -3)).toBe(true);
    const mids = out.filter((e) => e.type === 'set_eq' && e.band === 'mid');
    // The original mid event at 1500 is replaced; its value comes back as the restore at t1.
    expect(mids.map((e) => [e.elapsed_ms, e.db])).toEqual([
      [1200, -6],
      [2000, 2]
    ]);
  });
});

describe('filterActiveAt', () => {
  it('returns the last active state at or before ms, defaulting to false', () => {
    const events = [
      ev({ elapsed_ms: 1000, type: 'set_filter_active', deck: 'A', active: true }),
      ev({ elapsed_ms: 3000, type: 'set_filter_active', deck: 'A', active: false })
    ];
    expect(filterActiveAt(events, 'A', 500)).toBe(false);
    expect(filterActiveAt(events, 'A', 2000)).toBe(true);
    expect(filterActiveAt(events, 'A', 4000)).toBe(false);
  });

  it('excludes an event at exactly ms when inclusive is false', () => {
    const events = [ev({ elapsed_ms: 1000, type: 'set_filter_active', deck: 'A', active: true })];
    expect(filterActiveAt(events, 'A', 1000, true)).toBe(true);
    expect(filterActiveAt(events, 'A', 1000, false)).toBe(false);
  });

  it('ignores other decks', () => {
    const events = [ev({ elapsed_ms: 1000, type: 'set_filter_active', deck: 'B', active: true })];
    expect(filterActiveAt(events, 'A', 2000)).toBe(false);
  });
});

describe('toggleFilterActiveRange', () => {
  it('turns an inactive range on and restores off at t1', () => {
    const out = toggleFilterActiveRange([], 'A', 1000, 2000);
    expect(out.map((e) => [e.elapsed_ms, e.active])).toEqual([
      [1000, true],
      [2000, false]
    ]);
  });

  it('turns an active range off and restores on at t1', () => {
    const events = [ev({ elapsed_ms: 500, type: 'set_filter_active', deck: 'A', active: true })];
    const out = toggleFilterActiveRange(events, 'A', 1000, 2000);
    const inserted = out.filter((e) => e.elapsed_ms >= 1000);
    expect(inserted.map((e) => [e.elapsed_ms, e.active])).toEqual([
      [1000, false],
      [2000, true]
    ]);
  });

  it('replaces activation events inside the range', () => {
    const events = [
      ev({ elapsed_ms: 1200, type: 'set_filter_active', deck: 'A', active: true }),
      ev({ elapsed_ms: 1800, type: 'set_filter_active', deck: 'A', active: false })
    ];
    const out = toggleFilterActiveRange(events, 'A', 1000, 2000);
    expect(out.map((e) => [e.elapsed_ms, e.active])).toEqual([
      [1000, true],
      [2000, false]
    ]);
  });

  it('omits the restore when the original state at t1 matches the painted state', () => {
    // Original turns on at 1500; painting on over [1000, 2000] ends in the same state.
    const events = [ev({ elapsed_ms: 1500, type: 'set_filter_active', deck: 'A', active: true })];
    const out = toggleFilterActiveRange(events, 'A', 1000, 2000);
    expect(out.map((e) => [e.elapsed_ms, e.active])).toEqual([[1000, true]]);
  });

  it('does not touch set_filter value events or other decks', () => {
    const events = [
      ev({ elapsed_ms: 1500, type: 'set_filter', deck: 'A', value: -0.4 }),
      ev({ elapsed_ms: 1500, type: 'set_filter_active', deck: 'B', active: true })
    ];
    const out = toggleFilterActiveRange(events, 'A', 1000, 2000);
    expect(out.some((e) => e.type === 'set_filter' && e.value === -0.4)).toBe(true);
    expect(out.some((e) => e.deck === 'B' && e.active === true)).toBe(true);
  });

  it('returns the input unchanged for an empty or inverted range', () => {
    const events = [ev({ elapsed_ms: 500, type: 'set_filter_active', deck: 'A', active: true })];
    expect(toggleFilterActiveRange(events, 'A', 1000, 1000)).toBe(events);
    expect(toggleFilterActiveRange(events, 'A', 2000, 1000)).toBe(events);
  });

  it('rejects ranges shorter than the minimum gesture duration', () => {
    const events = [ev({ elapsed_ms: 500, type: 'set_filter_active', deck: 'A', active: true })];
    const t1 = 1000 + MIN_GESTURE_MS - 1;
    expect(toggleFilterActiveRange(events, 'A', 1000, t1)).toBe(events);
  });

  it('does not mutate the input array', () => {
    const events = [ev({ elapsed_ms: 500, type: 'set_filter_active', deck: 'A', active: true })];
    const snapshot = JSON.parse(JSON.stringify(events));
    toggleFilterActiveRange(events, 'A', 1000, 2000);
    expect(events).toEqual(snapshot);
  });
});

describe('nudgeValueAt', () => {
  it('returns the last nudge percent at or before ms, defaulting to 0', () => {
    const events = [
      ev({ elapsed_ms: 1000, type: 'set_nudge', deck: 'A', percent: 4 }),
      ev({ elapsed_ms: 3000, type: 'set_nudge', deck: 'A', percent: 0 })
    ];
    expect(nudgeValueAt(events, 'A', 500)).toBe(0);
    expect(nudgeValueAt(events, 'A', 2000)).toBe(4);
    expect(nudgeValueAt(events, 'A', 4000)).toBe(0);
  });

  it('ignores other decks', () => {
    const events = [ev({ elapsed_ms: 1000, type: 'set_nudge', deck: 'B', percent: 8 })];
    expect(nudgeValueAt(events, 'A', 2000)).toBe(0);
  });
});

describe('deleteNudgeRange', () => {
  it('removes the opener, mid-span changes, and the closing zero', () => {
    const events = [
      ev({ elapsed_ms: 1000, type: 'set_nudge', deck: 'A', percent: 4 }),
      ev({ elapsed_ms: 1500, type: 'set_nudge', deck: 'A', percent: 8 }),
      ev({ elapsed_ms: 2000, type: 'set_nudge', deck: 'A', percent: 0 })
    ];
    const out = deleteNudgeRange(events, 'A', 1000, 2000);
    expect(out.filter((event) => event.type === 'set_nudge')).toEqual([]);
  });

  it('leaves other decks, other event types, and nudges outside the range alone', () => {
    const events = [
      ev({ elapsed_ms: 500, type: 'set_nudge', deck: 'A', percent: 4 }),
      ev({ elapsed_ms: 800, type: 'set_nudge', deck: 'A', percent: 0 }),
      ev({ elapsed_ms: 1200, type: 'set_nudge', deck: 'A', percent: 4 }),
      ev({ elapsed_ms: 1500, type: 'set_nudge', deck: 'B', percent: 8 }),
      ev({ elapsed_ms: 1500, type: 'set_volume', deck: 'A', gain: 0.5 }),
      ev({ elapsed_ms: 1800, type: 'set_nudge', deck: 'A', percent: 0 })
    ];
    const out = deleteNudgeRange(events, 'A', 1200, 1800);
    expect(out.map((event) => [event.elapsed_ms, event.type, event.deck])).toEqual([
      [500, 'set_nudge', 'A'],
      [800, 'set_nudge', 'A'],
      [1500, 'set_nudge', 'B'],
      [1500, 'set_volume', 'A']
    ]);
  });

  it('keeps a following span opener sitting exactly at the range end', () => {
    const events = [
      ev({ elapsed_ms: 1000, type: 'set_nudge', deck: 'A', percent: 4 }),
      ev({ elapsed_ms: 2000, type: 'set_nudge', deck: 'A', percent: 0 }),
      ev({ elapsed_ms: 2000, type: 'set_nudge', deck: 'A', percent: -4 }),
      ev({ elapsed_ms: 2500, type: 'set_nudge', deck: 'A', percent: 0 })
    ];
    const out = deleteNudgeRange(events, 'A', 1000, 2000);
    expect(
      out
        .filter((event) => event.type === 'set_nudge')
        .map((event) => [event.elapsed_ms, event.percent])
    ).toEqual([
      [2000, -4],
      [2500, 0]
    ]);
  });

  it('removes an unfinished span that runs to the session end', () => {
    const events = [ev({ elapsed_ms: 9000, type: 'set_nudge', deck: 'A', percent: 6 })];
    const out = deleteNudgeRange(events, 'A', 9000, 10_000);
    expect(out).toEqual([]);
  });

  it('returns the input array unchanged when nothing matches', () => {
    const events = [ev({ elapsed_ms: 500, type: 'set_volume', deck: 'A', gain: 0.5 })];
    expect(deleteNudgeRange(events, 'A', 1000, 2000)).toBe(events);
  });
});

describe('paintNudgeRange', () => {
  it('inserts a nudge at t0 and returns to 0 at t1', () => {
    const out = paintNudgeRange([], 'A', 1000, 2000, 4);
    expect(out.map((event) => [event.elapsed_ms, event.percent])).toEqual([
      [1000, 4],
      [2000, 0]
    ]);
    expect(out.every((event) => event.type === 'set_nudge' && event.deck === 'A')).toBe(true);
  });

  it('supports negative percent for nudge down', () => {
    const out = paintNudgeRange([], 'A', 1000, 2000, -4);
    expect(out[0]).toMatchObject({ elapsed_ms: 1000, percent: -4 });
    expect(out[1]).toMatchObject({ elapsed_ms: 2000, percent: 0 });
  });

  it('replaces recorded nudge events inside the range', () => {
    const events = [
      ev({ elapsed_ms: 1200, type: 'set_nudge', deck: 'A', percent: 8 }),
      ev({ elapsed_ms: 1800, type: 'set_nudge', deck: 'A', percent: 0 })
    ];
    const out = paintNudgeRange(events, 'A', 1000, 2000, 4);
    expect(out.map((event) => [event.elapsed_ms, event.percent])).toEqual([
      [1000, 4],
      [2000, 0]
    ]);
  });

  it('restores the recorded percent when the range ends inside an active nudge', () => {
    const events = [
      ev({ elapsed_ms: 1500, type: 'set_nudge', deck: 'A', percent: 8 }),
      ev({ elapsed_ms: 3000, type: 'set_nudge', deck: 'A', percent: 0 })
    ];
    const out = paintNudgeRange(events, 'A', 1000, 2000, 4);
    const inserted = out.filter((event) => event.elapsed_ms <= 2000);
    expect(inserted.map((event) => [event.elapsed_ms, event.percent])).toEqual([
      [1000, 4],
      [2000, 8]
    ]);
    expect(out.some((event) => event.elapsed_ms === 3000 && event.percent === 0)).toBe(true);
  });

  it('does not touch other decks or other event types', () => {
    const events = [
      ev({ elapsed_ms: 1500, type: 'set_nudge', deck: 'B', percent: 8 }),
      ev({ elapsed_ms: 1500, type: 'set_volume', deck: 'A', gain: 0.5 })
    ];
    const out = paintNudgeRange(events, 'A', 1000, 2000, 4);
    expect(out.some((event) => event.deck === 'B' && event.percent === 8)).toBe(true);
    expect(out.some((event) => event.type === 'set_volume')).toBe(true);
  });

  it('rejects ranges shorter than the minimum gesture duration', () => {
    const events = [ev({ elapsed_ms: 500, type: 'set_nudge', deck: 'A', percent: 4 })];
    const t1 = 1000 + MIN_GESTURE_MS - 1;
    expect(paintNudgeRange(events, 'A', 1000, t1, 4)).toBe(events);
  });

  it('does not mutate the input array', () => {
    const events = [ev({ elapsed_ms: 1500, type: 'set_nudge', deck: 'A', percent: 8 })];
    const snapshot = JSON.parse(JSON.stringify(events));
    paintNudgeRange(events, 'A', 1000, 2000, 4);
    expect(events).toEqual(snapshot);
  });
});

describe('relocateEventPaths', () => {
  const events = [
    ev({ elapsed_ms: 0, type: 'deck_snapshot', deck: 'A', path: '/old/a.mp3' }),
    ev({ elapsed_ms: 100, type: 'load_track', deck: 'B', path: '/old/b.mp3' }),
    ev({ elapsed_ms: 200, type: 'set_gain', deck: 'A', gain: 0.5 }),
    ev({ elapsed_ms: 300, type: 'load_track', deck: 'A', path: '/old/a.mp3' })
  ];

  it('rewrites every event carrying a mapped path', () => {
    const out = relocateEventPaths(events, { '/old/a.mp3': '/new/a.mp3' });
    expect(out.map((event) => event.path)).toEqual([
      '/new/a.mp3',
      '/old/b.mp3',
      undefined,
      '/new/a.mp3'
    ]);
  });

  it('rewrites multiple paths in one pass', () => {
    const out = relocateEventPaths(events, {
      '/old/a.mp3': '/new/a.mp3',
      '/old/b.mp3': '/new/b.mp3'
    });
    expect(out.map((event) => event.path)).toEqual([
      '/new/a.mp3',
      '/new/b.mp3',
      undefined,
      '/new/a.mp3'
    ]);
  });

  it('returns a new array and never mutates the input', () => {
    const snapshot = JSON.parse(JSON.stringify(events));
    const out = relocateEventPaths(events, { '/old/a.mp3': '/new/a.mp3' });
    expect(out).not.toBe(events);
    expect(events).toEqual(snapshot);
  });

  it('returns the same array reference when nothing matches', () => {
    expect(relocateEventPaths(events, { '/elsewhere/x.mp3': '/new/x.mp3' })).toBe(events);
  });

  it('preserves all other fields on rewritten events', () => {
    const out = relocateEventPaths(events, { '/old/a.mp3': '/new/a.mp3' });
    expect(out[0]).toMatchObject({ elapsed_ms: 0, type: 'deck_snapshot', deck: 'A' });
  });
});

describe('formatLaneValue', () => {
  it('formats rate as signed percent', () => {
    expect(formatLaneValue('rate', 1.032)).toBe('+3.2%');
    expect(formatLaneValue('rate', 0.875)).toBe('-12.5%');
  });

  it('formats EQ as signed dB', () => {
    expect(formatLaneValue('eqLow', -12.5)).toBe('-12.5 dB');
    expect(formatLaneValue('eqHigh', 3)).toBe('+3.0 dB');
  });

  it('formats gain values as plain numbers', () => {
    expect(formatLaneValue('gain', 0.75)).toBe('0.75');
    expect(formatLaneValue('masterGain', 0.7943)).toBe('0.79');
  });
});
