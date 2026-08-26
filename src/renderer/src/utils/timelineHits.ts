// Listed front to back: earlier wins when several items claim the same point.

import type { Hit } from '@renderer/utils/timelineEngine';

// The targets the timeline's own items emit. The engine stays generic over
// target names, so this is where the vocabulary is pinned.
export const HIT_TARGETS = [
  'overview',
  'laneDropdown',
  'deckLabel',
  'filterRegion',
  'clip',
  'waveformSeparator',
  'laneSeparator',
  'lane',
  'clipBand'
] as const;

export type HitTarget = (typeof HIT_TARGETS)[number];

// Some targets are ranked only per part, because no item emits them bare.
export const HIT_PRECEDENCE: readonly (HitTarget | `${HitTarget}:${string}`)[] = [
  'overview',
  // Resize/trim edges beat the body of their OWN element.
  'filterRegion:start',
  'filterRegion:end',
  'clip:start',
  'clip:end',
  // Elements you click. They all sit ABOVE the lane separator: a separator can
  // still be grabbed by moving off the element (it spans the full width), but if
  // the separator won you could never reach an element overlapping it.
  'filterRegion:body',
  // The waveform separator sits ON the clip band (clips cover the waveform), so
  // unlike the lane separator it must beat the clip body to stay grabbable.
  'waveformSeparator',
  'clip:body',
  // The lane separator sits over the (often empty) lane bottom, so it can sink
  // below the elements: move off an element to grab it.
  'laneSeparator',
  // Below both separators, which cross it in the label column: a drag there
  // resizes the lane rather than opening the picker. Nothing else reaches that
  // column, so this costs the dropdown nothing.
  'laneDropdown',
  'deckLabel',
  'lane',
  'clipBand'
];

// Keyed by plain string: the engine is generic over target names, so a lookup
// can legitimately ask about one this table does not rank.
const RANK = new Map<string, number>(
  HIT_PRECEDENCE.map((key, i) => [key, HIT_PRECEDENCE.length - i])
);

// Higher number = higher priority, so the engine's `>=` tie-break keeps top-most
// draw order for equal ranks.
export function hitPriority(hit: Hit): number {
  if (hit.part) {
    const withPart = RANK.get(`${hit.target}:${hit.part}`);
    if (withPart !== undefined) return withPart;
  }
  return RANK.get(hit.target) ?? 0;
}
