// The timeline's hit vocabulary and the single, explicit precedence table.
//
// Items report a `Hit` with a `target` (which element) and optional `part`
// (which region of it). When several items claim the same point, the engine
// picks the one ranked highest here. Precedence is intentionally per-`target:part`
// so behaviour can differ by region: e.g. a filter region's resize EDGE beats a
// nudge, while its BODY does not. Edit this ONE list to change precedence; do not
// rely on draw order.
//
// Listed front (top) to back. Earlier = higher priority = wins.

import type { Hit } from '@renderer/utils/timelineEngine';

export const HIT_PRECEDENCE: readonly string[] = [
  // Always-on-top chrome.
  'scrollbar',
  'overview',
  // Label-column control.
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
  'clip:body',
  // Separators come AFTER the elements, but still above the bare lane surface
  // (otherwise the lane draw surface would swallow every separator grab).
  'laneSeparator',
  // The automation lane drawing surface (draw value / shift-paint).
  'lane',
  // The deck's clip band (seek / shift-nudge) and master lane.
  'clipBand',
  'master',
  // Background ruler.
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
