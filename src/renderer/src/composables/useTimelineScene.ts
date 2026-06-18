// Composes the timeline's scene: turns the session data + current interaction
// state into the ordered list of SceneItems the engine renders and hit-tests.
// This is "the timeline knows which components to render." It's a pure function
// (rebuilt each frame from reactive inputs) and also reports the content height
// so the camera can clamp vertical scroll.

import type { SceneItem, ViewContext } from '@renderer/utils/timelineEngine';
import { MASTER_ROW_H, type LaneKey, type RowLayout } from '@renderer/utils/timelineDraw';
import {
  computeRowLayout,
  selectionSpanFor,
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
  scrollbarItem,
  overviewItem,
  frameGuttersItem
} from '@renderer/utils/timelineItems';
import type { DeckId } from '@renderer/stores/decks';
import type {
  Clip,
  LoadedSpan,
  DeckLanes,
  MasterLanes,
  NudgeSpan,
  TrackWaveform
} from '@renderer/composables/useSessionTimeline';

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
  clipSelection: ClipSelectionRef | null;
  filterSelection: { deck: string; startMs: number; endMs: number } | null;
  scrollY: number;
  maxScrollY: number;
  // Active gesture/selection previews, drawn on top of the rows.
  overlays?: SceneItem[];
};

export type SceneResult = {
  items: SceneItem[];
  rows: RowLayout[];
  contentHeight: number;
  masterTop: number;
  masterHeight: number;
};

export function buildScene(input: SceneInput): SceneResult {
  const { vc, decks } = input;
  const deckSpecs = decks.map((deckId) => ({
    deckId,
    laneHeights: input.editMode ? [{ key: input.laneFor(deckId), height: input.laneHeight }] : []
  }));
  const rows = computeRowLayout(deckSpecs, vc.laneOriginY, input.waveformHeight);

  const items: SceneItem[] = [tickRowItem()];

  rows.forEach((row, ri) => {
    const deck = row.deckId;
    items.push(
      deckChromeItem(row, ri, {
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
        input.editMode ? selectionSpanFor(input.clipSelection, input.clips, deck) : null
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
  const masterTop = lastRow ? lastRow.top + lastRow.height : vc.laneOriginY;
  const masterHeight = MASTER_ROW_H;
  items.push(masterItem(masterTop, masterHeight, input.masterLanes));

  const bottomY = Math.min(masterTop + masterHeight, vc.scrollViewport.bottom);
  items.push(playheadItem(input.playheadMs, bottomY));

  if (input.overlays) items.push(...input.overlays);

  items.push(rowDividersItem(rows));

  const scrollbar = scrollbarItem(input.scrollY, input.maxScrollY);
  if (scrollbar) items.push(scrollbar);

  items.push(
    overviewItem(
      input.durationMs || 1,
      input.clips,
      input.playheadMs,
      Object.fromEntries(decks.map((id) => [id, input.accentFor(id)]))
    )
  );

  items.push(frameGuttersItem());

  const contentHeight = rows.reduce((sum, r) => sum + r.height, 0) + masterHeight;

  return { items, rows, contentHeight, masterTop, masterHeight };
}
