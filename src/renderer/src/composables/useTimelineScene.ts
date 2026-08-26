// Pure, and rebuilt each frame from reactive inputs. Also reports the content
// height, because only the composed scene knows how tall the rows came out.

import type { SceneItem, ViewContext } from '@renderer/utils/timelineEngine';
import { TrackWaveform, type LaneKey, type RowLayout } from '@renderer/utils/timelineDraw';
import {
  computeRowLayout,
  selectionSpansFor,
  type ClipSelectionRef
} from '@renderer/utils/timelineLayout';
import {
  tickRowItem,
  deckChromeItem,
  clipBandItem,
  jogLaneItem,
  laneSurfaceItem,
  filterRegionItem,
  filterSelectionItem,
  laneSeparatorItem,
  rowSeparatorItem,
  waveformSeparatorItem,
  masterItem,
  rowDividersItem,
  playheadItem,
  overviewItem,
  frameGuttersItem
} from '@renderer/utils/timelineItems';
import { MASTER_ROW_ID, type DeckId } from '@renderer/utils/types';
import type {
  Clip,
  LoadedSpan,
  DeckLanes,
  MasterLanes,
  MasterLaneKey,
  EditableLaneKey,
  LanePoint
} from '@renderer/utils/types';

export type SceneInput = {
  vc: ViewContext;
  decks: readonly DeckId[];
  clips: Clip[];
  loadedSpans: LoadedSpan[];
  deckLanes: Record<string, DeckLanes>;
  masterLanes: MasterLanes;
  deckJog: Record<string, LanePoint[]>;
  waveforms: Map<string, TrackWaveform>;
  playheadMs: number;
  durationMs: number;
  editMode: boolean;
  lanesFor: (deck: string) => LaneKey[];
  masterLane: MasterLaneKey;
  laneHeightFor: (deck: string, key: LaneKey) => number;
  waveformHeightFor: (deck: string) => number;
  openLaneFor: (deck: string) => string | null;
  badgeAlphaFor: (deck: string) => number;
  menuOpenFor: (deck: string) => boolean;
  // The span a "reset this move" would clear, shown while its menu is open.
  resetPreview: { deck: string; lane: EditableLaneKey; startMs: number; endMs: number } | null;
  accentFor: (deck: string) => string;
  // Named by the locale, so the timeline never spells a lane or a deck out itself.
  laneLabel: (key: EditableLaneKey) => string;
  deckLabel: (deck: string) => string;
  badgeLabel: (deck: string) => string;
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

function resetHighlight(
  preview: SceneInput['resetPreview'],
  deck: string,
  lane: EditableLaneKey
): { lane: EditableLaneKey; startMs: number; endMs: number } | null {
  if (!preview || preview.deck !== deck || preview.lane !== lane) return null;
  return { lane, startMs: preview.startMs, endMs: preview.endMs };
}

export function buildScene(input: SceneInput): SceneResult {
  const { vc, decks } = input;
  const deckSpecs = decks.map((deckId) => ({
    deckId,
    waveformHeight: input.waveformHeightFor(deckId),
    laneHeights: input.editMode
      ? input.lanesFor(deckId).map((key) => ({ key, height: input.laneHeightFor(deckId, key) }))
      : []
  }));
  // The master lane sits at the top, directly below the time ruler and above
  // every deck row. The deck rows begin below it.
  const masterTop = vc.laneOriginY;
  const masterHeight = input.waveformHeightFor(MASTER_ROW_ID);
  const rows = computeRowLayout(deckSpecs, masterTop + masterHeight);

  const items: SceneItem[] = [];

  items.push(
    masterItem(
      masterTop,
      masterHeight,
      input.masterLanes,
      input.masterLane,
      input.laneLabel(input.masterLane),
      input.openLaneFor(MASTER_ROW_ID) !== null,
      resetHighlight(input.resetPreview, MASTER_ROW_ID, input.masterLane)
    )
  );

  items.push(rowSeparatorItem(masterTop + masterHeight, MASTER_ROW_ID));

  rows.forEach((row) => {
    const deck = row.deckId;
    items.push(
      deckChromeItem(row, {
        accent: input.accentFor(deck),
        audible: input.audibleFor(deck),
        solo: input.soloFor(deck),
        muted: input.mutedFor(deck),
        deckLabel: input.deckLabel(deck),
        badgeLabel: input.badgeLabel(deck),
        laneLabel: input.laneLabel,
        badgeAlpha: input.badgeAlphaFor(deck),
        openLane: input.openLaneFor(deck),
        menuOpen: input.menuOpenFor(deck)
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
    const deckClips = input.clips.filter((clip) => clip.deck === deck);
    for (const lane of row.lanes) {
      if (lane.key === 'jog') {
        items.push(
          jogLaneItem(
            lane,
            deck,
            input.deckJog[deck] ?? [],
            deckClips,
            input.waveforms,
            input.accentFor(deck)
          )
        );
      } else {
        items.push(
          laneSurfaceItem(
            lane,
            deck,
            input.deckLanes[deck],
            deckClips,
            input.waveforms,
            input.accentFor(deck),
            resetHighlight(input.resetPreview, deck, lane.key)
          )
        );
      }
      if (lane.key === 'filter') {
        for (const span of input.deckLanes[deck]?.filterActive ?? []) {
          items.push(filterRegionItem(lane, deck, span));
        }
        const sel = input.filterSelection;
        if (sel && sel.deck === deck) {
          items.push(filterSelectionItem(lane, sel.startMs, sel.endMs));
        }
      }
      // One per lane, so a drag grows the lane it is on rather than the stack.
      items.push(laneSeparatorItem(lane, deck));
    }
    if (row.lanes.length > 0) items.push(waveformSeparatorItem(row, deck));
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
