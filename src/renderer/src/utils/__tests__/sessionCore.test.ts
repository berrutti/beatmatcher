// Marshalling-layer tests for the sessionCore WASM wrappers; edit-op semantics
// are covered by the Rust suite in session-core.

import { describe, it, expect } from 'vitest';
import { laneSpecFor } from '@renderer/utils/sessionEditOps';
import { ALL_LANE_KEYS } from '@renderer/utils/types';
import {
  laneSpecs,
  mixerParams,
  buildTimeline,
  blockBounds,
  blocksForDeck,
  moveTransportBlock,
  trimTransportBlock,
  spliceLaneEvents,
  normalizeGestureSamples,
  decimateSteps,
  filterActiveAt,
  toggleFilterActiveRange,
  deleteNudgeRange,
  relocateEventPaths,
  editConstants,
  currentBeat
} from '@renderer/utils/sessionCore';
import { PITCH_RANGE_OPTIONS } from '@renderer/stores/settings';
import type { SessionEvent } from '@renderer/utils/types';

const CLASSIC = 'classic-3band';
const ISOLATOR = 'isolator-3band';

function simpleSession(): SessionEvent[] {
  return [
    { elapsed_ms: 0, type: 'load_track', deck: 'A', path: '/tracks/one.mp3' },
    { elapsed_ms: 1000, type: 'play', deck: 'A' },
    { elapsed_ms: 2000, type: 'set_param', deck: 'A', slot: 'fader', param: 'gain', value: 0.5 },
    { elapsed_ms: 5000, type: 'stop', deck: 'A' }
  ];
}

function builtFor(events: SessionEvent[]) {
  return buildTimeline(
    events,
    10_000,
    PITCH_RANGE_OPTIONS,
    (path) => `name:${path}`,
    () => null
  );
}

describe('buildTimeline', () => {
  it('returns camelCase clips, spans, and lanes with names resolved', () => {
    const built = builtFor(simpleSession());
    expect(built.clips).toHaveLength(1);
    const clip = built.clips[0];
    expect(clip.sessionStartMs).toBe(1000);
    expect(clip.sessionEndMs).toBe(5000);
    expect(clip.trackPath).toBe('/tracks/one.mp3');
    expect(clip.trackName).toBe('name:/tracks/one.mp3');
    expect(built.loadedSpans).toHaveLength(1);
    expect(built.loadedSpans[0].trackName).toBe('name:/tracks/one.mp3');
    expect(built.deckLanes['A'].gain.length).toBeGreaterThan(1);
    expect(built.masterLanes.gain[0].ms).toBe(0);
    expect(built.deckNudges['A']).toEqual([]);
  });

  it('prefers the collection grid and falls back to recorded values', () => {
    const events: SessionEvent[] = [
      ...simpleSession().slice(0, 1),
      { elapsed_ms: 10, type: 'set_beat_grid', deck: 'A', bpm: 120, beat_offset_sec: 0.5 },
      ...simpleSession().slice(1)
    ];
    const fromGrid = buildTimeline(
      events,
      10_000,
      PITCH_RANGE_OPTIONS,
      (path) => path,
      () => ({ bpm: 128, beatOffsetSec: 0.25 })
    );
    expect(fromGrid.clips[0].bpm).toBe(128);
    expect(fromGrid.clips[0].beatOffsetSec).toBe(0.25);

    const fromRecording = builtFor(events);
    expect(fromRecording.clips[0].bpm).toBe(120);
    expect(fromRecording.clips[0].beatOffsetSec).toBe(0.5);
  });
});

describe('blockBounds', () => {
  it('restores Infinity for an open right bound and carries trim clamps', () => {
    const events = simpleSession();
    const { clips } = builtFor(events);
    const block = blocksForDeck(clips, 'A')[0];
    const bounds = blockBounds(events, clips, block);
    expect(bounds).not.toBeNull();
    if (!bounds) return;
    expect(bounds.minStartMs).toBe(0);
    expect(bounds.maxEndMs).toBe(Infinity);
    // Track second 0 plays at session ms 1000, so the start edge cannot be
    // dragged earlier than the block start itself here.
    expect(bounds.startTrimMinMs).toBe(1000);
    expect(bounds.minBlockMs).toBeGreaterThan(0);
  });
});

describe('transport edit wrappers', () => {
  it('preserves the input reference for a sub-millisecond move', () => {
    const events = simpleSession();
    const { clips } = builtFor(events);
    const block = blocksForDeck(clips, 'A')[0];
    const result = moveTransportBlock(events, clips, block, 0.4);
    expect(result.appliedDeltaMs).toBe(0);
    expect(result.events).toBe(events);
  });

  it('applies a real move in open space', () => {
    const events = simpleSession();
    const { clips } = builtFor(events);
    const block = blocksForDeck(clips, 'A')[0];
    const result = moveTransportBlock(events, clips, block, 2000);
    expect(result.appliedDeltaMs).toBe(2000);
    expect(result.events).not.toBe(events);
    const rebuilt = builtFor(result.events);
    expect(rebuilt.clips[0].sessionStartMs).toBeCloseTo(3000, 3);
    expect(rebuilt.clips[0].sessionEndMs).toBeCloseTo(7000, 3);
  });

  it('preserves the input reference for a no-op trim', () => {
    const events = simpleSession();
    const { clips } = builtFor(events);
    const block = blocksForDeck(clips, 'A')[0];
    const result = trimTransportBlock(events, clips, block, 'end', block.endMs);
    expect(result.appliedMs).toBe(block.endMs);
    expect(result.events).toBe(events);
  });
});

