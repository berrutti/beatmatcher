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
    <!-- Deck right-click menu: mute/solo, nudge delete, and the lanes submenu. -->
    <div
      v-if="deckMenu"
      class="lane-menu"
      :style="{ left: deckMenu.x + 'px', top: deckMenu.y + 'px' }"
      @click.stop
    >
      <button v-if="deckMenu.nudge" class="lane-menu__item" @click="onDeleteNudge">
        {{ $t('session.deleteNudge') }}
      </button>
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
      <button class="lane-menu__item" @click="onToggleMute">
        {{ $t('session.mute') }}
        <span class="lane-menu__check">{{
          sessionStore.mutedDecks.has(deckMenu.deck) ? '✓' : ''
        }}</span>
      </button>
      <button class="lane-menu__item" @click="onToggleSolo">
        {{ $t('session.solo') }}
        <span class="lane-menu__check">{{
          sessionStore.soloDecks.has(deckMenu.deck) ? '✓' : ''
        }}</span>
      </button>
      <div v-if="editStore.editMode" class="lane-menu__item lane-menu__item--sub">
        {{ $t('session.lanesMenu') }}
        <span class="lane-menu__arrow">▶</span>
        <div class="lane-menu__submenu">
          <button
            v-for="key in LANE_KEYS"
            :key="key"
            class="lane-menu__item"
            @click="onPickLaneFromMenu(key)"
          >
            {{ $t(`session.lanes.${key}`) }}
            <span class="lane-menu__check">{{
              controller.laneFor(deckMenu.deck) === key ? '✓' : ''
            }}</span>
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

    <!-- Lane dropdown: choose which automation lane a deck shows. -->
    <div
      v-if="lanePicker"
      class="lane-menu"
      :style="{ left: lanePicker.x + 'px', top: lanePicker.y + 'px' }"
      @click.stop
    >
      <button v-for="key in LANE_KEYS" :key="key" class="lane-menu__item" @click="onPickLane(key)">
        {{ $t(`session.lanes.${key}`) }}
        <span class="lane-menu__check">{{
          controller.laneFor(lanePicker.deck) === key ? '✓' : ''
        }}</span>
      </button>
    </div>
    <div
      v-if="lanePicker"
      class="lane-menu__backdrop"
      @click="lanePicker = null"
      @contextmenu.prevent="lanePicker = null"
    />

    <!-- Filter-region right-click menu: delete (mirrors the nudge delete). -->
    <div
      v-if="filterMenu"
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

    <!-- Set-BPM dialog: insert a rate change at the right-clicked point. -->
    <BpmModal
      :open="bpmDialog !== null"
      :current-bpm="bpmDialog?.currentBpm ?? null"
      @submit="onSetBpm"
      @cancel="bpmDialog = null"
    />
  </Teleport>
</template>

<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted } from 'vue';
import type { Clip, LoadedSpan, DeckLanes, MasterLanes, NudgeSpan } from '@renderer/utils/types';
import {
  DECK_ORDER,
  LANE_KEYS,
  LABEL_W,
  PADDING,
  makeMsToX,
  type LaneKey,
  type RowLayout,
  type TrackWaveform
} from '@renderer/utils/timelineDraw';
import { overlapsRange } from '@renderer/utils/timelineView';
import { renderScene, type SceneItem, type ViewContext } from '@renderer/utils/timelineEngine';
import { useTimelineView } from '@renderer/composables/useTimelineView';
import { useTimelineController } from '@renderer/composables/useTimelineController';
import { useTimelineGestures } from '@renderer/composables/useTimelineGestures';
import { buildScene } from '@renderer/composables/useTimelineScene';
import { useSessionStore } from '@renderer/stores/session';
import { useSessionEditStore } from '@renderer/stores/sessionEdit';
import { useSettingsStore } from '@renderer/stores/settings';
import BpmModal from '@renderer/components/modals/BpmModal.vue';
import type { BpmContext } from '@renderer/utils/timelineIntents';

