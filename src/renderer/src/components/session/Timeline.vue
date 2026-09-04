<template>
  <div class="timeline" ref="containerEl">
    <!-- Real scroll container so vertical overflow uses the browser's native
         scrollbar. The canvas is sticky (always fills the viewport and redraws
         the visible slice); the sizer is an empty spacer whose height is the
         scrollable amount, giving the scrollbar its range. -->
    <div class="timeline__scroll" ref="scrollEl" @scroll="onScroll">
      <canvas
        ref="canvasEl"
        class="timeline__canvas"
        @click="onCanvasClick"
        @dblclick="onCanvasDblClick"
        @contextmenu.prevent="onCanvasContextMenu"
        @wheel="onCanvasWheel"
        @mousedown="onCanvasMouseDown"
        @mousemove="onCanvasHoverMove"
        @mouseleave="onCanvasHoverLeave"
      />
      <div class="timeline__sizer" ref="sizerEl" aria-hidden="true" />
    </div>
  </div>

  <Teleport to="body">
    <div
      v-if="deckMenu"
      v-menu-placement
      class="lane-menu"
      :style="{ left: deckMenu.x + 'px', top: deckMenu.y + 'px' }"
      @click.stop
    >
      <button
        v-if="editStore.editMode && deckMenu.bpm"
        class="lane-menu__item"
        @click="onOpenBpmDialog('clip')"
      >
        {{ $t('session.setClipBpm') }}
      </button>
      <button
        v-if="editStore.editMode && deckMenu.bpm"
        class="lane-menu__item"
        @click="onOpenBpmDialog('fromHere')"
      >
        {{ $t('session.setBpmFromHere') }}
      </button>
      <button
        v-if="editStore.editMode && deckMenu.lane"
        class="lane-menu__item"
        @click="onResetLane('thisMove')"
      >
        {{ $t('session.resetLaneThisMove', { lane: laneName(deckMenu.lane.key) }) }}
      </button>
      <button
        v-if="editStore.editMode && deckMenu.lane"
        class="lane-menu__item"
        @click="onResetLane('toEnd')"
      >
        {{ $t('session.resetLaneFromHere', { lane: laneName(deckMenu.lane.key) }) }}
      </button>
      <button
        v-if="editStore.editMode && deckMenu.lane"
        class="lane-menu__item"
        @click="onResetLane('untilHere')"
      >
        {{ $t('session.resetLaneUntilHere', { lane: laneName(deckMenu.lane.key) }) }}
      </button>
      <button
        v-if="editStore.editMode && deckMenu.split"
        class="lane-menu__item"
        @click="onSplitClip"
      >
        {{ $t('session.splitClip') }}
      </button>
      <button
        v-if="deckMenu.deck !== MASTER_ROW_ID"
        class="lane-menu__item"
        :class="{ 'lane-menu__item--no-effect': sessionStore.soloedDeck !== null }"
        v-tooltip="sessionStore.soloedDeck !== null ? $t('session.enabledOverridden') : undefined"
        @click="onToggleEnabled"
      >
        {{ $t('session.enabled') }}
        <span class="lane-menu__check">{{
          sessionStore.deckEnabled(deckMenu.deck) ? '✓' : ''
        }}</span>
      </button>
      <button v-if="deckMenu.deck !== MASTER_ROW_ID" class="lane-menu__item" @click="onToggleSolo">
        {{ $t('session.solo') }}
        <span class="lane-menu__check">{{
          sessionStore.soloedDeck === deckMenu.deck ? '✓' : ''
        }}</span>
      </button>
      <div
        v-if="editStore.editMode && deckMenu.deck !== MASTER_ROW_ID"
        class="lane-menu__item lane-menu__item--sub"
      >
        {{ $t('session.lanesMenu') }}
        <span class="lane-menu__arrow">▶</span>
        <div class="lane-menu__submenu">
          <button
            v-for="key in DECK_LANE_KEYS"
            :key="key"
            class="lane-menu__item"
            :class="{ 'lane-menu__item--refused': isLastLane(deckMenu.deck, key) }"
            v-tooltip="isLastLane(deckMenu.deck, key) ? $t('session.lastLane') : undefined"
            @click="onPickLaneFromMenu(key)"
          >
            {{ $t(`session.lanes.${key}`) }}
            <span class="lane-menu__check">{{ laneIsShown(deckMenu.deck, key) ? '✓' : '' }}</span>
          </button>
        </div>
      </div>
    </div>
    <div
      v-if="deckMenu"
      class="lane-menu__backdrop"
      @click="deckMenu = null"
      @contextmenu.prevent="deckMenu = null"
    />

    <div
      v-if="lanePicker"
      v-menu-placement
      class="lane-menu"
      :style="{ left: lanePicker.x + 'px', top: lanePicker.y + 'px' }"
      @click.stop
    >
      <button
        v-for="key in controller.laneKeysForRow(lanePicker.deck)"
        :key="key"
        class="lane-menu__item"
        :class="{ 'lane-menu__item--refused': isLastLane(lanePicker.deck, key) }"
        v-tooltip="isLastLane(lanePicker.deck, key) ? $t('session.lastLane') : undefined"
        @click="onPickLane(key)"
      >
        {{ $t(`session.lanes.${key}`) }}
        <span class="lane-menu__check">{{ laneIsShown(lanePicker.deck, key) ? '✓' : '' }}</span>
      </button>
    </div>
    <div
      v-if="lanePicker"
      class="lane-menu__backdrop"
      @click="lanePicker = null"
      @contextmenu.prevent="lanePicker = null"
    />

    <div
      v-if="filterMenu"
      v-menu-placement
      class="lane-menu"
      :style="{ left: filterMenu.x + 'px', top: filterMenu.y + 'px' }"
      @click.stop
    >
      <button class="lane-menu__item" @click="onDeleteFilterRegion">
        {{ $t('session.deleteFilterRegion') }}
      </button>
    </div>
    <div
      v-if="filterMenu"
      class="lane-menu__backdrop"
      @click="filterMenu = null"
      @contextmenu.prevent="filterMenu = null"
    />

    <BpmModal
      :open="bpmDialog !== null"
      :current-bpm="bpmDialog?.currentBpm ?? null"
      @submit="onSetBpm"
      @cancel="bpmDialog = null"
    />
  </Teleport>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import type { Clip, LoadedSpan, DeckLanes, MasterLanes, LanePoint } from '@renderer/utils/types';
