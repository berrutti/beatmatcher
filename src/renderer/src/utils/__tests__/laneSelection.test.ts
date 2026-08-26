import { describe, it, expect } from 'vitest';
import { toggleLane, lanesForDeck, DEFAULT_DECK_LANES } from '@renderer/utils/laneSelection';

describe('toggleLane', () => {
  it('adds a lane that was not selected', () => {
    expect(toggleLane(['filter'], 'gain')).toEqual(['gain', 'filter']);
  });

  it('removes a lane that was', () => {
    expect(toggleLane(['gain', 'filter'], 'filter')).toEqual(['gain']);
  });

  it('stacks lanes in a canonical order however they were toggled', () => {
    const byOneOrder = toggleLane(toggleLane(['jog'], 'gain'), 'eqMid');
    const byAnother = toggleLane(toggleLane(['eqMid'], 'jog'), 'gain');

    expect(byOneOrder).toEqual(byAnother);
    expect(byOneOrder).toEqual(['gain', 'eqMid', 'jog']);
  });

  it('lets the last lane be turned off, so a deck can show only its waveform', () => {
    expect(toggleLane(['filter'], 'filter')).toEqual([]);
  });

  it('returns a new array, so the caller can compare by reference', () => {
    const current: ReturnType<typeof toggleLane> = ['filter'];
    expect(toggleLane(current, 'gain')).not.toBe(current);
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

  it('keeps an explicitly emptied deck empty rather than restoring the default', () => {
    expect(lanesForDeck({ A: [] }, 'A')).toEqual([]);
  });

  it('drops a stored key that is no longer a lane', () => {
    expect(lanesForDeck({ A: ['gain', 'nonsense'] }, 'A')).toEqual(['gain']);
  });
});
