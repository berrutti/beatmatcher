<template>
  <div v-if="visible" class="save-progress" role="status">
    <div class="save-progress__row">
      <span class="save-progress__label">{{ $t('topStrip.savingRecording') }}</span>
      <span class="save-progress__pct">{{ Math.round(fraction * 100) }}%</span>
      <button
        class="save-progress__dismiss"
        v-tooltip="$t('topStrip.hideProgress')"
        @click="dismissed = true"
      >
        ✕
      </button>
    </div>
    <div class="save-progress__track">
      <div class="save-progress__fill" :style="{ width: `${fraction * 100}%` }" />
    </div>
    <p class="save-progress__hint">{{ $t('topStrip.savingHint') }}</p>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useMixerStore } from '@renderer/stores/mixer';

const mixer = useMixerStore();

// Dismissing hides the readout, never the save. Reset when the next one starts,
// or a dismissal would silence every save for the rest of the session.
const dismissed = ref(false);
const fraction = computed(() => mixer.saveProgress ?? 0);
const visible = computed(() => mixer.saveProgress !== null && !dismissed.value);

watch(
  () => mixer.saveProgress,
  (value) => {
    if (value === null) dismissed.value = false;
  }
);
</script>

<style scoped>
.save-progress {
  position: fixed;
  right: 12px;
  bottom: 12px;
  z-index: 900;
  width: 240px;
  padding: 10px 12px;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: var(--radius);
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.6);
  font-family: var(--font);
}

.save-progress__row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.save-progress__label {
  flex: 1;
  font-size: 10px;
  letter-spacing: var(--label-letter-spacing);
  text-transform: uppercase;
  color: var(--color-text);
}

.save-progress__pct {
  font-size: 10px;
  font-variant-numeric: tabular-nums;
  color: var(--color-muted);
}

.save-progress__dismiss {
  background: transparent;
  border: none;
  color: var(--color-muted);
  font-size: 11px;
  line-height: 1;
  padding: 0 2px;
  cursor: pointer;
}

.save-progress__dismiss:hover {
  color: var(--color-text);
}

.save-progress__track {
  margin-top: 8px;
  height: 4px;
  background: var(--color-border);
  border-radius: 2px;
  overflow: hidden;
}

.save-progress__fill {
  height: 100%;
  background: var(--color-accent-cyan);
  transition: width 0.2s linear;
}

.save-progress__hint {
  margin: 6px 0 0;
  font-size: 9px;
  color: var(--color-muted);
}
</style>
