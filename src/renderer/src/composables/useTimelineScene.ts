import type { SceneItem, ViewContext } from '@renderer/utils/timelineEngine';
import { TrackWaveform, type MasterSublane, type RowLayout } from '@renderer/utils/timelineDraw';
import {
  computeRowLayout,
  stackLanes,
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
  waveformSeparatorItem,
  masterChromeItem,
  masterLaneItem,
  rowDividersItem,
  playheadItem,
  overviewItem,
  frameGuttersItem
} from '@renderer/utils/timelineItems';
import {
  MASTER_ROW_ID,
  isMasterLaneKey,
  type DeckId,
  type DeckLaneKey
} from '@renderer/utils/types';
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
  lanesFor: (deck: string) => DeckLaneKey[];
  masterLanesFor: () => MasterLaneKey[];
  laneHeightFor: (deck: string, key: EditableLaneKey) => number;
  waveformHeightFor: (deck: string) => number;
  openLaneFor: (deck: string) => EditableLaneKey | null;
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
  clipSelection: ClipSelectionRef[];
  filterSelection: { deck: string; startMs: number; endMs: number } | null;
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
  // The master lanes sit at the top, directly below the time ruler and above
  // every deck row. The deck rows begin below them.
  const masterTop = vc.laneOriginY;
  const masterSublanes: MasterSublane[] = stackLanes(
    masterTop,
    input.masterLanesFor().map((key) => ({ key, height: input.laneHeightFor(MASTER_ROW_ID, key) }))
  );
  const masterHeight = masterSublanes.reduce((total, lane) => total + lane.height, 0);
  const rows = computeRowLayout(deckSpecs, masterTop + masterHeight);

  const items: SceneItem[] = [];

  const openMaster = input.openLaneFor(MASTER_ROW_ID);
  items.push(
    masterChromeItem(
      masterTop,
      masterHeight,
      masterSublanes,
      input.laneLabel,
      openMaster !== null && isMasterLaneKey(openMaster) ? openMaster : null
    )
  );
  for (const lane of masterSublanes) {
    items.push(
      masterLaneItem(
        lane,
        input.masterLanes,
        resetHighlight(input.resetPreview, MASTER_ROW_ID, lane.key)
      )
    );
    items.push(laneSeparatorItem(lane, MASTER_ROW_ID));
  }

  rows.forEach((row) => {
    const deck = row.deckId;
    items.push(
      deckChromeItem(row, {
        accent: input.accentFor(deck),
        audible: input.audibleFor(deck),
        solo: input.soloFor(deck),
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