import {
  DECK_LANE_KEYS,
  MASTER_ROW_ID,
  type DeckLaneKey,
  type EditableLaneKey
} from '@renderer/utils/types';
import type { ResetExtent } from '@renderer/utils/sessionCore';
import {
  DECK_ORDER,
  LABEL_W,
  PADDING,
  makeMsToX,
  type RowLayout,
  type TrackWaveform
} from '@renderer/utils/timelineDraw';
import {
  badgeAlpha,
  badgeFading,
  updateBadgeFade,
  type Badge,
  type BadgeFade
} from '@renderer/utils/badgeFade';
import { overlapsRange } from '@renderer/utils/timelineView';
import { TIMELINE_LOD_DEBOUNCE_MS } from '@renderer/utils/waveformLod';
import { renderScene, type SceneItem, type ViewContext } from '@renderer/utils/timelineEngine';
import { useTimelineView } from '@renderer/composables/useTimelineView';
import { useTimelineController } from '@renderer/composables/useTimelineController';
import { useTimelineGestures } from '@renderer/composables/useTimelineGestures';
import { buildScene } from '@renderer/composables/useTimelineScene';
import { useSessionStore } from '@renderer/stores/session';
import { useSessionEditStore } from '@renderer/stores/sessionEdit';
import { DEFAULT_MIXER_ID } from '@renderer/stores/settings';
import BpmModal from '@renderer/components/modals/BpmModal.vue';
import type { BpmContext } from '@renderer/utils/timelineIntents';

// Region-based rather than whole-track, so detail scales with zoom however long
// the track is.
const MIN_REGION_POINTS = 256;
const MAX_REGION_POINTS = 16000;
const BASE_REGION_POINTS_MAX = 4000;
const LOD_OVERSAMPLE = 1.5;

const props = defineProps<{
  durationMs: number;
  clips: Clip[];
  loadedSpans: LoadedSpan[];
  playheadMs: number;
  deckLanes: Record<string, DeckLanes>;
  masterLanes: MasterLanes;
  deckJog: Record<string, LanePoint[]>;
  waveforms: Map<string, TrackWaveform>;
}>();

const emit = defineEmits<{ seek: [ms: number] }>();

