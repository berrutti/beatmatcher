import { describe, it, expect } from 'vitest';
import { portEvents } from '../bmsCompatibility';
import { bmsVersion } from '../sessionCore';
import type { SessionEvent } from '../types';

describe('portEvents', () => {
  // The exact shapes read out of a session recorded 2025-07-18, whose mixer moves
  // drew no lanes because the timeline recognizes a param by its event type.
  it('rewrites the version 1 vocabulary onto classic slots', () => {
    const events: SessionEvent[] = [
      { elapsed_ms: 1, type: 'set_volume', deck: 'A', gain: 0 },
      { elapsed_ms: 2, type: 'set_eq', deck: 'B', band: 'low', db: -2.5 },
      { elapsed_ms: 3, type: 'set_filter', deck: 'B', value: 0.35 },
      { elapsed_ms: 4, type: 'set_filter_active', deck: 'B', active: true }
    ];

    expect(portEvents(events, 1)).toEqual([
      {
        elapsed_ms: 1,
        type: 'set_param',
        deck: 'A',
        gain: 0,
        slot: 'fader',
        param: 'gain',
        value: 0
      },
      {
        elapsed_ms: 2,
        type: 'set_param',
        deck: 'B',
        band: 'low',
        db: -2.5,
        slot: 'eq',
        param: 'low',
        value: -2.5
      },
      { elapsed_ms: 3, type: 'set_param', deck: 'B', slot: 'filter', param: 'value', value: 0.35 },
      {
        elapsed_ms: 4,
        type: 'set_param',
        deck: 'B',
        active: true,
        slot: 'filter',
        param: 'active',
        value: 1
      }
    ]);
  });

  it('maps an inactive filter to zero rather than dropping it', () => {
    const [ported] = portEvents(
      [{ elapsed_ms: 1, type: 'set_filter_active', deck: 'A', active: false }],
      1
    );
    expect(ported.value).toBe(0);
    expect(ported.type).toBe('set_param');
  });

  it('ports nothing at the current version', () => {
    const events: SessionEvent[] = [{ elapsed_ms: 1, type: 'set_volume', deck: 'A', gain: 0.5 }];
    expect(portEvents(events, bmsVersion())).toEqual(events);
  });

  it('leaves the current vocabulary untouched', () => {
    const events: SessionEvent[] = [
      { elapsed_ms: 1, type: 'set_param', deck: 'A', slot: 'eq', param: 'low', value: -6 },
      { elapsed_ms: 2, type: 'play', deck: 'A' },
      { elapsed_ms: 3, type: 'set_nudge', deck: 'A', percent: 4 }
    ];
    expect(portEvents(events, 1)).toEqual(events);
  });

  it('does not port a version 1 event that is missing its value or deck', () => {
    const events: SessionEvent[] = [
      { elapsed_ms: 1, type: 'set_volume', deck: 'A' },
      { elapsed_ms: 2, type: 'set_eq', deck: 'A', db: -3 },
      { elapsed_ms: 3, type: 'set_volume', gain: 0.5 }
    ];
    expect(portEvents(events, 1).map((event) => event.type)).toEqual([
      'set_volume',
      'set_eq',
      'set_volume'
    ]);
  });
});
