<template>
  <Transition name="loading-modal">
    <div v-if="open" class="loading-modal__backdrop">
      <div class="loading-modal" role="alertdialog" aria-live="polite" :aria-busy="true">
        <div class="loading-modal__title">{{ $t('session.loadingTitle') }}</div>
        <div class="loading-modal__body">{{ $t('session.loadingBody') }}</div>
        <div
          v-if="determinate"
          class="loading-modal__track"
          role="progressbar"
          aria-valuemin="0"
          aria-valuemax="100"
          :aria-valuenow="percent"
        >
          <div class="loading-modal__fill" :style="{ width: `${percent}%` }" />
        </div>
        <div v-else class="loading-modal__track" role="progressbar">
          <div class="loading-modal__fill loading-modal__fill--indeterminate" />
        </div>
        <div class="loading-modal__stats">
          <span class="loading-modal__phase">{{ phaseLabel }}</span>
          <span v-if="determinate" class="loading-modal__percent">{{ percent }}%</span>
        </div>
        <div v-if="counts" class="loading-modal__counts">{{ counts }}</div>
      </div>
    </div>
  </Transition>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import type { SessionLoadPhase } from '@renderer/stores/session';

const { open, phase, fraction, loadedTracks, totalTracks } = defineProps<{
  open: boolean;
  phase: SessionLoadPhase;
  fraction: number;
  loadedTracks: number;
  totalTracks: number;
}>();

const { t } = useI18n();

// Only the decode reports increments. Reading, parsing and indexing each take
// one long step, so a percentage there would sit at 0 and read as a hang.
const determinate = computed(() => phase === 'decoding' || phase === 'done');

// Floored, so the bar never reads 100% while a track is still decoding.
const percent = computed(() => Math.floor(Math.min(1, Math.max(0, fraction)) * 100));

const PHASE_LABELS: Record<SessionLoadPhase, string> = {
  reading: 'session.loadingPhaseReading',
  parsing: 'session.loadingPhaseParsing',
  decoding: 'session.loadingPhaseDecoding',
  indexing: 'session.loadingPhaseIndexing',
  done: 'session.loadingPhaseIndexing'
};

const phaseLabel = computed(() => t(PHASE_LABELS[phase] ?? PHASE_LABELS.decoding));

const counts = computed(() =>
  determinate.value && totalTracks > 0
    ? t('session.loadingTracks', { loaded: loadedTracks, total: totalTracks })
    : ''
);
</script>

<style scoped>
.loading-modal__backdrop {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.7);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 200;
}

.loading-modal {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 8px;
  padding: 24px;
  min-width: 340px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.loading-modal__title {
  font-size: 0.85rem;
  font-weight: 700;
  letter-spacing: 0.04em;
  color: var(--color-text);
}

.loading-modal__body {
  font-size: 0.7rem;
  line-height: 1.5;
  color: var(--color-muted);
}

.loading-modal__track {
  height: 6px;
  border-radius: 3px;
  background: #1a1a1a;
  border: 1px solid var(--color-border);
  overflow: hidden;
}

.loading-modal__fill {
  height: 100%;
  background: var(--color-accent, #3b82f6);
  transition: width 0.2s ease;
}

/* transform, never background-position: WKWebView blanks the whole UI per frame
   when that property is animated. */
.loading-modal__fill--indeterminate {
  width: 35%;
  animation: loading-modal-sweep 1.1s ease-in-out infinite;
}

@keyframes loading-modal-sweep {
  0% {
    transform: translateX(-100%);
  }
  100% {
    transform: translateX(386%);
  }
}

.loading-modal__stats {
  display: flex;
  justify-content: space-between;
  font-size: 0.65rem;
  letter-spacing: 0.04em;
  color: var(--color-muted);
}

.loading-modal__percent {
  color: var(--color-text);
  font-variant-numeric: tabular-nums;
}

.loading-modal-enter-active,
.loading-modal-leave-active {
  transition: opacity 0.15s ease;
}

.loading-modal-enter-from,
.loading-modal-leave-to {
  opacity: 0;
}
</style>
