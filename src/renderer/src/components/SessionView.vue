<template>
  <div class="session">
    <div class="session__header">
      <span class="session__label">SESSION</span>
      <div class="session__header-center">
        <template v-if="session.session">
          <span class="session__filename">{{ session.session.filename }}</span>
          <span class="session__duration">
            {{ formatMs(playheadMs) }} / {{ formatMs(session.durationMs) }}
          </span>
          <span v-if="!session.hasTrackInfo" class="session__warning">
            No track info. Load tracks manually in Performance mode first.
          </span>
        </template>
      </div>
      <div class="session__header-right">
        <button
          class="session__btn"
          @click="session.openSession()"
        >
          Load
        </button>
        <template v-if="session.session">
          <button
            class="session__btn session__btn--transport"
            :class="{ 'session__btn--active': session.isPlaying }"
            @click="onTransport"
          >
            {{ session.isPlaying ? '■' : '▶' }}
          </button>
        </template>
        <button class="session__close" @click="emit('close')">✕</button>
      </div>
    </div>

    <div class="session__body">
      <div v-if="!session.session" class="session__empty">
        <button class="session__load-btn" @click="session.openSession()">Load session file</button>
      </div>
      <SessionTimeline
        v-else
        :duration-ms="session.durationMs"
        :clips="clips"
        :playhead-ms="playheadMs"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onUnmounted } from 'vue';
import { storeToRefs } from 'pinia';
import { useSessionStore } from '@renderer/stores/session';
import { useSessionTimeline } from '@renderer/composables/useSessionTimeline';
import SessionTimeline from '@renderer/components/session/SessionTimeline.vue';

const emit = defineEmits<{ close: [] }>();
const session = useSessionStore();
const { session: sessionRef } = storeToRefs(session);
const { clips } = useSessionTimeline(sessionRef as never);

const playheadMs = ref(0);
let rafId = 0;
let playStartWall = 0;

function formatMs(ms: number): string {
  const totalSec = Math.floor(ms / 1000);
  const h = Math.floor(totalSec / 3600);
  const m = Math.floor((totalSec % 3600) / 60);
  const s = totalSec % 60;
  if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
  return `${m}:${String(s).padStart(2, '0')}`;
}

function tickPlayhead() {
  if (!session.isPlaying) return;
  playheadMs.value = performance.now() - playStartWall;
  rafId = requestAnimationFrame(tickPlayhead);
}

async function onTransport() {
  if (session.isPlaying) {
    cancelAnimationFrame(rafId);
    playheadMs.value = 0;
    await session.stop();
  } else {
    playStartWall = performance.now();
    await session.play();
    rafId = requestAnimationFrame(tickPlayhead);
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

.session__header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 0 12px;
  height: 32px;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}

.session__label {
  font-size: 10px;
  letter-spacing: 0.15em;
  color: #06b6d4;
  font-weight: 700;
  flex-shrink: 0;
}

.session__header-center {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
  overflow: hidden;
}

.session__filename {
  font-size: 11px;
  color: var(--color-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.session__duration {
  font-size: 10px;
  color: var(--color-muted);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
  flex-shrink: 0;
}

.session__warning {
  font-size: 10px;
  color: #f59e0b;
  white-space: nowrap;
}

.session__header-right {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}

.session__btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 10px;
  letter-spacing: 0.1em;
  padding: 2px 10px;
  border-radius: 3px;
  cursor: pointer;
}

.session__btn:hover {
  border-color: #06b6d4;
  color: #06b6d4;
}

.session__btn--transport {
  min-width: 28px;
  text-align: center;
}

.session__btn--active {
  border-color: #06b6d4;
  color: #06b6d4;
  background: color-mix(in srgb, #06b6d4 12%, transparent);
}

.session__close {
  background: transparent;
  border: none;
  color: var(--color-muted);
  font-size: 13px;
  cursor: pointer;
  padding: 2px 4px;
  line-height: 1;
}

.session__close:hover {
  color: var(--color-text);
}

.session__body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.session__empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}

.session__load-btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 10px;
  letter-spacing: 0.12em;
  padding: 8px 24px;
  border-radius: 3px;
  cursor: pointer;
}

.session__load-btn:hover {
  border-color: #06b6d4;
  color: #06b6d4;
}
</style>