const { t } = useI18n();
const sessionStore = useSessionStore();
const editStore = useSessionEditStore();

const containerEl = ref<HTMLDivElement | null>(null);
const scrollEl = ref<HTMLDivElement | null>(null);
const sizerEl = ref<HTMLDivElement | null>(null);
const canvasEl = ref<HTMLCanvasElement | null>(null);

const camera = useTimelineView(
  () => props.durationMs,
  () => sessionStore.session?.mixerId ?? DEFAULT_MIXER_ID
);
const controller = useTimelineController({
  camera,
  getClips: () => props.clips,
  emitSeek: (ms) => emit('seek', ms),
  requestRender: scheduleRender
});
const { deckMenu, lanePicker, filterMenu } = controller;

// Last frame's scene: hit-testing runs against it with a freshly computed
// ViewContext, which shares the canvas size so the geometry still lines up.
let sceneItems: SceneItem[] = [];
let sceneRows: RowLayout[] = [];

function viewContext(): ViewContext {
  const el = scrollEl.value;
  return camera.viewContext(el?.clientWidth ?? 0, el?.clientHeight ?? 0);
}

const gestures = useTimelineGestures({
  camera,
  emit: controller.handleIntent,
  getItems: () => sceneItems,
  getRows: () => sceneRows,
  getVc: viewContext,
  getClips: () => props.clips,
  getEvents: () => sessionStore.session?.events ?? [],
  getDeckLanes: () => props.deckLanes,
  laneHeightFor: (deck, lane) => controller.laneHeightOf(deck, lane),
  waveformHeightFor: (deck) => controller.waveformHeightOf(deck),
  isEditMode: () => editStore.editMode,
  durationMs: () => props.durationMs,
  accentFor: controller.accentFor,
  requestRender: scheduleRender,
  setCursor: (cursor) => {
    if (canvasEl.value) canvasEl.value.style.cursor = cursor;
  }
});

let raf = 0;
function scheduleRender(): void {
  if (raf) return;
  raf = requestAnimationFrame(() => {
    raf = 0;
    render();
  });
}

const badgeFades = new Map<string, BadgeFade>();

function badgeFor(deck: string): Badge | null {
  if (sessionStore.soloedDeck === deck) return { label: t('session.solo'), solo: true };
  if (!sessionStore.deckEnabled(deck)) return { label: t('session.disabledBadge'), solo: false };
  return null;
}

function fadeOf(deck: string, nowMs: number): BadgeFade {
  const fade = updateBadgeFade(badgeFades.get(deck), badgeFor(deck), nowMs);
  badgeFades.set(deck, fade);
  return fade;
}

function render(): void {
  const canvas = canvasEl.value;
  const scroll = scrollEl.value;
  if (!canvas || !scroll) return;
  const dpr = window.devicePixelRatio || 1;
  const cw = scroll.clientWidth;
  const ch = scroll.clientHeight;
  if (cw === 0 || ch === 0) return;

  canvas.width = cw * dpr;
  canvas.height = ch * dpr;
  canvas.style.width = cw + 'px';
  canvas.style.height = ch + 'px';

  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  ctx.scale(dpr, dpr);
  ctx.fillStyle = '#111';
  ctx.fillRect(0, 0, cw, ch);

  const nowMs = performance.now();
  const fades = new Map<string, BadgeFade>(DECK_ORDER.map((deck) => [deck, fadeOf(deck, nowMs)]));

  const vc = camera.viewContext(cw, ch);
  const scene = buildScene({
    vc,
    decks: DECK_ORDER,
    clips: props.clips,
    loadedSpans: props.loadedSpans,
    deckLanes: props.deckLanes,
    masterLanes: props.masterLanes,
    deckJog: props.deckJog,
    waveforms: props.waveforms,
    playheadMs: props.playheadMs,
    durationMs: props.durationMs,
    editMode: editStore.editMode,
    lanesFor: controller.lanesFor,
    masterLanesFor: controller.masterLanesFor,
    laneHeightFor: controller.laneHeightOf,
    waveformHeightFor: controller.waveformHeightOf,
    openLaneFor: openLaneOf,
    accentFor: controller.accentFor,
    laneLabel: (key) => t(`session.lanes.${key}`),
    deckLabel: (deck) => t('deck.label', { id: deck }),
    badgeLabel: (deck) => fades.get(deck)?.badge?.label ?? '',
    badgeAlphaFor: (deck) => {
      const fade = fades.get(deck);
      return fade ? badgeAlpha(fade, nowMs) : 0;
    },
    menuOpenFor: (deck) => deckMenu.value?.deck === deck,
    resetPreview: resetPreview.value,
    audibleFor: (deck) => sessionStore.deckAudible(deck),
    soloFor: (deck) => fades.get(deck)?.badge?.solo ?? false,
    clipSelection: controller.clipSelection.value,
    filterSelection: controller.filterSelection.value,
    overlays: gestures.overlays()
  });

  camera.setContentMetrics(scene.contentHeight, vc.scrollViewport.bottom - vc.scrollViewport.top);
  // Re-synced in case the content shrank and the clamp moved scrollY below where
  // the element still sits.
  if (sizerEl.value) sizerEl.value.style.height = `${camera.maxScrollY()}px`;
  if (scroll.scrollTop !== camera.scrollY.value) scroll.scrollTop = camera.scrollY.value;
  sceneItems = scene.items;
  sceneRows = scene.rows;
  renderScene(ctx, scene.items, vc);

  for (const fade of fades.values()) {
    if (badgeFading(fade, nowMs)) {
      scheduleRender();
      return;
    }
  }
}

