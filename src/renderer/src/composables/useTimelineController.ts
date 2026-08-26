// The "parent reacts" half: intents in, store edits and camera moves out.
// Nothing here or in the gestures touches the canvas.

import { ref } from 'vue';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';
import { lanesForDeck, toggleLane } from '@renderer/utils/laneSelection';
import {
  DEFAULT_LANE_HEIGHT,
  DEFAULT_WAVEFORM_HEIGHT,
  laneHeightFor,
  waveformHeightFor,
  withLaneHeight,
  withWaveformHeight,
  type StoredLaneHeights
} from '@renderer/utils/laneHeights';
import { useDecksStore } from '@renderer/stores/decks';
import { DECK_ACCENTS, type DeckId, type EditableLaneKey } from '@renderer/utils/types';
import { useSessionEditStore } from '@renderer/stores/sessionEdit';
import { type LaneKey } from '@renderer/utils/timelineDraw';
import {
  bpmRegionSpanAt,
  mergeSelectionRanges,
  type ClipSelectionRef
} from '@renderer/utils/timelineLayout';
import type { Clip, FilterActiveSpan, MasterLaneKey, TransportBlock } from '@renderer/utils/types';
import { MASTER_LANE_KEYS } from '@renderer/utils/types';
import type { BpmContext, Intent } from '@renderer/utils/timelineIntents';
import type { useTimelineView } from '@renderer/composables/useTimelineView';

export type DeckMenu = {
  deck: string;
  x: number;
  y: number;
  bpm: BpmContext | null;
  split: { block: TransportBlock; ms: number } | null;
  lane: { key: EditableLaneKey; ms: number } | null;
};
export type LanePicker = { deck: string; lane: string | null; x: number; y: number };
export type FilterMenu = { deck: string; span: FilterActiveSpan; x: number; y: number };