describe('lane edit wrappers', () => {
  it('normalizes gestures last-write-wins per ms and sorted', () => {
    const out = normalizeGestureSamples([
      { ms: 200, value: 0.1 },
      { ms: 100, value: 0.9 },
      { ms: 200, value: 0.5 }
    ]);
    expect(out).toEqual([
      { ms: 100, value: 0.9 },
      { ms: 200, value: 0.5 }
    ]);
  });

  it('decimates steps below epsilon but keeps the final value', () => {
    const out = decimateSteps(
      [
        { ms: 0, value: 0 },
        { ms: 1, value: 0.005 },
        { ms: 2, value: 0.5 }
      ],
      0.01
    );
    expect(out).toEqual([
      { ms: 0, value: 0 },
      { ms: 2, value: 0.5 }
    ]);
  });

  it('splices a gain gesture and restores the prior value at t1', () => {
    const events: SessionEvent[] = [
      { elapsed_ms: 1000, type: 'set_param', deck: 'A', slot: 'fader', param: 'gain', value: 0.8 }
    ];
    const out = spliceLaneEvents(events, 'gain', CLASSIC, 'A', 5000, 8000, [
      { ms: 5000, value: 0.4 },
      { ms: 6000, value: 0.4 }
    ]);
    const gains = out
      .filter((event) => event.type === 'set_param' && event.slot === 'fader')
      .map((event) => event.value);
    expect(gains[0]).toBeCloseTo(0.8, 5);
    expect(gains[gains.length - 1]).toBeCloseTo(0.8, 5);
    expect(gains).toContain(0.4);
  });

  // Runs against the built pkg, so a wasm predating `frame` fails here.
  it('keeps the recorded frame on an event the edit did not touch', () => {
    const events: SessionEvent[] = [
      {
        elapsed_ms: 1000,
        type: 'set_param',
        deck: 'A',
        slot: 'fader',
        param: 'gain',
        value: 0.8,
        frame: 44100
      },
      {
        elapsed_ms: 20000,
        type: 'set_param',
        deck: 'A',
        slot: 'eq',
        param: 'low',
        value: 3,
        frame: 882000
      }
    ];
    const out = spliceLaneEvents(events, 'gain', CLASSIC, 'A', 5000, 8000, [
      { ms: 5000, value: 0.4 },
      { ms: 6000, value: 0.4 }
    ]);
    const untouched = out.find((event) => event.elapsed_ms === 20000);
    expect(untouched?.frame).toBe(882000);
  });

  it('gives an event the edit created no frame', () => {
    const events: SessionEvent[] = [
      {
        elapsed_ms: 1000,
        type: 'set_param',
        deck: 'A',
        slot: 'fader',
        param: 'gain',
        value: 0.8,
        frame: 44100
      }
    ];
    const out = spliceLaneEvents(events, 'gain', CLASSIC, 'A', 5000, 8000, [
      { ms: 5000, value: 0.4 },
      { ms: 6000, value: 0.4 }
    ]);
    const created = out.filter((event) => event.elapsed_ms >= 5000 && event.elapsed_ms <= 8000);
    expect(created.length).toBeGreaterThan(0);
    for (const event of created) expect(event.frame).toBeUndefined();
  });

  it('toggles a filter-active range and reads it back', () => {
    const out = toggleFilterActiveRange([], 'A', 1000, 4000);
    expect(filterActiveAt(out, 'A', 2000)).toBe(true);
    expect(filterActiveAt(out, 'A', 5000)).toBe(false);
  });
});

describe('reference-preserving no-ops', () => {
  it('deleteNudgeRange returns the input reference when nothing matches', () => {
    const events = simpleSession();
    expect(deleteNudgeRange(events, 'A', 1000, 2000)).toBe(events);
  });

  it('relocateEventPaths returns the input reference for an unmapped set', () => {
    const events = simpleSession();
    expect(relocateEventPaths(events, { '/never/there.mp3': '/new.mp3' })).toBe(events);
    const relocated = relocateEventPaths(events, { '/tracks/one.mp3': '/moved/one.mp3' });
    expect(relocated).not.toBe(events);
    expect(relocated[0].path).toBe('/moved/one.mp3');
  });
});

