<template>
  <div class="session">
    <div class="session__header">
      <span class="session__label">SESSION</span>
      <button class="session__close" @click="emit('close')">✕</button>
    </div>

    <div class="session__body">
      <div v-if="!session.session" class="session__empty">
        <button class="session__load-btn" @click="session.openSession()">Load session</button>
      </div>

      <template v-else>
        <div class="session__info">
          <span class="session__filename">{{ session.session.filename }}</span>
          <span class="session__duration">{{ formatMs(session.durationMs) }}</span>
          <span v-if="!session.hasTrackInfo" class="session__warning">
            No track info in this session. Load tracks manually in Performance mode first.
          </span>
        </div>

        <div class="session__controls">
          <button class="session__load-btn session__load-btn--small" @click="session.openSession()">
            Load session
          </button>
          <button
            class="session__transport-btn"
            :class="{ 'session__transport-btn--active': session.isPlaying }"
            @click="onTransport"
          >
            {{ session.isPlaying ? '■ Stop' : '▶ Play' }}
          </button>
        </div>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useSessionStore } from '@renderer/stores/session';

const emit = defineEmits<{ close: [] }>();
const session = useSessionStore();

function formatMs(ms: number): string {
  const totalSec = Math.floor(ms / 1000);
  const h = Math.floor(totalSec / 3600);
  const m = Math.floor((totalSec % 3600) / 60);
  const s = totalSec % 60;
  if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
  return `${m}:${String(s).padStart(2, '0')}`;
}

async function onTransport() {
  if (session.isPlaying) {
    await session.stop();
  } else {
    await session.play();
  }
}
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
  justify-content: space-between;
  padding: 8px 16px;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}

.session__label {
  font-size: 11px;
  letter-spacing: 0.15em;
  color: #06b6d4;
  font-weight: 700;
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
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 24px;
}

.session__empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
}

.session__info {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
}

.session__filename {
  font-size: 13px;
  color: var(--color-text);
  letter-spacing: 0.05em;
}

.session__duration {
  font-size: 11px;
  color: var(--color-muted);
  font-variant-numeric: tabular-nums;
}

.session__warning {
  font-size: 10px;
  color: #f59e0b;
  max-width: 320px;
  text-align: center;
  line-height: 1.5;
}

.session__controls {
  display: flex;
  align-items: center;
  gap: 12px;
}

.session__load-btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 10px;
  letter-spacing: 0.12em;
  padding: 6px 16px;
  border-radius: 3px;
  cursor: pointer;
}

.session__load-btn:hover {
  border-color: #06b6d4;
  color: #06b6d4;
}

.session__load-btn--small {
  font-size: 9px;
  padding: 4px 10px;
}

.session__transport-btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 11px;
  letter-spacing: 0.1em;
  padding: 8px 28px;
  border-radius: 3px;
  cursor: pointer;
  min-width: 100px;
}

.session__transport-btn:hover {
  border-color: #06b6d4;
  color: #06b6d4;
}

.session__transport-btn--active {
  border-color: #06b6d4;
  color: #06b6d4;
  background: color-mix(in srgb, #06b6d4 12%, transparent);
}
</style>