// Waveform LOD: as the user zooms, refetch only the VISIBLE track region of each
// visible track at ~one point per physical pixel (oversampled a touch). Because
// it's region-based, detail scales with zoom regardless of track length.
const MIN_REGION_POINTS = 256;
const MAX_REGION_POINTS = 16000;
const BASE_REGION_POINTS_MAX = 4000;
const LOD_OVERSAMPLE = 1.5;
const LOD_DEBOUNCE_MS = 150;

const props = defineProps<{
  durationMs: number;
  clips: Clip[];
  loadedSpans: LoadedSpan[];
  playheadMs: number;
  deckLanes: Record<string, DeckLanes>;
  masterLanes: MasterLanes;
  deckNudges: Record<string, NudgeSpan[]>;
  waveforms: Map<string, TrackWaveform>;
}>();

const emit = defineEmits<{ seek: [ms: number] }>();

const sessionStore = useSessionStore();
const editStore = useSessionEditStore();
const settingsStore = useSettingsStore();

const containerEl = ref<HTMLDivElement | null>(null);
const scrollEl = ref<HTMLDivElement | null>(null);
const sizerEl = ref<HTMLDivElement | null>(null);
const canvasEl = ref<HTMLCanvasElement | null>(null);

const camera = useTimelineView(() => props.durationMs);
const controller = useTimelineController({
  camera,
  getClips: () => props.clips,
  emitSeek: (ms) => emit('seek', ms),
  requestRender: scheduleRender
});
const { deckMenu, lanePicker, filterMenu } = controller;

// The scene built on the last frame; pointer hit-testing runs against it with a
// freshly computed ViewContext (same canvas size, so geometry stays consistent).
// The rows ride along for the marquee's rect-to-deck mapping.
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
  laneHeight: () => controller.laneHeight.value,
  waveformHeight: () => controller.waveformHeight.value,
  isEditMode: () => editStore.editMode,
  durationMs: () => props.durationMs,
  nudgeDirectionAt: (_deck, y, rowTop) =>
    y < rowTop + controller.waveformHeight.value / 2 ? 1 : -1,
  nudgeSensitivity: () => settingsStore.nudgeSensitivity,
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

  const vc = camera.viewContext(cw, ch);
  const scene = buildScene({
    vc,
    decks: DECK_ORDER,
    clips: props.clips,
    loadedSpans: props.loadedSpans,
    deckLanes: props.deckLanes,
    masterLanes: props.masterLanes,
    deckNudges: props.deckNudges,
    waveforms: props.waveforms,
    playheadMs: props.playheadMs,
    durationMs: props.durationMs,
    editMode: editStore.editMode,
    laneFor: controller.laneFor,
    laneHeight: controller.laneHeight.value,
    waveformHeight: controller.waveformHeight.value,
    accentFor: controller.accentFor,
    audibleFor: (deck) => sessionStore.deckAudible(deck),
    soloFor: (deck) => sessionStore.soloDecks.has(deck),
    mutedFor: (deck) => sessionStore.mutedDecks.has(deck),
    clipSelection: controller.clipSelection.value,
    filterSelection: controller.filterSelection.value,
    overlays: gestures.overlays()
  });

  camera.setContentMetrics(scene.contentHeight, vc.scrollViewport.bottom - vc.scrollViewport.top);
  // The spacer's height is the scrollable amount, so the native scrollbar's
  // range matches camera.scrollY exactly. Re-sync scrollTop in case the content
  // shrank and setContentMetrics clamped scrollY below the element's position.
  if (sizerEl.value) sizerEl.value.style.height = `${camera.maxScrollY()}px`;
  if (scroll.scrollTop !== camera.scrollY.value) scroll.scrollTop = camera.scrollY.value;
  sceneItems = scene.items;
  sceneRows = scene.rows;
  renderScene(ctx, scene.items, vc);
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

// Last hover position (canvas-local), kept so a Shift press/release can refresh
// the cursor without the pointer moving.
let lastHoverPoint: { x: number; y: number } | null = null;

