import { DECK_LANE_KEYS } from '@renderer/utils/types';

export type DeckLaneKey = (typeof DECK_LANE_KEYS)[number];

export const DEFAULT_DECK_LANES: DeckLaneKey[] = ['filter'];

function isLaneKey(value: unknown): value is DeckLaneKey {
  return typeof value === 'string' && DECK_LANE_KEYS.some((key) => key === value);
}

// Canonical order, so a stack reads the same however its lanes were switched on.
function inCanonicalOrder(lanes: DeckLaneKey[]): DeckLaneKey[] {
  return DECK_LANE_KEYS.filter((key) => lanes.includes(key));
}

export function toggleLane(current: readonly DeckLaneKey[], lane: DeckLaneKey): DeckLaneKey[] {
  const without = current.filter((key) => key !== lane);
  if (without.length !== current.length) return without;
  return inCanonicalOrder([...current, lane]);
}

// Reads what a deck had. A version that could only show one lane stored a bare
// key, so that shape is ported rather than discarded.
export function lanesForDeck(stored: Record<string, unknown>, deck: string): DeckLaneKey[] {
  const entry = stored[deck];
  if (entry === undefined) return [...DEFAULT_DECK_LANES];
  if (isLaneKey(entry)) return [entry];
  if (!Array.isArray(entry)) return [...DEFAULT_DECK_LANES];
  return inCanonicalOrder(entry.filter(isLaneKey));
}
