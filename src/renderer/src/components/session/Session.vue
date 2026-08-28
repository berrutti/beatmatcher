<template>
  <Modal
    :open="discardModalOpen"
    :title="$t('session.discardTitle')"
    :body="$t('session.discardBody')"
    :confirm-label="$t('session.discardConfirm')"
    @confirm="onDiscardConfirmed"
    @cancel="discardModalOpen = false"
  />

  <ProgressModal
    :open="session.isLoading"
    :title="$t('session.loadingTitle')"
    :body="$t('session.loadingBody')"
    :label="loadLabel"
    :fraction="session.loadedFraction"
    :determinate="loadIsMeasured"
    :counts="loadCounts"
  />

  <ProgressModal
    :open="session.renderProgress !== null"
    :title="$t('session.renderingTitle')"
    :body="$t('session.renderingBody')"
    :label="
      session.renderProgress?.writing
        ? $t('session.renderingPhaseWriting')
        : $t('session.renderingPhaseRendering')
    "
    :fraction="session.renderProgress?.fraction ?? 0"
    :determinate="!session.renderProgress?.writing"
    :cancel-label="session.renderProgress?.writing ? '' : $t('modal.cancel')"
    @cancel="onCancelRender"
  />

  <div class="session" v-bind="$attrs">
    <div class="session__body">
      <div
        v-if="!session.session"
        class="session__drop-zone"
        :class="{ 'session__drop-zone--hover': isFileDragOver }"
        @click="session.openSession()"
      >
        <span class="session__drop-hint">{{ $t('session.dropHint') }}</span>
      </div>
      <template v-else>
        <div v-if="!session.hasTrackInfo" class="session__no-track-info">
          {{ $t('session.noTrackInfo') }}
        </div>
        <div v-if="session.missingTracks.length > 0" class="session__missing">
          <div class="session__missing-header">
            <span class="session__missing-title">{{ $t('session.missingFiles') }}</span>
            <button class="session__missing-btn" @click="editStore.locateMissingTracks()">
              {{ $t('session.locate') }}
            </button>
          </div>
          <div v-for="path in session.missingTracks" :key="path" class="session__missing-row">
            <span class="session__missing-name" v-tooltip="path">{{ basename(path) }}</span>
          </div>
        </div>
        <SessionTimeline
          :duration-ms="session.durationMs"
          :clips="clips"
          :loaded-spans="loadedSpans"
          :playhead-ms="playheadMs"
          :deck-lanes="deckLanes"
          :master-lanes="masterLanes"
          :deck-jog="deckJog"
          :waveforms="session.waveforms"
          @seek="onSeek"
        />
      </template>
    </div>

    <div v-if="session.session" class="session__controls">
      <button
        class="session__btn session__btn--transport session__btn--play"
        :class="{ 'session__btn--active': session.isPlaying }"
        :disabled="session.isLoading"
        @click="onTransport"
      >
        {{ session.isPlaying ? '⏸︎' : '▶︎' }}
      </button>
      <span class="session__duration">
        {{ formatMs(playheadMs) }} / {{ formatMs(session.durationMs) }}
      </span>
      <span class="session__filename">
        {{ session.session.filename }}{{ editStore.dirty ? ' •' : '' }}
      </span>
      <div class="session__controls-right">
        <button
          class="session__btn session__btn--render"
          :disabled="!editStore.dirty"
          @click="editStore.save()"
        >
          {{ $t('modal.save') }}
        </button>
        <button class="session__btn session__btn--render" @click="editStore.saveAs()">
          {{ $t('session.saveAs') }}
        </button>
        <button
          class="session__btn session__btn--render"
          :disabled="isRendering"
          @click="onRender(false)"
        >
          {{ isRendering ? $t('session.rendering') : $t('session.renderWav') }}
        </button>
        <button
          class="session__btn session__btn--render"
          :disabled="isRendering"
          @click="onRender(true)"
        >
          {{ isRendering ? $t('session.rendering') : $t('session.renderFlac') }}
        </button>
        <button class="session__btn session__btn--eject" @click="onEject">⏏</button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
defineOptions({ inheritAttrs: false });
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { storeToRefs } from 'pinia';
import {
  useSessionStore,
  SESSION_LOAD_PHASE_KEYS,
  sessionLoadIsMeasured
} from '@renderer/stores/session';
import { useSessionEditStore } from '@renderer/stores/sessionEdit';
import { useCollectionStore } from '@renderer/stores/collection';
import { useSettingsStore } from '@renderer/stores/settings';
import { useSessionTimeline } from '@renderer/composables/useSessionTimeline';
import SessionTimeline from '@renderer/components/session/Timeline.vue';
import Modal from '@renderer/components/modals/Modal.vue';
import ProgressModal from '@renderer/components/modals/ProgressModal.vue';
import { formatMs, dateStamp } from '@renderer/utils/time';
import { basename } from '@renderer/utils/path';

