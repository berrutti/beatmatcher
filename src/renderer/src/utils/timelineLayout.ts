import type { Clip, TransportBlock, WaveSegment } from '@renderer/utils/types';
import type { DeckId } from '@renderer/utils/types';
import { ROW_H, type RowLayout, type SublaneLayout } from './timelineDraw';
import type { DeckLaneKey } from '@renderer/utils/types';

// Pure geometry for the session timeline: everything draw() needs to know
// about where things sit, computed without a canvas so it can be unit tested.

export type LaneHeight = { key: DeckLaneKey; height: number };

// Lays a run of lanes end to end from `top`. Shared by the deck rows and the
// master row, which stack the same way and differ only in what precedes them.
export function stackLanes<Key extends string>(
  top: number,
  lanes: { key: Key; height: number }[]
): { key: Key; top: number; height: number }[] {
  let laneTop = top;
  return lanes.map(({ key, height }) => {
    const lane = { key, top: laneTop, height };
    laneTop += height;
    return lane;
  });
}

export function computeRowLayout(
  decks: { deckId: DeckId; waveformHeight?: number; laneHeights: LaneHeight[] }[],
  topY: number
): RowLayout[] {
  const rows: RowLayout[] = [];
  let rowY = topY;
  for (const { deckId, laneHeights, waveformHeight = ROW_H } of decks) {
    const lanes: SublaneLayout[] = stackLanes(rowY + waveformHeight, laneHeights);
    const last = lanes[lanes.length - 1];
    const bottom = last === undefined ? rowY + waveformHeight : last.top + last.height;
    rows.push({ deckId, top: rowY, height: bottom - rowY, waveformHeight, lanes });
    rowY = bottom;
  }
  return rows;
}

export type ClipGestureKind = 'move' | 'trim-start' | 'trim-end';

export function ghostSpan(
  span: { sessionStartMs: number; sessionEndMs: number },
  gesture: { kind: ClipGestureKind; deltaMs: number; targetMs: number }
): { startMs: number; endMs: number } {
  if (gesture.kind === 'move') {
    return {
      startMs: span.sessionStartMs + gesture.deltaMs,
      endMs: span.sessionEndMs + gesture.deltaMs
    };
  }
  if (gesture.kind === 'trim-start') {
    return { startMs: gesture.targetMs, endMs: span.sessionEndMs };
  }
  return { startMs: span.sessionStartMs, endMs: gesture.targetMs };
}

export function clipGestureDeltaSec(
  kind: ClipGestureKind,
  deltaMs: number,
  targetMs: number,
  blockStartMs: number,
  blockEndMs: number
): number {
  if (kind === 'move') return deltaMs / 1000;
  return (targetMs - (kind === 'trim-start' ? blockStartMs : blockEndMs)) / 1000;
}

// Wall-time, because blockIds reallocate on every rebuild.
export type ClipSelectionRef = {
  deck: string;
  startMs: number;
  endMs: number;
};

export function selectionSpansFor(
  selections: ClipSelectionRef[],
  deckId: string
): { startMs: number; endMs: number }[] {
  return selections
    .filter((selection) => selection.deck === deckId)
    .map(({ startMs, endMs }) => ({ startMs, endMs }));
}

// The BPM labels round to 0.1 (drawClipBpmLabels), so segments whose displayed
// tempo matches at that precision read as one region.
const BPM_REGION_EPSILON = 0.05;

function segmentBpm(clip: Clip, segment: WaveSegment): number | null {
  if (clip.bpm === null || clip.bpm <= 0) return null;
  const wallSec = (segment.wallEndMs - segment.wallStartMs) / 1000;
  const trackSpan = segment.trackEndSec - segment.trackStartSec;
  if (wallSec <= 0 || trackSpan <= 0) return null;
  return clip.bpm * (trackSpan / wallSec);
}

// The whole clip when the track has no grid, or when no segment contains `ms`.
export function bpmRegionSpanAt(clip: Clip, ms: number): { startMs: number; endMs: number } {
  const segments = clip.waveSegments;
  const index = segments.findIndex(
    (segment) => ms >= segment.wallStartMs && ms <= segment.wallEndMs
  );
  const target = index >= 0 ? segmentBpm(clip, segments[index]) : null;
  if (target === null) return { startMs: clip.sessionStartMs, endMs: clip.sessionEndMs };
  const matches = (segment: WaveSegment) => {
    const bpm = segmentBpm(clip, segment);
    return bpm !== null && Math.abs(bpm - target) < BPM_REGION_EPSILON;
  };
  let lo = index;
  while (lo > 0 && matches(segments[lo - 1])) lo--;
  let hi = index;
  while (hi < segments.length - 1 && matches(segments[hi + 1])) hi++;
  return { startMs: segments[lo].wallStartMs, endMs: segments[hi].wallEndMs };
}

// Clipped to each block the rect touches, so a partial overlap selects only
// the part inside it.
export function marqueeTargets(
  rows: { deckId: string; top: number; waveformHeight: number }[],
  blocksFor: (deck: string) => TransportBlock[],
  rect: { x0: number; x1: number; y0: number; y1: number },
  xToMs: (x: number) => number
): ClipSelectionRef[] {
  const startMs = xToMs(Math.min(rect.x0, rect.x1));
  const endMs = xToMs(Math.max(rect.x0, rect.x1));
  const yTop = Math.min(rect.y0, rect.y1);
  const yBottom = Math.max(rect.y0, rect.y1);
  const targets: ClipSelectionRef[] = [];
  for (const row of rows) {
    if (row.top + row.waveformHeight < yTop || row.top > yBottom) continue;
    for (const block of blocksFor(row.deckId)) {
      if (block.endMs <= startMs || block.startMs >= endMs) continue;
      targets.push({
        deck: row.deckId,
        startMs: Math.max(block.startMs, startMs),
        endMs: Math.min(block.endMs, endMs)
      });
    }
  }
  return targets;
}

// Overlapping or touching spans on the same deck merge into one delete range,
// so Delete issues a minimal set of edits.
export function mergeSelectionRanges(selections: ClipSelectionRef[]): ClipSelectionRef[] {
  const byDeck = new Map<string, ClipSelectionRef[]>();
  for (const selection of selections) {
    const list = byDeck.get(selection.deck);
    if (list) list.push(selection);
    else byDeck.set(selection.deck, [selection]);
  }
  const merged: ClipSelectionRef[] = [];
  for (const list of byDeck.values()) {
    list.sort((first, second) => first.startMs - second.startMs);
    let current = { ...list[0] };
    for (const selection of list.slice(1)) {
      if (selection.startMs <= current.endMs) {
        current.endMs = Math.max(current.endMs, selection.endMs);
      } else {
        merged.push(current);
        current = { ...selection };
      }
    }
    merged.push(current);
  }
  return merged;
}