// The native scrollbar / wheel moved the container: mirror it into the camera so
// the next frame redraws the rows at the new offset.
function onScroll(): void {
  const scroll = scrollEl.value;
  if (!scroll || camera.scrollY.value === scroll.scrollTop) return;
  camera.scrollY.value = scroll.scrollTop;
  scheduleRender();
}

function onCanvasMouseDown(e: MouseEvent): void {
  if (!canvasEl.value) return;
  gestures.onMouseDown(e, canvasEl.value.getBoundingClientRect());
  window.addEventListener('mousemove', onWindowMove);
  window.addEventListener('mouseup', onWindowUp);
}

function onWindowMove(e: MouseEvent): void {
  if (!canvasEl.value) return;
  gestures.onMouseMove(e, canvasEl.value.getBoundingClientRect());
}

function onWindowUp(): void {
  gestures.onMouseUp();
  window.removeEventListener('mousemove', onWindowMove);
  window.removeEventListener('mouseup', onWindowUp);
}

function onCanvasClick(e: MouseEvent): void {
  if (!canvasEl.value) return;
  gestures.onClick(e, canvasEl.value.getBoundingClientRect());
}

function onCanvasDblClick(e: MouseEvent): void {
  if (!canvasEl.value) return;
  gestures.onDblClick(e, canvasEl.value.getBoundingClientRect());
}

function onCanvasContextMenu(e: MouseEvent): void {
  if (!canvasEl.value) return;
  gestures.onContextMenu(e, canvasEl.value.getBoundingClientRect());
}

function onCanvasWheel(e: WheelEvent): void {
  if (!canvasEl.value) return;
  gestures.onWheel(e, canvasEl.value.getBoundingClientRect());
}

function onCanvasHoverMove(e: MouseEvent): void {
  if (gestures.hasActive() || !canvasEl.value) return;
  const rect = canvasEl.value.getBoundingClientRect();
  canvasEl.value.style.cursor = gestures.cursorFor({
    x: e.clientX - rect.left,
    y: e.clientY - rect.top
  });
}

function onCanvasHoverLeave(): void {
  if (!gestures.hasActive() && canvasEl.value) canvasEl.value.style.cursor = '';
}

function onSplitClip(): void {
  const menu = deckMenu.value;
  deckMenu.value = null;
  if (!menu?.split) return;
  controller.handleIntent({ type: 'clip.split', block: menu.split.block, ms: menu.split.ms });
}

// Left open, like the lane ticks: a listener flips these against each other
// while comparing decks, so a click is rarely the last thing they want.
function onToggleEnabled(): void {
  if (deckMenu.value) sessionStore.toggleDeckEnabled(deckMenu.value.deck);
}

function onToggleSolo(): void {
  if (deckMenu.value) sessionStore.toggleSolo(deckMenu.value.deck);
}