const { t } = useI18n();
const session = useSessionStore();
const editStore = useSessionEditStore();
const collection = useCollectionStore();
const settingsStore = useSettingsStore();
const { session: sessionRef } = storeToRefs(session);

const { clips, loadedSpans, deckLanes, masterLanes, deckJog } = useSessionTimeline(
  sessionRef,
  (path) => collection.getName(path),
  (path) => {
    const saved = collection.getSaved(path);
    if (saved === null || saved.bpm === null) return null;
    return { bpm: saved.bpm, beatOffsetSec: saved.beatOffset };
  }
);

const loadPhase = computed(() => session.loadProgress?.phase ?? 'parsing');
const loadIsMeasured = computed(() => sessionLoadIsMeasured(loadPhase.value));
const loadLabel = computed(() => t(SESSION_LOAD_PHASE_KEYS[loadPhase.value]));
const loadCounts = computed(() => {
  const total = session.loadProgress?.totalTracks ?? 0;
  return loadIsMeasured.value && total > 0
    ? t('session.loadingTracks', { loaded: session.loadProgress?.loadedTracks ?? 0, total })
    : '';
});

async function onCancelRender(): Promise<void> {
  await session.cancelRender();
}

const isFileDragOver = ref(false);
const isRendering = ref<boolean>(false);
const playheadMs = ref(0);
let rafId = 0;
let playStartWall = 0;
let unlistenDrop: UnlistenFn | null = null;

// Tauri v2 dropped File.path, so an absolute path only arrives on the webview's
// own drag-drop event, not through HTML5 DnD.
onMounted(async () => {
  window.addEventListener('keydown', onKeyDown);
  unlistenDrop = await getCurrentWebview().onDragDropEvent(async (event) => {
    const payload = event.payload;
    if (payload.type === 'enter' || payload.type === 'over') {
      if (!session.session) isFileDragOver.value = true;
    } else if (payload.type === 'leave') {
      isFileDragOver.value = false;
    } else if (payload.type === 'drop') {
      isFileDragOver.value = false;
      const bms = payload.paths.find((p) => p.toLowerCase().endsWith('.bms'));
      if (bms) await session.openSessionFromPath(bms);
    }
  });
});

function isTypingTarget(e: KeyboardEvent): boolean {
  const target = e.target as HTMLElement | null;
  return (
    target !== null &&
    (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable)
  );
}

// Only while nothing is focused, so Tab still walks the controls once a user has
// reached them with the keyboard.
function isBodyFocused(): boolean {
  return document.activeElement === null || document.activeElement === document.body;
}

// preventDefault stops a focused button taking the same keypress, and stops the
// browser moving focus on Tab.
function onKeyDown(e: KeyboardEvent) {
  if (e.repeat) return;
  if (settingsStore.isOpen || discardModalOpen.value || isTypingTarget(e)) return;
  if (e.code === 'Tab') {
    if (!isBodyFocused()) return;
    e.preventDefault();
    editStore.toggleEditMode();
    return;
  }
  if (e.code !== 'Space' || !session.session) return;
  e.preventDefault();
  onTransport().catch(() => {});
}

function tickPlayhead() {
  playheadMs.value = performance.now() - playStartWall;
  if (session.durationMs > 0 && playheadMs.value >= session.durationMs) {
    // Works whether this RAF fires first, or Rust emits session-playback-ended
    // and sets isPlaying=false before this RAF runs.
    if (session.isPlaying) session.stop().catch(() => {});
    playheadMs.value = 0;
    return;
  }
  if (!session.isPlaying) return;
  rafId = requestAnimationFrame(tickPlayhead);
}

async function onTransport() {
  if (session.isLoading) return;
  if (session.isPlaying) {
    cancelAnimationFrame(rafId);
    await session.stop();
  } else {
    await editStore.flushSync();
    playStartWall = performance.now() - playheadMs.value;
    await session.play(playheadMs.value);
    rafId = requestAnimationFrame(tickPlayhead);
  }
}

async function onSeek(ms: number) {
  cancelAnimationFrame(rafId);
  playheadMs.value = ms;
  if (session.isPlaying) {
    playStartWall = performance.now() - ms;
    await session.play(ms);
    rafId = requestAnimationFrame(tickPlayhead);
  }
}

async function onRender(useFlac: boolean) {
  if (!session.session || isRendering.value) return;
  const outputPath = await session.pickRenderOutputPath(
    useFlac,
    t('files.defaultName', { date: dateStamp() })
  );
  if (!outputPath) return;
  isRendering.value = true;
  try {
    await editStore.flushSync();
    await session.renderSession(session.session.path, outputPath, useFlac);
  } finally {
    isRendering.value = false;
  }
}

