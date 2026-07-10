// Composes the timeline's scene: turns the session data + current interaction
// state into the ordered list of SceneItems the engine renders and hit-tests.
// This is "the timeline knows which components to render." It's a pure function
// (rebuilt each frame from reactive inputs) and also reports the content height
// so the camera can clamp vertical scroll.

import type { SceneItem, ViewContext } from '@renderer/utils/timelineEngine';
import {
  MASTER_ROW_H,
  TrackWaveform,
  type LaneKey,
  type RowLayout
} from '@renderer/utils/timelineDraw';
import {
  computeRowLayout,
  selectionSpansFor,
  type ClipSelectionRef
} from '@renderer/utils/timelineLayout';
import {
  tickRowItem,
  deckChromeItem,
  clipBandItem,
  nudgeItem,
  laneSurfaceItem,
  filterRegionItem,
  filterSelectionItem,
  laneSeparatorItem,
  waveformSeparatorItem,
  masterItem,
  rowDividersItem,
  playheadItem,
  overviewItem,
  frameGuttersItem
} from '@renderer/utils/timelineItems';
import type { DeckId } from '@renderer/stores/decks';
import type { Clip, LoadedSpan, DeckLanes, MasterLanes, NudgeSpan } from '@renderer/utils/types';

export type SceneInput = {
  vc: ViewContext;
  decks: readonly DeckId[];
  clips: Clip[];
  loadedSpans: LoadedSpan[];
  deckLanes: Record<string, DeckLanes>;
  masterLanes: MasterLanes;
  deckNudges: Record<string, NudgeSpan[]>;
  waveforms: Map<string, TrackWaveform>;
  playheadMs: number;
  durationMs: number;
  editMode: boolean;
  laneFor: (deck: string) => LaneKey;
  laneHeight: number;
  waveformHeight: number;
  accentFor: (deck: string) => string;
  audibleFor: (deck: string) => boolean;
  soloFor: (deck: string) => boolean;
  mutedFor: (deck: string) => boolean;
  clipSelection: ClipSelectionRef[];
  filterSelection: { deck: string; startMs: number; endMs: number } | null;
  // Active gesture/selection previews, drawn on top of the rows.
  overlays?: SceneItem[];
};

export type SceneResult = {
  items: SceneItem[];
  rows: RowLayout[];
  contentHeight: number;
};

export function buildScene(input: SceneInput): SceneResult {
  const { vc, decks } = input;
  const deckSpecs = decks.map((deckId) => ({
    deckId,
    laneHeights: input.editMode ? [{ key: input.laneFor(deckId), height: input.laneHeight }] : []
  }));
  // The master lane sits at the top, directly below the time ruler and above
  // every deck row; the deck rows begin below it.
  const masterTop = vc.laneOriginY;
  const masterHeight = MASTER_ROW_H;
  const rows = computeRowLayout(deckSpecs, masterTop + masterHeight, input.waveformHeight);

  const items: SceneItem[] = [];

  items.push(masterItem(masterTop, masterHeight, input.masterLanes));

  rows.forEach((row, rowIndex) => {
    const deck = row.deckId;
    items.push(
      deckChromeItem(row, rowIndex, {
        accent: input.accentFor(deck),
        audible: input.audibleFor(deck),
        solo: input.soloFor(deck),
        muted: input.mutedFor(deck)
      })
    );
    items.push(
      clipBandItem(
        row,
        input.clips,
        input.loadedSpans,
        input.waveforms,
        input.accentFor(deck),
        input.audibleFor(deck),
        input.editMode ? selectionSpansFor(input.clipSelection, deck) : []
      )
    );
    for (const span of input.deckNudges[deck] ?? []) {
      items.push(nudgeItem(row, span, deck));
    }
    const lane = row.lanes[0];
    if (lane) {
      items.push(laneSurfaceItem(lane, deck, input.deckLanes[deck]));
      if (lane.key === 'filter') {
        for (const span of input.deckLanes[deck]?.filterActive ?? []) {
          items.push(filterRegionItem(lane, deck, span));
        }
        const sel = input.filterSelection;
        if (sel && sel.deck === deck) {
          items.push(filterSelectionItem(lane, sel.startMs, sel.endMs));
        }
      }
      items.push(laneSeparatorItem(lane, deck));
      items.push(waveformSeparatorItem(row, deck));
    }
  });

  const lastRow = rows[rows.length - 1];
  const contentBottom = lastRow ? lastRow.top + lastRow.height : masterTop + masterHeight;
  const bottomY = Math.min(contentBottom, vc.scrollViewport.bottom);
  items.push(playheadItem(input.playheadMs, bottomY));

  if (input.overlays) items.push(...input.overlays);

  items.push(rowDividersItem(rows));

  items.push(
    overviewItem(
      input.durationMs || 1,
      input.clips,
      input.playheadMs,
      Object.fromEntries(decks.map((id) => [id, input.accentFor(id)]))
    )
  );

  items.push(frameGuttersItem());

  // Drawn last so vertical scroll (which can push the master row's top above
  // TICK_H, see masterTop above) never paints over the ruler.
  items.push(tickRowItem());

  const contentHeight = rows.reduce((sum, row) => sum + row.height, 0) + masterHeight;

  return { items, rows, contentHeight };
}