const resetPreview = computed(() => {
  const menu = deckMenu.value;
  if (!menu?.lane || !editStore.editMode) return null;
  const lanes = props.deckLanes[menu.deck];
  const span = editStore.moveSpanAt(menu.deck, menu.lane.key, menu.lane.ms, {
    rateMin: lanes?.rateMin,
    rateMax: lanes?.rateMax
  });
  if (!span) return null;
  return {
    deck: menu.deck,
    lane: menu.lane.key,
    startMs: span.startMs,
    endMs: span.endMs
  };
});

function laneName(lane: EditableLaneKey): string {
  return t(`session.lanes.${lane}`);
}

function onResetLane(extent: ResetExtent): void {
  const menu = deckMenu.value;
  deckMenu.value = null;
  if (!menu?.lane) return;
  const lanes = props.deckLanes[menu.deck];
  controller.handleIntent({
    type: 'lane.reset',
    deck: menu.deck,
    lane: menu.lane.key,
    ms: menu.lane.ms,
    extent,
    rateMin: lanes?.rateMin,
    rateMax: lanes?.rateMax
  });
}

function onPickLaneFromMenu(lane: DeckLaneKey): void {
  if (deckMenu.value) controller.toggleDeckLane(deckMenu.value.deck, lane);
}

function laneIsShown(deck: string, lane: EditableLaneKey): boolean {
  return controller.lanesForRow(deck).includes(lane);
}

// The row would have no lane left to click on, so the tick is held down.
function isLastLane(deck: string, lane: EditableLaneKey): boolean {
  const shown = controller.lanesForRow(deck);
  return shown.length === 1 && shown[0] === lane;
}

function openLaneOf(deck: string): EditableLaneKey | null {
  const picker = lanePicker.value;
  return picker && picker.deck === deck ? picker.lane : null;
}

// Stays open: a stack is built by ticking several in a row.
function onPickLane(lane: EditableLaneKey): void {
  const picker = lanePicker.value;
  if (picker) controller.toggleLaneForRow(picker.deck, lane);
}

function onDeleteFilterRegion(): void {
  const menu = filterMenu.value;
  filterMenu.value = null;
  if (!menu) return;
  controller.handleIntent({ type: 'filterRegion.delete', deck: menu.deck, span: menu.span });
}

// Right-clicked clip's set-BPM target, captured when the dialog opens so the
// edit applies even after the menu closes. `mode` picks whole-clip vs from-here.
const bpmDialog = ref<({ deck: string; mode: 'clip' | 'fromHere' } & BpmContext) | null>(null);

function onOpenBpmDialog(mode: 'clip' | 'fromHere'): void {
  const menu = deckMenu.value;
  deckMenu.value = null;
  if (!menu?.bpm) return;
  bpmDialog.value = { deck: menu.deck, mode, ...menu.bpm };
}

function onSetBpm(bpm: number): void {
  const dialog = bpmDialog.value;
  bpmDialog.value = null;
  if (!dialog || dialog.trackBpm <= 0) return;
  const rate = bpm / dialog.trackBpm;
  if (dialog.mode === 'clip') {
    editStore
      .commitSetClipBpm(dialog.deck, dialog.clipStartMs, dialog.clipEndMs, rate)
      .catch(() => {});
  } else {
    editStore.commitSetBpm(dialog.deck, dialog.ms, rate).catch(() => {});
  }
}

function clipTrackSegments(clip: Clip) {
  return clip.waveSegments.length > 0
    ? clip.waveSegments
    : [
        {
          wallStartMs: clip.sessionStartMs,
          wallEndMs: clip.sessionEndMs,
          trackStartSec: clip.trackStartSec,
          trackEndSec:
            clip.trackStartSec +
            ((clip.sessionEndMs - clip.sessionStartMs) / 1000) * clip.playbackRate
        }
      ];
}