const discardModalOpen = ref(false);

function onEject() {
  if (editStore.dirty) {
    discardModalOpen.value = true;
    return;
  }
  session.unload().catch(() => {});
}

async function onDiscardConfirmed() {
  discardModalOpen.value = false;
  await session.unload();
}

onUnmounted(() => {
  window.removeEventListener('keydown', onKeyDown);
  cancelAnimationFrame(rafId);
  unlistenDrop?.();
});
</script>

<style scoped>
.session {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  font-family: var(--font);
  background: var(--color-bg);
}

.session__body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.session__no-track-info {
  padding: 8px 16px;
  font-size: 0.8em;
  color: var(--color-muted);
  background: color-mix(in srgb, #f97316 8%, transparent);
  border-bottom: 1px solid color-mix(in srgb, #f97316 30%, transparent);
  letter-spacing: 0.02em;
}

.session__missing {
  padding: 8px 16px;
  font-size: 0.8em;
  background: color-mix(in srgb, var(--color-danger) 8%, transparent);
  border-bottom: 1px solid color-mix(in srgb, var(--color-danger) 30%, transparent);
}

.session__missing-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.75em;
  margin-bottom: 4px;
}

.session__missing-title {
  color: var(--color-danger);
  letter-spacing: 0.02em;
}

.session__missing-row {
  display: flex;
  align-items: center;
  gap: 0.75em;
  padding: 2px 0;
}

.session__missing-name {
  color: var(--color-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.session__missing-btn {
  font-family: var(--font);
  font-size: 0.85em;
  letter-spacing: 0.04em;
  padding: 0.2em 0.8em;
  border-radius: 4px;
  border: 1px solid color-mix(in srgb, var(--color-danger) 40%, transparent);
  background: var(--color-surface);
  color: var(--color-danger);
  cursor: pointer;
  flex-shrink: 0;
  text-transform: uppercase;
}

.session__missing-btn:hover {
  background: color-mix(in srgb, var(--color-danger) 15%, transparent);
}

.session__drop-zone {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  border: 2px dashed transparent;
  transition: border-color 0.15s;
}

.session__drop-zone:hover,
.session__drop-zone--hover {
  border-color: var(--color-border);
}

.session__drop-zone--hover {
  border-color: var(--color-accent-cyan);
}

.session__drop-hint {
  font-size: 1em;
  color: var(--color-muted);
  letter-spacing: 0.04em;
  opacity: 0.6;
  font-style: italic;
  pointer-events: none;
}

.session__controls {
  display: flex;
  align-items: center;
  gap: 0.5em;
  padding: 0 12px;
  height: 44px;
  border-top: 1px solid var(--color-border);
  background: #0d0d0d;
  flex-shrink: 0;
}

.session__btn {
  font-family: var(--font);
  font-size: 0.8em;
  /* Pinned so glyphs from fallback fonts (play/pause are not in JetBrains
     Mono) cannot change the button height between states. */
  line-height: 1.2;
  letter-spacing: 0.04em;
  padding: 0.45em 1.2em;
  border-radius: 4px;
  border: 1px solid var(--color-border);
  background: var(--color-surface);
  color: var(--color-muted);
  cursor: pointer;
  text-transform: uppercase;
}

.session__btn--play {
  min-width: 3.6em;
}

.session__btn--transport:disabled {
  opacity: 0.4;
  cursor: default;
}

.session__btn--transport:hover:not(:disabled):not(.session__btn--active) {
  border-color: var(--color-border-hover);
  color: var(--color-text);
  background: var(--toggle-hover-fill);
}

.session__btn--active {
  background: color-mix(in srgb, var(--color-accent-cyan) var(--toggle-on-fill), transparent);
  border-color: var(--color-accent-cyan);
  color: var(--color-accent-cyan);
}

.session__btn--active:hover:not(:disabled) {
  background: color-mix(in srgb, var(--color-accent-cyan) var(--toggle-on-fill-hover), transparent);
}

.session__duration {
  font-size: 0.8em;
  color: var(--color-muted);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
  margin-left: 4px;
}

.session__filename {
  font-size: 0.75em;
  color: var(--color-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 200px;
  opacity: 0.6;
}

.session__controls-right {
  margin-left: auto;
  display: flex;
  gap: 0.5em;
}

.session__btn--render {
  color: var(--color-muted);
}

.session__btn--render:hover:not(:disabled) {
  border-color: var(--color-border-hover);
  color: var(--color-text);
  background: var(--toggle-hover-fill);
}

.session__btn--render:disabled {
  opacity: 0.4;
  cursor: default;
}

.session__btn--eject:hover {
  border-color: var(--color-border-hover);
  color: var(--color-text);
  background: var(--toggle-hover-fill);
}
</style>