export function useTimelineController(opts: {
  camera: ReturnType<typeof useTimelineView>;
  getClips: () => Clip[];
  emitSeek: (ms: number) => void;
  requestRender: () => void;
}) {
  const decks = useDecksStore();
  const editStore = useSessionEditStore();

  const storedDeckLanes = ref<Record<string, unknown>>(
    storageGet(STORAGE_KEYS.sessionDeckLane, {})
  );
  const storedMasterLane = storageGet<string>(STORAGE_KEYS.sessionMasterLane, 'masterGain');
  const selectedMasterLane = ref<MasterLaneKey>(
    MASTER_LANE_KEYS.find((key) => key === storedMasterLane) ?? 'masterGain'
  );
  const storedLaneHeights = ref<StoredLaneHeights>(storageGet(STORAGE_KEYS.sessionLaneHeight, {}));
  const storedWaveformHeights = ref<StoredLaneHeights>(
    storageGet(STORAGE_KEYS.sessionWaveformHeight, {})
  );

  const clipSelection = ref<ClipSelectionRef[]>([]);
  const unlockedBlockIds = ref<Set<number>>(new Set());
  const filterSelection = ref<{ deck: string; startMs: number; endMs: number } | null>(null);

  const deckMenu = ref<DeckMenu | null>(null);
  const lanePicker = ref<LanePicker | null>(null);
  const filterMenu = ref<FilterMenu | null>(null);

  function lanesFor(deck: string): LaneKey[] {
    return lanesForDeck(storedDeckLanes.value, deck);
  }

  function setMasterLane(lane: MasterLaneKey): void {
    selectedMasterLane.value = lane;
    storageSet(STORAGE_KEYS.sessionMasterLane, lane);
    lanePicker.value = null;
    opts.requestRender();
  }

  // Left open, because a stack is built by toggling several in a row.
  function toggleDeckLane(deck: string, lane: LaneKey): void {
    storedDeckLanes.value = {
      ...storedDeckLanes.value,
      [deck]: toggleLane(lanesFor(deck), lane)
    };
    storageSet(STORAGE_KEYS.sessionDeckLane, storedDeckLanes.value);
    opts.requestRender();
  }

  function laneHeightOf(deck: string, lane: LaneKey): number {
    return laneHeightFor(storedLaneHeights.value, deck, lane);
  }

  function setLaneHeight(deck: string, lane: LaneKey, height: number): void {
    storedLaneHeights.value = withLaneHeight(storedLaneHeights.value, deck, lane, height);
    storageSet(STORAGE_KEYS.sessionLaneHeight, storedLaneHeights.value);
    opts.requestRender();
  }

  function waveformHeightOf(deck: string): number {
    return waveformHeightFor(storedWaveformHeights.value, deck);
  }

  function setWaveformHeight(deck: string, height: number): void {
    storedWaveformHeights.value = withWaveformHeight(storedWaveformHeights.value, deck, height);
    storageSet(STORAGE_KEYS.sessionWaveformHeight, storedWaveformHeights.value);
    opts.requestRender();
  }

  function accentFor(deck: string): string {
    return decks.decks[deck as DeckId]?.accent ?? DECK_ACCENTS[deck as DeckId];
  }

  // The selected filter span resolved against the live data (its bounds shift
  // after an edit, so match by midpoint).
  function selectedFilterSpan(deckLanes: Record<string, { filterActive: FilterActiveSpan[] }>): {
    deck: string;
    span: FilterActiveSpan;
  } | null {
    const sel = filterSelection.value;
    if (!sel) return null;
    const mid = (sel.startMs + sel.endMs) / 2;
    const span = deckLanes[sel.deck]?.filterActive?.find(
      (active) => mid >= active.startMs && mid <= active.endMs
    );
    return span ? { deck: sel.deck, span } : null;
  }

  function clickSelectionRef(block: TransportBlock, ms: number): ClipSelectionRef {
    if (block.loop) {
      if (unlockedBlockIds.value.has(block.blockId)) {
        const iterations = opts
          .getClips()
          .filter((clip) => clip.deck === block.deck && clip.blockId === block.blockId)
          .sort((first, second) => first.sessionStartMs - second.sessionStartMs);
        const iteration =
          iterations.find((clip) => ms >= clip.sessionStartMs && ms <= clip.sessionEndMs) ??
          iterations[0];
        if (iteration) {
          return {
            deck: block.deck,
            startMs: iteration.sessionStartMs,
            endMs: iteration.sessionEndMs
          };
        }
      }
      return { deck: block.deck, startMs: block.startMs, endMs: block.endMs };
    }
    const clip = opts
      .getClips()
      .find((candidate) => candidate.deck === block.deck && candidate.blockId === block.blockId);
    if (!clip) return { deck: block.deck, startMs: block.startMs, endMs: block.endMs };
    return { deck: block.deck, ...bpmRegionSpanAt(clip, ms) };
  }

  function sameRef(first: ClipSelectionRef, second: ClipSelectionRef): boolean {
    return (
      first.deck === second.deck && first.startMs === second.startMs && first.endMs === second.endMs
    );
  }

  function applySelection(refs: ClipSelectionRef[], additive: boolean): void {
    if (!additive) {
      clipSelection.value = refs;
      return;
    }
    // Cmd/Ctrl toggles each span in and out of the selection.
    const current = [...clipSelection.value];
    for (const ref of refs) {
      const at = current.findIndex((existing) => sameRef(existing, ref));
      if (at >= 0) current.splice(at, 1);
      else current.push(ref);
    }
    clipSelection.value = current;
  }

  async function handleIntent(intent: Intent): Promise<void> {
    try {
      await applyIntent(intent);
    } catch (error) {
      console.error('timeline intent failed', intent.type, error);
    }
  }

  async function applyIntent(intent: Intent): Promise<void> {
    switch (intent.type) {
      case 'seek':
        opts.emitSeek(intent.ms);
        break;
      case 'view.set':
        opts.camera.setViewFromUser(intent.view);
        break;
      case 'lane.openDropdown':
        lanePicker.value = {
          deck: intent.deck,
          lane: intent.lane,
          x: intent.clientX,
          y: intent.clientY
        };
        break;
      case 'lane.resize':
        setLaneHeight(intent.deck, intent.lane, intent.height);
        break;
      case 'lane.resizeReset':
        setLaneHeight(intent.deck, intent.lane, DEFAULT_LANE_HEIGHT);
        break;
      case 'waveform.resize':
        setWaveformHeight(intent.deck, intent.height);
        break;
      case 'waveform.resizeReset':
        setWaveformHeight(intent.deck, DEFAULT_WAVEFORM_HEIGHT);
        break;
      case 'lane.draw':
        await editStore.commitGesture(
          intent.deck,
          intent.lane,
          intent.samples,
          intent.t0,
          intent.t1,
          { rateMin: intent.rateMin, rateMax: intent.rateMax }
        );
        break;
        break;
      case 'filter.toggle':
        await editStore.commitFilterActiveToggle(intent.deck, intent.t0, intent.t1);
        break;
      case 'clip.move':
        await editStore.commitClipMove(opts.getClips(), intent.block, intent.deltaMs);
        break;
      case 'clip.trim':
        await editStore.commitClipTrim(opts.getClips(), intent.block, intent.edge, intent.newMs);
        break;
      case 'clip.delete':
        clipSelection.value = [];
        opts.requestRender();
        await editStore.commitRangesDelete(opts.getClips(), intent.ranges);
        break;
      case 'clip.split':
        await editStore.commitClipSplit(opts.getClips(), intent.block, intent.ms);
        break;
      case 'clip.select':
        applySelection([clickSelectionRef(intent.block, intent.ms)], intent.additive);
        filterSelection.value = null;
        opts.requestRender();
        break;
      case 'clip.selectRange':
        applySelection(intent.targets, intent.additive);
        filterSelection.value = null;
        opts.requestRender();
        break;
      case 'clip.clearSelection':
        clipSelection.value = [];
        opts.requestRender();
        break;
      case 'loopBlock.toggleUnlock': {
        const ids = new Set(unlockedBlockIds.value);
        if (ids.has(intent.block.blockId)) ids.delete(intent.block.blockId);
        else ids.add(intent.block.blockId);
        unlockedBlockIds.value = ids;
        applySelection([clickSelectionRef(intent.block, intent.ms)], false);
        opts.requestRender();
        break;
      }
      case 'filterRegion.select':
        filterSelection.value = {
          deck: intent.deck,
          startMs: intent.span.startMs,
          endMs: intent.span.endMs
        };
        clipSelection.value = [];
        opts.requestRender();
        break;
      case 'lane.reset':
        await editStore.commitLaneReset(intent.deck, intent.lane, intent.ms, intent.extent, {
          rateMin: intent.rateMin,
          rateMax: intent.rateMax
        });
        break;
      case 'filterRegion.clearSelection':
        filterSelection.value = null;
        opts.requestRender();
        break;
      case 'filterRegion.resize':
        filterSelection.value = {
          deck: intent.deck,
          startMs: intent.edge === 'start' ? intent.newMs : intent.span.startMs,
          endMs: intent.edge === 'end' ? intent.newMs : intent.span.endMs
        };
        await editStore.resizeFilterSpan(
          intent.deck,
          intent.span.startMs,
          intent.span.endMs,
          intent.edge,
          intent.newMs
        );
        break;
      case 'filterRegion.delete':
        filterSelection.value = null;
        opts.requestRender();
        await editStore.deleteFilterSpan(intent.deck, intent.span.startMs, intent.span.endMs);
        break;
      case 'filterRegion.move':
        filterSelection.value = {
          deck: intent.deck,
          startMs: intent.span.startMs + intent.deltaMs,
          endMs: intent.span.endMs + intent.deltaMs
        };
        clipSelection.value = [];
        await editStore.moveFilterSpan(
          intent.deck,
          intent.span.startMs,
          intent.span.endMs,
          intent.deltaMs
        );
        break;
      case 'menu.deck':
        deckMenu.value = {
          deck: intent.deck,
          x: intent.clientX,
          y: intent.clientY,
          bpm: intent.bpm,
          split: intent.split,
          lane: intent.lane
        };
        break;
      case 'menu.filterRegion':
        filterMenu.value = {
          deck: intent.deck,
          span: intent.span,
          x: intent.clientX,
          y: intent.clientY
        };
        break;
    }
  }

  function deleteSelectedFilterSpan(
    deckLanes: Record<string, { filterActive: FilterActiveSpan[] }>
  ): void {
    const sel = selectedFilterSpan(deckLanes);
    if (!sel) return;
    editStore.deleteFilterSpan(sel.deck, sel.span.startMs, sel.span.endMs);
    filterSelection.value = null;
    opts.requestRender();
  }

  // Deletes every selected span through the intent path (which clears the
  // selection and commits one edit). Overlapping spans merge per deck.
  function deleteSelectedRanges(): void {
    if (clipSelection.value.length === 0) return;
    handleIntent({ type: 'clip.delete', ranges: mergeSelectionRanges(clipSelection.value) });
  }

  return {
    storedDeckLanes,
    storedLaneHeights,
    laneHeightOf,
    storedWaveformHeights,
    waveformHeightOf,
    clipSelection,
    filterSelection,
    unlockedBlockIds,
    deckMenu,
    lanePicker,
    selectedMasterLane,
    setMasterLane,
    filterMenu,
    lanesFor,
    toggleDeckLane,
    accentFor,
    selectedFilterSpan,
    deleteSelectedFilterSpan,
    deleteSelectedRanges,
    handleIntent
  };
}
