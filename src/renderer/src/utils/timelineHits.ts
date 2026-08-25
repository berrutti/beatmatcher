// Listed front to back: earlier wins when several items claim the same point.

import type { Hit } from '@renderer/utils/timelineEngine';

const HIT_PRECEDENCE: readonly string[] = [
  'overview',
  'laneDropdown',
  // Resize/trim edges beat the body of their OWN element.
  'filterRegion:start',
  'filterRegion:end',
  'clip:start',
  'clip:end',
  // Elements you click. They all sit ABOVE the lane separator: a separator can
  // still be grabbed by moving off the element (it spans the full width), but if
  // the separator won you could never reach an element overlapping it. The nudge
  // sits above the waveform (clip body) and the lane limit per the same logic.
  'filterRegion:body',
  'nudgeSpan',
  // The waveform separator sits ON the clip band (clips cover the waveform), so
  // unlike the lane separator it must beat the clip body to stay grabbable.
  'waveformSeparator',
  'clip:body',
  // The lane separator sits over the (often empty) lane bottom, so it can sink
  // below the elements: move off an element to grab it.
  'laneSeparator',
  'lane',
  'clipBand',
  'tickRow'
];

const RANK = new Map(HIT_PRECEDENCE.map((key, i) => [key, HIT_PRECEDENCE.length - i]));

// Higher number = higher priority (so the engine's `>=` tie-break keeps top-most
// draw order for equal ranks). Tries `target:part`, then `target`, else 0.
export function hitPriority(hit: Hit): number {
  if (hit.part) {
    const withPart = RANK.get(`${hit.target}:${hit.part}`);
    if (withPart !== undefined) return withPart;
  }
  return RANK.get(hit.target) ?? 0;
}