function applyHoverCursor(shiftKey: boolean): void {
  if (gestures.hasActive() || !canvasEl.value || !lastHoverPoint) return;
  canvasEl.value.style.cursor = gestures.cursorFor(lastHoverPoint, shiftKey);
}

function onCanvasHoverMove(e: MouseEvent): void {
  if (gestures.hasActive() || !canvasEl.value) return;
  const rect = canvasEl.value.getBoundingClientRect();
  lastHoverPoint = { x: e.clientX - rect.left, y: e.clientY - rect.top };
  applyHoverCursor(e.shiftKey);
}

function onCanvasHoverLeave(): void {
  lastHoverPoint = null;
  if (!gestures.hasActive() && canvasEl.value) canvasEl.value.style.cursor = '';
}

function onDeleteNudge(): void {
  const menu = deckMenu.value;
  deckMenu.value = null;
  if (!menu?.nudge) return;
  editStore.deleteNudge(menu.deck, menu.nudge.startMs, menu.nudge.endMs).catch(() => {});
}

function onToggleMute(): void {
  if (deckMenu.value) sessionStore.toggleMute(deckMenu.value.deck);
  deckMenu.value = null;
}

function onToggleSolo(): void {
  if (deckMenu.value) sessionStore.toggleSolo(deckMenu.value.deck);
  deckMenu.value = null;
}

function onPickLaneFromMenu(lane: LaneKey): void {
  if (deckMenu.value) controller.setDeckLane(deckMenu.value.deck, lane);
  deckMenu.value = null;
}

function onPickLane(lane: LaneKey): void {
  if (lanePicker.value) controller.setDeckLane(lanePicker.value.deck, lane);
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

// Ask the session store for a finer waveform on each visible track when the zoom
// (or canvas width) calls for more detail than is loaded. The store no-ops when
// it already has enough points; redraws happen when the new data lands.
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
  lodTimer = setTimeout(updateWaveformLod, LOD_DEBOUNCE_MS);
}

// Zoom (duration), pan (start), and clips arriving all change which track region
// needs detail, so refetch on any of them (debounced).
watch(
  () => [camera.viewStartMs.value, camera.viewDurationMs.value, props.clips],
  scheduleWaveformLod,
  { immediate: true }
);

// Keep the playhead on screen while playing: if it runs off either edge of the
// zoomed-in view, the view jumps so it lands near the left edge with a lead-in.
watch(
  () => props.playheadMs,
  (ms) => camera.followPlayhead(ms)
);

// Delete/Backspace removes whichever editable thing is selected (clip takes
// precedence over a filter span). Edit-mode only; the commit stops playback.
function onKeyDown(e: KeyboardEvent): void {
  if (e.key === 'Shift') {
    applyHoverCursor(true);
    return;
  }
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

function onKeyUp(e: KeyboardEvent): void {
  if (e.key === 'Shift') applyHoverCursor(false);
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
  window.addEventListener('keyup', onKeyUp);
  scheduleRender();
});

onUnmounted(() => {
  ro?.disconnect();
  if (lodTimer !== null) clearTimeout(lodTimer);
  window.removeEventListener('keydown', onKeyDown);
  window.removeEventListener('keyup', onKeyUp);
  window.removeEventListener('mousemove', onWindowMove);
  window.removeEventListener('mouseup', onWindowUp);
});

// Anything that changes the picture redraws (the camera's own state, the props,
// the interaction state, and per-deck accents).
watch(
  () => [
    props.clips,
    props.loadedSpans,
    props.durationMs,
    props.playheadMs,
    props.deckLanes,
    props.masterLanes,
    props.deckNudges,
    props.waveforms,
    camera.viewStartMs.value,
    camera.viewDurationMs.value,
    camera.scrollY.value,
    controller.selectedDeckLane.value,
    controller.laneHeight.value,
    controller.waveformHeight.value,
    controller.clipSelection.value,
    controller.filterSelection.value,
    editStore.editMode,
    sessionStore.mutedDecks,
    sessionStore.soloDecks,
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
  letter-spacing: 0.05em;
  text-align: left;
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
