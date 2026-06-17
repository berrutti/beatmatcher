import type { Clip } from '@renderer/composables/useSessionTimeline';
import type { DeckId } from '@renderer/stores/decks';
import { ROW_H, type LaneKey, type RowLayout, type SublaneLayout } from './timelineDraw';

// Pure geometry for the session timeline: everything draw() needs to know
// about where things sit, computed without a canvas so it can be unit tested.

export type LaneHeight = { key: LaneKey; height: number };

export function computeRowLayout(
  decks: { deckId: DeckId; laneHeights: LaneHeight[] }[],
  topY: number
): RowLayout[] {
  const rows: RowLayout[] = [];
  let rowY = topY;
  for (const { deckId, laneHeights } of decks) {
    let sublaneTop = rowY + ROW_H;
    const lanes: SublaneLayout[] = laneHeights.map(({ key, height }) => {
      const sublane = { key, top: sublaneTop, height };
      sublaneTop += height;
      return sublane;
    });
    rows.push({ deckId, top: rowY, height: sublaneTop - rowY, lanes });
    rowY = sublaneTop;
  }
  return rows;
}

// The lane whose bottom edge (the separator below it) is within grab range of y,
// or null. Each separator resizes the lane above it.
export function findLaneSeparator(rows: RowLayout[], y: number, grabPx: number): LaneKey | null {
  for (const row of rows) {
    for (const sub of row.lanes) {
      if (Math.abs(y - (sub.top + sub.height)) <= grabPx) return sub.key;
    }
  }
  return null;
}

export type LaneRect = { top: number; height: number };

export function selectedLaneRect(
  sel: { deck: string; lane: string } | null,
  rows: RowLayout[],
  masterRect: LaneRect | null
): LaneRect | null {
  if (!sel) return null;
  if (sel.deck === 'master') return masterRect;
  const row = rows.find((candidate) => candidate.deckId === sel.deck);
  const sublane = row?.lanes.find((lane) => lane.key === sel.lane);
  return sublane ? { top: sublane.top, height: sublane.height } : null;
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

export type ClipSelectionRef = {
  deck: string;
  blockId: number;
  iterationStartMs: number | null;
};

export function selectionSpanFor(
  selection: ClipSelectionRef | null,
  clips: Clip[],
  deckId: string
): { startMs: number; endMs: number } | null {
  if (!selection || selection.deck !== deckId) return null;
  let startMs = Infinity;
  let endMs = -Infinity;
  for (const clip of clips) {
    if (clip.deck !== deckId || clip.blockId !== selection.blockId) continue;
    if (selection.iterationStartMs !== null && clip.sessionStartMs !== selection.iterationStartMs) {
      continue;
    }
    startMs = Math.min(startMs, clip.sessionStartMs);
    endMs = Math.max(endMs, clip.sessionEndMs);
  }
  return startMs <= endMs ? { startMs, endMs } : null;
}