// The store no-ops when it already holds enough points, and the redraw follows
// whenever new data lands.
function updateWaveformLod(): void {
  const container = containerEl.value;
  if (!container) return;
  const trackW = container.clientWidth - LABEL_W - PADDING;
  if (trackW <= 0) return;
  const view = camera.currentView();
  const viewEnd = view.start + view.duration;
  const msToX = makeMsToX(view, trackW);
  const dpr = window.devicePixelRatio || 1;

  const acc = new Map<string, { minT: number; maxT: number; px: number }>();
  const extent = new Map<string, { minT: number; maxT: number }>();
  for (const clip of props.clips) {
    const visible = overlapsRange(clip.sessionStartMs, clip.sessionEndMs, view.start, viewEnd);
    for (const seg of clipTrackSegments(clip)) {
      const segLo = Math.min(seg.trackStartSec, seg.trackEndSec);
      const segHi = Math.max(seg.trackStartSec, seg.trackEndSec);
      const ext = extent.get(clip.trackPath);
      if (ext) {
        ext.minT = Math.min(ext.minT, segLo);
        ext.maxT = Math.max(ext.maxT, segHi);
      } else {
        extent.set(clip.trackPath, { minT: segLo, maxT: segHi });
      }
      if (!visible) continue;
      const w0 = Math.max(seg.wallStartMs, view.start);
      const w1 = Math.min(seg.wallEndMs, viewEnd);
      const wallSpan = seg.wallEndMs - seg.wallStartMs;
      if (w1 <= w0 || wallSpan <= 0) continue;
      const trackSpan = seg.trackEndSec - seg.trackStartSec;
      const t0 = seg.trackStartSec + ((w0 - seg.wallStartMs) / wallSpan) * trackSpan;
      const t1 = seg.trackStartSec + ((w1 - seg.wallStartMs) / wallSpan) * trackSpan;
      const px = msToX(w1) - msToX(w0);
      const entry = acc.get(clip.trackPath);
      if (entry) {
        entry.minT = Math.min(entry.minT, Math.min(t0, t1));
        entry.maxT = Math.max(entry.maxT, Math.max(t0, t1));
        entry.px += px;
      } else {
        acc.set(clip.trackPath, { minT: Math.min(t0, t1), maxT: Math.max(t0, t1), px });
      }
    }
  }

  for (const [path, r] of acc) {
    const ext = extent.get(path);
    if (ext) {
      const basePoints = Math.min(
        BASE_REGION_POINTS_MAX,
        Math.max(MIN_REGION_POINTS, trackW * dpr)
      );
      sessionStore
        .ensureWaveformBase(path, Math.max(0, ext.minT), ext.maxT, Math.ceil(basePoints))
        .catch(() => {});
    }

    const span = Math.max(1e-3, r.maxT - r.minT);
    const pad = span * 0.2;
    const startSec = Math.max(0, r.minT - pad);
    const endSec = r.maxT + pad;
    const pointsPerTrackSec = (r.px * dpr * LOD_OVERSAMPLE) / span;
    const numPoints = Math.min(
      MAX_REGION_POINTS,
      Math.max(MIN_REGION_POINTS, Math.ceil(pointsPerTrackSec * (endSec - startSec)))
    );
    sessionStore.ensureWaveformRegion(path, startSec, endSec, numPoints).catch(() => {});
  }
}

let lodTimer: ReturnType<typeof setTimeout> | null = null;
function scheduleWaveformLod(): void {
  if (lodTimer !== null) clearTimeout(lodTimer);
  lodTimer = setTimeout(updateWaveformLod, TIMELINE_LOD_DEBOUNCE_MS);
}

// Zoom (duration), pan (start), and clips arriving all change which track region
// needs detail, so refetch on any of them (debounced).
watch(
  () => [camera.viewStartMs.value, camera.viewDurationMs.value, props.clips],
  scheduleWaveformLod,
  { immediate: true }
);

watch(
  () => props.playheadMs,
  (ms) => camera.followPlayhead(ms)
);

// Clip takes precedence over a filter span.
function onKeyDown(e: KeyboardEvent): void {
  if (e.key !== 'Delete' && e.key !== 'Backspace') return;
  if (!editStore.editMode) return;
  if (controller.clipSelection.value.length > 0) {
    e.preventDefault();
    controller.deleteSelectedRanges();
  } else if (controller.filterSelection.value) {
    e.preventDefault();
    controller.deleteSelectedFilterSpan(props.deckLanes);
  }
}

// blockIds are reallocated whenever clips rebuild (any edit), so clip selection
// and loop unlocks cannot survive an edit. Leaving edit mode clears them too.
watch(
  () => props.clips,
  () => {
    controller.clipSelection.value = [];
    controller.unlockedBlockIds.value = new Set();
  }
);
watch(
  () => editStore.editMode,
  (on) => {
    if (!on) {
      controller.clipSelection.value = [];
      controller.unlockedBlockIds.value = new Set();
    }
  }
);

