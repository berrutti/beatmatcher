// The "parent reacts" half. Owns the timeline's interaction state (lane choice +
// height, clip/filter selection, menus) and turns each emitted Intent into a
// store edit, a camera move, or a selection change. The component stays a thin
// shell that renders the scene and forwards DOM events to the gestures; nothing
// here or in the gestures touches the canvas.

import { ref } from 'vue';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';
import { DECK_ACCENTS, type DeckId, useDecksStore } from '@renderer/stores/decks';
import { useSessionEditStore } from '@renderer/stores/sessionEdit';
import { type LaneKey } from '@renderer/utils/timelineDraw';
import { type ClipSelectionRef } from '@renderer/utils/timelineLayout';
import type { Clip, FilterActiveSpan, NudgeSpan } from '@renderer/composables/useSessionTimeline';
import type { TransportBlock } from '@renderer/utils/types';
import type { Intent } from '@renderer/utils/timelineIntents';
import type { useTimelineView } from '@renderer/composables/useTimelineView';

const DEFAULT_DECK_LANE: LaneKey = 'filter';
const DEFAULT_LANE_H = 96;

export type DeckMenu = { deck: string; x: number; y: number; nudge: NudgeSpan | null };
export type LanePicker = { deck: string; x: number; y: number };
export type FilterMenu = { deck: string; span: FilterActiveSpan; x: number; y: number };

export function useTimelineController(opts: {
  camera: ReturnType<typeof useTimelineView>;
  getClips: () => Clip[];
  emitSeek: (ms: number) => void;
  requestRender: () => void;
}) {
  const decks = useDecksStore();
  const editStore = useSessionEditStore();

  const selectedDeckLane = ref<Record<string, LaneKey>>(
    storageGet(STORAGE_KEYS.sessionDeckLane, {})
  );
  const storedH = storageGet<number>(STORAGE_KEYS.sessionLaneHeight, DEFAULT_LANE_H);
  const laneHeight = ref(typeof storedH === 'number' ? storedH : DEFAULT_LANE_H);

  const clipSelection = ref<ClipSelectionRef | null>(null);
  const unlockedBlockIds = ref<Set<number>>(new Set());
  const filterSelection = ref<{ deck: string; startMs: number; endMs: number } | null>(null);

  const deckMenu = ref<DeckMenu | null>(null);
  const lanePicker = ref<LanePicker | null>(null);
  const filterMenu = ref<FilterMenu | null>(null);

  function laneFor(deck: string): LaneKey {
    return selectedDeckLane.value[deck] ?? DEFAULT_DECK_LANE;
  }

  function setDeckLane(deck: string, lane: LaneKey): void {
    selectedDeckLane.value = { ...selectedDeckLane.value, [deck]: lane };
    storageSet(STORAGE_KEYS.sessionDeckLane, selectedDeckLane.value);
    lanePicker.value = null;
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
    const span = deckLanes[sel.deck]?.filterActive?.find((s) => mid >= s.startMs && mid <= s.endMs);
    return span ? { deck: sel.deck, span } : null;
  }

  function selectBlock(block: TransportBlock, ms: number): void {
    if (block.loop && unlockedBlockIds.value.has(block.blockId)) {
      // Pick the iteration under the cursor.
      const iters = opts
        .getClips()
        .filter((c) => c.deck === block.deck && c.blockId === block.blockId)
        .sort((a, b) => a.sessionStartMs - b.sessionStartMs);
      const iter = iters.find((c) => ms >= c.sessionStartMs && ms <= c.sessionEndMs) ?? iters[0];
      clipSelection.value = iter
        ? { deck: block.deck, blockId: block.blockId, iterationStartMs: iter.sessionStartMs }
        : { deck: block.deck, blockId: block.blockId, iterationStartMs: null };
    } else {
      clipSelection.value = { deck: block.deck, blockId: block.blockId, iterationStartMs: null };
    }
  }

  function handleIntent(intent: Intent): void {
    switch (intent.type) {
      case 'seek':
        opts.emitSeek(intent.ms);
        break;
      case 'view.set':
        opts.camera.setView(intent.view);
        break;
      case 'scroll.set':
        opts.camera.scrollY.value = intent.scrollY;
        opts.requestRender();
        break;
      case 'lane.openDropdown':
        lanePicker.value = { deck: intent.deck, x: intent.clientX, y: intent.clientY };
        break;
      case 'lane.resize':
        laneHeight.value = intent.height;
        storageSet(STORAGE_KEYS.sessionLaneHeight, laneHeight.value);
        opts.requestRender();
        break;
      case 'lane.resizeReset':
        laneHeight.value = DEFAULT_LANE_H;
        storageSet(STORAGE_KEYS.sessionLaneHeight, laneHeight.value);
        opts.requestRender();
        break;
      case 'lane.draw':
        editStore
          .commitGesture(intent.deck, intent.lane, intent.samples, intent.t0, intent.t1, {
            rateMin: intent.rateMin,
            rateMax: intent.rateMax
          })
          .catch(() => {});
        break;
      case 'nudge.paint':
        editStore
          .commitNudgePaint(intent.deck, intent.t0, intent.t1, intent.direction)
          .catch(() => {});
        break;
      case 'filter.toggle':
        editStore.commitFilterActiveToggle(intent.deck, intent.t0, intent.t1).catch(() => {});
        break;
      case 'clip.move':
        editStore.commitClipMove(opts.getClips(), intent.block, intent.deltaMs).catch(() => {});
        break;
      case 'clip.trim':
        editStore
          .commitClipTrim(opts.getClips(), intent.block, intent.edge, intent.newMs)
          .catch(() => {});
        break;
      case 'clip.select':
        selectBlock(intent.block, intent.ms);
        opts.requestRender();
        break;
      case 'clip.clearSelection':
        clipSelection.value = null;
        opts.requestRender();
        break;
      case 'loopBlock.toggleUnlock': {
        const ids = new Set(unlockedBlockIds.value);
        if (ids.has(intent.block.blockId)) ids.delete(intent.block.blockId);
        else ids.add(intent.block.blockId);
        unlockedBlockIds.value = ids;
        selectBlock(intent.block, intent.ms);
        opts.requestRender();
        break;
      }
      case 'filterRegion.select':
        filterSelection.value = {
          deck: intent.deck,
          startMs: intent.span.startMs,
          endMs: intent.span.endMs
        };
        opts.requestRender();
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
        editStore
          .resizeFilterSpan(
            intent.deck,
            intent.span.startMs,
            intent.span.endMs,
            intent.edge,
            intent.newMs
          )
          .catch(() => {});
        break;
      case 'filterRegion.delete':
        editStore.deleteFilterSpan(intent.deck, intent.span.startMs, intent.span.endMs);
        filterSelection.value = null;
        opts.requestRender();
        break;
      case 'menu.deck':
        deckMenu.value = {
          deck: intent.deck,
          x: intent.clientX,
          y: intent.clientY,
          nudge: intent.nudge
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

  return {
    // state for the scene + menus
    selectedDeckLane,
    laneHeight,
    clipSelection,
    filterSelection,
    unlockedBlockIds,
    deckMenu,
    lanePicker,
    filterMenu,
    // helpers the component/scene need
    laneFor,
    setDeckLane,
    accentFor,
    selectedFilterSpan,
    deleteSelectedFilterSpan,
    handleIntent
  };
}