describe('shared constants', () => {
  it('exposes the shared edit constants', () => {
    const constants = editConstants();
    expect(constants.eqMinDb).toBeLessThan(constants.eqMaxDb);
    expect(constants.filterDeadZone).toBeGreaterThan(0);
    expect(constants.defaultMasterGain).toBeGreaterThan(0);
    expect(constants.defaultMasterGain).toBeLessThanOrEqual(1);
    expect(constants.minBlockMs).toBeGreaterThan(0);
    expect(constants.minGestureMs).toBeGreaterThan(0);
  });

  it('returns the same cached object rather than re-crossing the boundary', () => {
    expect(editConstants()).toBe(editConstants());
  });

  it('session-core supplies a spec for every editable lane, and no others', () => {
    const specs = laneSpecs(CLASSIC);
    expect(Object.keys(specs).sort()).toEqual([...ALL_LANE_KEYS].sort());
    for (const key of ALL_LANE_KEYS) {
      const spec = specs[key];
      expect(spec.key).toBe(key);
      expect(spec.max).toBeGreaterThan(spec.min);
      expect(spec.epsilon).toBeGreaterThan(0);
      expect(spec.defaultValue).toBeGreaterThanOrEqual(spec.min);
      expect(spec.defaultValue).toBeLessThanOrEqual(spec.max);
    }
  });

  it('carries the display metadata the timeline draws lanes from', () => {
    const specs = laneSpecs(CLASSIC);
    for (const key of ALL_LANE_KEYS) {
      expect(specs[key].shortLabel, `no short label for ${key}`).toBeTruthy();
      expect(specs[key].laneGroup).toBeGreaterThanOrEqual(0);
    }
    expect(new Set(ALL_LANE_KEYS.map((key) => specs[key].shortLabel)).size).toBe(
      ALL_LANE_KEYS.length
    );
    expect(specs.eqLow.laneGroup).toBe(specs.eqHigh.laneGroup);
    expect(specs.gain.laneGroup).not.toBe(specs.filter.laneGroup);
  });

  it('the eq lane range is the same one the mixer constants publish', () => {
    const constants = editConstants();
    for (const key of ['eqLow', 'eqMid', 'eqHigh'] as const) {
      expect(laneSpecFor(key, CLASSIC).min).toBe(constants.eqMinDb);
      expect(laneSpecFor(key, CLASSIC).max).toBe(constants.eqMaxDb);
    }
  });

  // The timeline draws and clamps against these, so a session recorded on the
  // isolator must not be drawn with the classic mixer's dB range.
  it('lane specs follow the mixer they are asked for', () => {
    for (const key of ['eqLow', 'eqMid', 'eqHigh'] as const) {
      const isolator = laneSpecFor(key, ISOLATOR);
      expect(isolator.min).toBe(0);
      expect(isolator.max).toBe(1);
      expect(isolator.defaultValue).toBe(1);
      expect(isolator.unit).toBe('normalized');
      expect(laneSpecFor(key, CLASSIC).unit).toBe('db');
    }

    // Transport and master lanes are mixer-independent.
    for (const key of ['rate', 'masterGain', 'gain', 'filter'] as const) {
      expect(laneSpecFor(key, ISOLATOR)).toEqual(laneSpecFor(key, CLASSIC));
    }
  });

  it('falls back to the classic mixer for a lane spec on an unknown id', () => {
    expect(laneSpecs('no-such-mixer')).toEqual(laneSpecs(CLASSIC));
  });

  it("publishes each mixer's own param ranges", () => {
    const classic = mixerParams('classic-3band');
    const isolator = mixerParams('isolator-3band');
    const constants = editConstants();

    expect(classic['eq/low'].min).toBe(constants.eqMinDb);
    expect(classic['eq/low'].max).toBe(constants.eqMaxDb);
    expect(classic['eq/low'].defaultValue).toBe(0);

    expect(isolator['eq/low'].min).toBe(0);
    expect(isolator['eq/low'].max).toBe(1);
    expect(isolator['eq/low'].defaultValue).toBe(1);

    for (const params of [classic, isolator]) {
      expect(params['fader/gain'].defaultValue).toBe(1);
      expect(params['filter/value'].min).toBe(-1);
    }
  });

  // Falling back keeps a build that dropped a mixer usable, and matches what
  // the engine loads for the same unknown id.
  it('falls back to the classic mixer for an unknown id', () => {
    expect(mixerParams('no-such-mixer')).toEqual(mixerParams('classic-3band'));
  });

  it('only the rate lane takes a caller-supplied range', () => {
    const specs = laneSpecs(CLASSIC);
    expect(laneSpecFor('rate', CLASSIC)).toEqual(specs.rate);
    expect(laneSpecFor('rate', CLASSIC, { rateMin: 0.5, rateMax: 1.5 })).toEqual({
      ...specs.rate,
      min: 0.5,
      max: 1.5
    });
    expect(laneSpecFor('gain', CLASSIC, { rateMin: 0.5, rateMax: 1.5 })).toEqual(specs.gain);
  });

  it('currentBeat mirrors the engine beat math', () => {
    expect(currentBeat(1, 0, 120)).toBeCloseTo(2, 9);
    expect(currentBeat(2.5, 0.5, 120)).toBeCloseTo(4, 9);
    expect(currentBeat(10, 0, 0)).toBe(0);
  });
});
