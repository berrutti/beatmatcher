<template>
  <div class="session">
    <div class="session__body">
      <div
        v-if="!session.session"
        class="session__drop-zone"
        :class="{ 'session__drop-zone--hover': isFileDragOver }"
        @click="session.openSession()"
        @dragover.prevent="isFileDragOver = true"
        @dragleave="isFileDragOver = false"
        @drop.prevent="onFileDrop"
      >
        <span class="session__drop-hint">Drop a recorded session here, or click to open</span>
      </div>
      <template v-else>
        <div v-if="!session.hasTrackInfo" class="session__no-track-info">
          No track info. Load tracks in Performance mode first, then record.
        </div>
        <SessionTimeline
          :duration-ms="session.durationMs"
          :clips="clips"
          :loaded-spans="loadedSpans"
          :playhead-ms="playheadMs"
          @seek="onSeek"
        />
      </template>
    </div>

    <div v-if="session.session" class="session__controls">
      <button
        class="session__btn session__btn--transport"
        :class="{ 'session__btn--active': session.isPlaying }"
        @click="onTransport"
      >
        {{ session.isPlaying ? '■' : '▶' }}
      </button>
      <span class="session__duration">
        {{ formatMs(playheadMs) }} / {{ formatMs(session.durationMs) }}
      </span>
      <span class="session__filename">{{ session.session.filename }}</span>
      <div class="session__controls-right">
        <button
          class="session__btn session__btn--render"
          :disabled="isRendering"
          @click="onRender(false)"
        >
          {{ isRendering ? 'RENDERING...' : 'RENDER WAV' }}
        </button>
        <button
          class="session__btn session__btn--render"
          :disabled="isRendering"
          @click="onRender(true)"
        >
          {{ isRendering ? 'RENDERING...' : 'RENDER FLAC' }}
        </button>
        <button class="session__btn session__btn--eject" @click="session.unload()">⏏</button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onUnmounted } from 'vue';
import { storeToRefs } from 'pinia';
import { useSessionStore } from '@renderer/stores/session';
import { useCollectionStore } from '@renderer/stores/collection';
import { useMixerStore } from '@renderer/stores/mixer';
import { useSessionTimeline } from '@renderer/composables/useSessionTimeline';
import SessionTimeline from '@renderer/components/session/Timeline.vue';
import { formatMs } from '@renderer/utils/time';

const session = useSessionStore();
const collection = useCollectionStore();
const mixer = useMixerStore();
const { session: sessionRef } = storeToRefs(session);

const { clips, loadedSpans } = useSessionTimeline(sessionRef, (path) => collection.getName(path));

const isFileDragOver = ref(false);
const isRendering = ref<boolean>(false);
const playheadMs = ref(0);
let rafId = 0;
let playStartWall = 0;

async function onFileDrop(e: DragEvent) {
  isFileDragOver.value = false;
  const file = e.dataTransfer?.files[0];
  if (!file) return;
  // Tauri exposes a .path property on dropped File objects
  const path = (file as File & { path?: string }).path;
  if (path) {
    await session.openSessionFromPath(path);
  } else {
    await session.openSession();
  }
}

function tickPlayhead() {
  if (!session.isPlaying) return;
  playheadMs.value = performance.now() - playStartWall;
  if (session.durationMs > 0 && playheadMs.value >= session.durationMs) {
    playheadMs.value = session.durationMs;
    void session.stop();
    return;
  }
  rafId = requestAnimationFrame(tickPlayhead);
}

async function onTransport() {
  if (session.isPlaying) {
    cancelAnimationFrame(rafId);
    await session.stop();
  } else {
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
  const outputPath = await mixer.pickRenderOutputPath(useFlac);
  if (!outputPath) return;
  isRendering.value = true;
  try {
    await mixer.renderSession(session.session.path, outputPath, useFlac);
  } finally {
    isRendering.value = false;
  }
}

onUnmounted(() => cancelAnimationFrame(rafId));
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
  letter-spacing: 0.05em;
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
  border-color: #06b6d4;
}

.session__drop-hint {
  font-size: 1em;
  color: var(--color-muted);
  letter-spacing: 0.1em;
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
  letter-spacing: 0.1em;
  padding: 0.45em 1.2em;
  border-radius: 4px;
  border: 1px solid var(--color-border);
  background: var(--color-surface);
  color: var(--color-muted);
  cursor: pointer;
}

.session__btn--transport:hover,
.session__btn--active {
  background: color-mix(in srgb, #06b6d4 15%, transparent);
  border-color: #06b6d4;
  color: #06b6d4;
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
}

.session__btn--render {
  color: var(--color-muted);
}

.session__btn--render:hover:not(:disabled) {
  background: color-mix(in srgb, #06b6d4 15%, transparent);
  border-color: #06b6d4;
  color: #06b6d4;
}

.session__btn--render:disabled {
  opacity: 0.4;
  cursor: default;
}

.session__btn--eject:hover {
  color: var(--color-text);
  border-color: var(--color-text);
}
</style>
