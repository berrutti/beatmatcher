<template>
  <Modal :open="open" :title="title" :body="body" :dismissable="false">
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
    <div class="loading-modal__stats" aria-live="polite">
      <span class="loading-modal__phase">{{ label }}</span>
      <span v-if="determinate" class="loading-modal__percent">{{ percent }}%</span>
    </div>
    <div v-if="counts" class="loading-modal__counts">{{ counts }}</div>
    <div v-if="cancelLabel" class="loading-modal__actions">
      <Button class="loading-modal__cancel" @click="emit('cancel')">{{ cancelLabel }}</Button>
    </div>
  </Modal>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import Modal from '@renderer/components/modals/Modal.vue';
import Button from '@renderer/components/Button.vue';

const {
  open,
  title,
  body,
  label,
  fraction,
  determinate = true,
  counts = '',
  cancelLabel = ''
} = defineProps<{
  open: boolean;
  title: string;
  body: string;
  label: string;
  fraction: number;
  determinate?: boolean;
  counts?: string;
  // Given only by work the user is allowed to abandon. The backdrop and escape stay
  // inert either way, so this button is the single exit rather than one of three.
  cancelLabel?: string;
}>();

const emit = defineEmits<{ cancel: [] }>();

// Floored, so the bar never reads 100% while work is still outstanding.
const percent = computed(() => Math.floor(Math.min(1, Math.max(0, fraction)) * 100));
</script>

<style scoped>
.loading-modal__track {
  height: 6px;
  /* Wider than the shared modal minimum, or the bar is too short to read as one. */
  min-width: 340px;
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

.loading-modal__stats,
.loading-modal__counts {
  font-size: 0.65rem;
  letter-spacing: 0.04em;
  color: var(--color-muted);
}

.loading-modal__stats {
  display: flex;
  justify-content: space-between;
}

.loading-modal__actions {
  display: flex;
  justify-content: flex-end;
}

.loading-modal__percent {
  color: var(--color-text);
  font-variant-numeric: tabular-nums;
}
</style>
