import { describe, it, expect } from 'vitest';
import {
  toggleLane,
  lanesForDeck,
  lanesForMaster,
  lanesOnMixer,
  DEFAULT_DECK_LANES,
  DEFAULT_MASTER_LANES
} from '@renderer/utils/laneSelection';
import { laneSpecs } from '@renderer/utils/sessionCore';
import { DECK_LANE_KEYS, MASTER_LANE_KEYS, type EditableLaneKey } from '@renderer/utils/types';

describe('toggleLane', () => {
  it('adds a lane that was not selected', () => {
    expect(toggleLane(DECK_LANE_KEYS, ['filter'], 'gain')).toEqual(['gain', 'filter']);
  });

  it('removes a lane that was', () => {
    expect(toggleLane(DECK_LANE_KEYS, ['gain', 'filter'], 'filter')).toEqual(['gain']);
  });

  it('stacks lanes in a canonical order however they were toggled', () => {
    const byOneOrder = toggleLane(
      DECK_LANE_KEYS,
      toggleLane(DECK_LANE_KEYS, ['jog'], 'gain'),
      'eqHighMid'
    );
    const byAnother = toggleLane(
      DECK_LANE_KEYS,
      toggleLane(DECK_LANE_KEYS, ['eqHighMid'], 'jog'),
      'gain'
    );

    expect(byOneOrder).toEqual(byAnother);
    expect(byOneOrder).toEqual(['gain', 'eqHighMid', 'jog']);
  });

  it('refuses to turn off the last lane, so a row always has one to click on', () => {
    expect(toggleLane(DECK_LANE_KEYS, ['filter'], 'filter')).toEqual(['filter']);
    expect(toggleLane(MASTER_LANE_KEYS, ['xfader'], 'xfader')).toEqual(['xfader']);
  });

  it('returns a new array, so the caller can compare by reference', () => {
    const current = ['filter'] as const;
    expect(toggleLane(DECK_LANE_KEYS, current, 'gain')).not.toBe(current);
    expect(toggleLane(DECK_LANE_KEYS, current, 'filter')).not.toBe(current);
  });
});

describe('lanesForDeck', () => {
  it('reads a lane list stored by this version', () => {
    expect(lanesForDeck({ A: ['gain', 'jog'] }, 'A')).toEqual(['gain', 'jog']);
  });

  it('ports a single lane stored before lanes could stack', () => {
    expect(lanesForDeck({ A: 'filter' }, 'A')).toEqual(['filter']);
  });

  it('falls back to the default for a deck with no entry', () => {
    expect(lanesForDeck({}, 'A')).toEqual(DEFAULT_DECK_LANES);
  });

  it('restores the default for a deck stored empty by an older version', () => {
    expect(lanesForDeck({ A: [] }, 'A')).toEqual(DEFAULT_DECK_LANES);
  });

  it('drops a stored key that is no longer a lane', () => {
    expect(lanesForDeck({ A: ['gain', 'nonsense'] }, 'A')).toEqual(['gain']);
  });
});

describe('lanesForMaster', () => {
  it('ports the single lane stored before the master row could stack', () => {
    expect(lanesForMaster('xfader')).toEqual(['xfader']);
  });

  it('falls back to the default when nothing is stored', () => {
    expect(lanesForMaster(undefined)).toEqual(DEFAULT_MASTER_LANES);
  });

  it('never offers a deck lane on the master row', () => {
    expect(lanesForMaster(['filter', 'xfader'])).toEqual(['xfader']);
  });
});

describe('lanesOnMixer', () => {
  function existsOn(mixerId: string): (lane: EditableLaneKey) => boolean {
    return (lane) => laneSpecs(mixerId)[lane].available;
  }

  it('offers no low-mid lane on the three-band mixer', () => {
    expect(lanesOnMixer(DECK_LANE_KEYS, existsOn('classic-3band'), DEFAULT_DECK_LANES)).toEqual(
      DECK_LANE_KEYS.filter((lane) => lane !== 'eqLowMid')
    );
  });

  it('offers every lane on the four-band mixer', () => {
    expect(lanesOnMixer(DECK_LANE_KEYS, existsOn('classic-4band'), DEFAULT_DECK_LANES)).toEqual([
      ...DECK_LANE_KEYS
    ]);
  });

  it('falls back to the default when every chosen lane is missing', () => {
    expect(lanesOnMixer(['eqLowMid'], existsOn('classic-3band'), DEFAULT_DECK_LANES)).toEqual(
      DEFAULT_DECK_LANES
    );
  });

  it('keeps the master lanes on both mixers', () => {
    for (const mixerId of ['classic-3band', 'classic-4band']) {
      expect(lanesOnMixer(MASTER_LANE_KEYS, existsOn(mixerId), DEFAULT_MASTER_LANES)).toEqual([
        ...MASTER_LANE_KEYS
      ]);
    }
  });
});
