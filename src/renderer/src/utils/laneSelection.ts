import {
  DECK_LANE_KEYS,
  MASTER_LANE_KEYS,
  type DeckLaneKey,
  type MasterLaneKey
} from '@renderer/utils/types';

export const DEFAULT_DECK_LANES: DeckLaneKey[] = ['filter'];
export const DEFAULT_MASTER_LANES: MasterLaneKey[] = ['masterGain'];

// Canonical order, so a stack reads the same however its lanes were switched on.
function inCanonicalOrder<Key extends string>(all: readonly Key[], lanes: Key[]): Key[] {
  return all.filter((key) => lanes.includes(key));
}

// A row with no lanes has no way back: its label column would offer a picker with
// nothing ticked and no lane to click on.
export function toggleLane<Key extends string>(
  all: readonly Key[],
  current: readonly Key[],
  lane: Key
): Key[] {
  const without = current.filter((key) => key !== lane);
  if (without.length === current.length) return inCanonicalOrder(all, [...current, lane]);
  return without.length === 0 ? [...current] : without;
}

// Reads what a row had. A version that could only show one lane stored a bare key,
// so that shape is ported rather than discarded.
function lanesFrom<Key extends string>(
  all: readonly Key[],
  stored: unknown,
  fallback: readonly Key[]
): Key[] {
  const isKey = (value: unknown): value is Key =>
    typeof value === 'string' && all.some((key) => key === value);
  if (stored === undefined) return [...fallback];
  if (isKey(stored)) return [stored];
  if (!Array.isArray(stored)) return [...fallback];
  const kept = inCanonicalOrder(all, stored.filter(isKey));
  return kept.length === 0 ? [...fallback] : kept;
}

export function lanesForDeck(stored: Record<string, unknown>, deck: string): DeckLaneKey[] {
  return lanesFrom(DECK_LANE_KEYS, stored[deck], DEFAULT_DECK_LANES);
}

export function lanesForMaster(stored: unknown): MasterLaneKey[] {
  return lanesFrom(MASTER_LANE_KEYS, stored, DEFAULT_MASTER_LANES);
}
