<template>
  <span v-if="status === 'analyzing'" class="track-bpm-cell__tag">
    {{ $t('browser.detecting') }}
  </span>
  <span v-else-if="bpm !== null" class="track-bpm-cell__value">{{ bpm.toFixed(1) }} BPM</span>
  <button
    v-else-if="status === 'ready'"
    class="track-bpm-cell__btn"
    tabindex="-1"
    @click.stop="onSetBpm"
  >
    {{ $t('browser.setBpm') }}
  </button>
  <button
    v-else-if="status === 'idle'"
    class="track-bpm-cell__btn"
    tabindex="-1"
    @click.stop="onAnalyze"
  >
    {{ $t('browser.analyze') }}
  </button>
</template>

<script setup lang="ts">
import type { CollectionEntryStatus } from '@renderer/stores/collection';

defineProps<{
  status: CollectionEntryStatus | null;
  bpm: number | null;
  onAnalyze: () => void;
  onSetBpm: () => void;
}>();
</script>

<style scoped>
.track-bpm-cell__value {
  color: var(--color-muted);
  font-size: 0.9em;
  white-space: nowrap;
  letter-spacing: 0.02em;
}

.track-bpm-cell__tag {
  font-size: 0.85em;
  color: var(--color-muted);
  opacity: 0.6;
  white-space: nowrap;
}

.track-bpm-cell__btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.85em;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  padding: 0.15em 0.5em;
  border-radius: 3px;
  cursor: pointer;
  white-space: nowrap;
}

.track-bpm-cell__btn:hover {
  border-color: #555;
  color: var(--color-text);
}
</style>