let ro: ResizeObserver | null = null;
onMounted(() => {
  ro = new ResizeObserver(() => {
    scheduleRender();
    scheduleWaveformLod();
  });
  // Observe the scroll element's content box so a re-render also fires when the
  // native scrollbar appears/disappears (which changes the usable canvas width).
  if (scrollEl.value) ro.observe(scrollEl.value);
  window.addEventListener('keydown', onKeyDown);
  scheduleRender();
});

onUnmounted(() => {
  ro?.disconnect();
  if (lodTimer !== null) clearTimeout(lodTimer);
  window.removeEventListener('keydown', onKeyDown);
  window.removeEventListener('mousemove', onWindowMove);
  window.removeEventListener('mouseup', onWindowUp);
});

watch(
  () => [
    props.clips,
    props.loadedSpans,
    props.durationMs,
    props.playheadMs,
    props.deckLanes,
    props.masterLanes,
    props.deckJog,
    props.waveforms,
    camera.viewStartMs.value,
    camera.viewDurationMs.value,
    camera.scrollY.value,
    controller.storedDeckLanes.value,
    controller.storedMasterLanes.value,
    controller.storedLaneHeights.value,
    controller.storedWaveformHeights.value,
    controller.clipSelection.value,
    controller.filterSelection.value,
    controller.lanePicker.value,
    controller.deckMenu.value,
    resetPreview.value,
    editStore.editMode,
    sessionStore.disabledDecks,
    sessionStore.soloedDeck,
    DECK_ORDER.map((id) => controller.accentFor(id))
  ],
  scheduleRender
);
</script>

<style scoped>
.timeline {
  /* flex: 1 + min-height: 0, not height: 100%: the timeline sits in a flex
     column next to banner siblings, and a 100% height there overflows the
     body and paints over the transport bar when the window shrinks. */
  width: 100%;
  flex: 1;
  min-height: 0;
  overflow: hidden;
  position: relative;
}

.timeline__scroll {
  position: absolute;
  inset: 0;
  overflow-y: auto;
  overflow-x: hidden;
}

/* Sticky so the canvas stays pinned to the viewport top and keeps filling it
   while the container scrolls; the render redraws the visible rows per scroll. */
.timeline__canvas {
  display: block;
  position: sticky;
  top: 0;
}

/* Empty spacer that gives the scroll container its scrollable height. Behind
   the sticky canvas and click-through so it never intercepts canvas pointer
   events where it overlaps. */
.timeline__sizer {
  width: 1px;
  pointer-events: none;
}

.lane-menu__backdrop {
  position: fixed;
  inset: 0;
  z-index: 999;
}

.lane-menu {
  position: fixed;
  z-index: 1000;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 4px;
  padding: 4px 0;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.6);
  font-family: var(--font);
}

.lane-menu__item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 6px 14px;
  background: none;
  border: none;
  color: var(--color-text);
  font-family: var(--font);
  font-size: 0.75rem;
  letter-spacing: 0.02em;
  text-align: left;
  white-space: nowrap;
  cursor: pointer;
}

.lane-menu__item:hover {
  background: var(--color-border);
  color: #fff;
}

/* Right-aligned, fixed-width column so toggle items show their check on the
   right while plain action items just left-align their label. The width is
   always reserved (glyph blank when inactive) so toggling never resizes the menu. */
.lane-menu__check {
  margin-left: auto;
  width: 1em;
  text-align: center;
  color: var(--color-accent-cyan);
}

/* Still clickable, and it does record the change: only the audio ignores it
   while a solo is up. */
.lane-menu__item--no-effect {
  opacity: 0.45;
}

/* The click is refused outright, so it says so the way the column picker does. */
.lane-menu__item--refused {
  cursor: not-allowed;
  opacity: 0.45;
}

.lane-menu__item--refused:hover {
  background: none;
  color: var(--color-text);
}

.lane-menu__item--sub {
  position: relative;
}

.lane-menu__arrow {
  margin-left: auto;
  font-size: 0.6rem;
  color: var(--color-muted);
}

.lane-menu__submenu {
  display: none;
  position: absolute;
  left: 100%;
  top: -5px;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 4px;
  padding: 4px 0;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.6);
}

.lane-menu__item--sub:hover .lane-menu__submenu {
  display: block;
}
</style>
