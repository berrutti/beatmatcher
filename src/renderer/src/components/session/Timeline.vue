<template>
  <div class="timeline" ref="containerEl">
    <canvas
      ref="canvasEl"
      class="timeline__canvas"
      @click="onCanvasClick"
      @dblclick="onCanvasDblClick"
      @contextmenu.prevent="onCanvasContextMenu"
      @wheel.prevent="onCanvasWheel"
      @mousedown="onCanvasMouseDown"
      @mousemove="onCanvasHoverMove"
      @mouseleave="onCanvasHoverLeave"
    />
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
        <span class="lane-menu__check"></span>
        {{ $t('session.deleteNudge') }}
      </button>
      <button class="lane-menu__item" @click="onToggleMute">
        <span class="lane-menu__check">{{
          sessionStore.mutedDecks.has(deckMenu.deck) ? '✓' : ''
        }}</span>
        {{ $t('session.mute') }}
      </button>
      <button class="lane-menu__item" @click="onToggleSolo">
        <span class="lane-menu__check">{{
          sessionStore.soloDecks.has(deckMenu.deck) ? '✓' : ''
        }}</span>
        {{ $t('session.solo') }}
      </button>
      <div v-if="editStore.editMode" class="lane-menu__item lane-menu__item--sub">
        <span class="lane-menu__check"></span>
        {{ $t('session.lanesMenu') }}
        <span class="lane-menu__arrow">▶</span>
        <div class="lane-menu__submenu">
          <button
            v-for="key in LANE_KEYS"
            :key="key"
            class="lane-menu__item"
            @click="onPickLaneFromMenu(key)"
          >
            <span class="lane-menu__check">{{
              controller.laneFor(deckMenu.deck) === key ? '✓' : ''
            }}</span>
            {{ $t(`session.lanes.${key}`) }}
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
        <span class="lane-menu__check">{{
          controller.laneFor(lanePicker.deck) === key ? '✓' : ''
        }}</span>
        {{ $t(`session.lanes.${key}`) }}
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
        <span class="lane-menu__check"></span>
        {{ $t('session.deleteFilterRegion') }}
      </button>
    </div>
    <div
      v-if="filterMenu"
      class="lane-menu__backdrop"
      @click="filterMenu = null"
      @contextmenu.prevent="filterMenu = null"
    />
  </Teleport>
</template>

<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted } from 'vue';
import type {
  Clip,
  LoadedSpan,
  DeckLanes,
  MasterLanes,
  NudgeSpan
} from '@renderer/composables/useSessionTimeline';
import {
  DECK_ORDER,
  LANE_KEYS,
  LABEL_W,
  PADDING,
  makeMsToX,
  type LaneKey,
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
let sceneItems: SceneItem[] = [];

function viewContext(): ViewContext {
  const c = containerEl.value;
  return camera.viewContext(c?.clientWidth ?? 0, c?.clientHeight ?? 0);
}

const gestures = useTimelineGestures({
  camera,
  emit: controller.handleIntent,
  getItems: () => sceneItems,
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
  const container = containerEl.value;
  if (!canvas || !container) return;
  const dpr = window.devicePixelRatio || 1;
  const cw = container.clientWidth;
  const ch = container.clientHeight;
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
    scrollY: camera.scrollY.value,
    maxScrollY: camera.maxScrollY(),
    overlays: gestures.overlays()
  });

  camera.setContentMetrics(scene.contentHeight, vc.scrollViewport.bottom - vc.scrollViewport.top);
  sceneItems = scene.items;
  renderScene(ctx, scene.items, vc);
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
  if (e.key !== 'Delete' && e.key !== 'Backspace') return;
  if (!editStore.editMode) return;
  if (controller.clipSelection.value) {
    e.preventDefault();
    controller.deleteSelectedClip();
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
    controller.clipSelection.value = null;
    controller.unlockedBlockIds.value = new Set();
  }
);
watch(
  () => editStore.editMode,
  (on) => {
    if (!on) {
      controller.clipSelection.value = null;
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
  if (containerEl.value) ro.observe(containerEl.value);
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

.timeline__canvas {
  display: block;
}

.lane-menu__backdrop {
  position: fixed;
  inset: 0;
  z-index: 999;
}

.lane-menu {
  position: fixed;
  z-index: 1000;
  background: #1a1a1a;
  border: 1px solid #333;
  border-radius: 4px;
  padding: 4px 0;
  min-width: 140px;
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
  background: #2a2a2a;
  color: #fff;
}

.lane-menu__check {
  display: inline-block;
  width: 1em;
  color: #06b6d4;
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
  background: #1a1a1a;
  border: 1px solid #333;
  border-radius: 4px;
  padding: 4px 0;
  min-width: 140px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.6);
}

.lane-menu__item--sub:hover .lane-menu__submenu {
  display: block;
}
</style>
