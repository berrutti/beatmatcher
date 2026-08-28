import type { Hit } from '@renderer/utils/timelineEngine';

// The engine stays generic over target names, so the vocabulary is pinned here.
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
  // Above the lane separator, which spans the full width: if it won, an element
  // overlapping it could never be reached.
  'filterRegion:body',
  // Sits on the clip band rather than beside it, so it has to beat the clip body.
  'waveformSeparator',
  'clip:body',
  // Over the lane's own empty bottom, so it can sink below the elements above it.
  'laneSeparator',
  // Below both separators where they cross the label column, so a drag there
  // resizes rather than opening the picker.
  'laneDropdown',
  'deckLabel',
  'lane',
  'clipBand'
];

// Plain string keys: a lookup can ask about a target this table does not rank.
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
