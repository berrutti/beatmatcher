<template>
  <Modal
    :open="open"
    title="Set track BPM"
    confirm-label="Set BPM"
    @confirm="submit"
    @cancel="emit('cancel')"
  >
    <input
      ref="inputEl"
      class="bpm-input"
      type="number"
      min="60"
      max="200"
      step="0.1"
      placeholder="e.g. 128"
      @keydown.enter="submit"
      @keydown.escape="emit('cancel')"
    />
  </Modal>
</template>

<script setup lang="ts">
import { ref, watch, nextTick } from 'vue';
import Modal from './Modal.vue';

const props = defineProps<{ open: boolean; currentBpm: number | null }>();
const emit = defineEmits<{ submit: [bpm: number]; cancel: [] }>();

const inputEl = ref<HTMLInputElement | null>(null);

watch(
  () => props.open,
  async (isOpen) => {
    if (isOpen) {
      await nextTick();
      if (inputEl.value) {
        inputEl.value.value = props.currentBpm ? props.currentBpm.toFixed(1) : '';
        inputEl.value.select();
      }
    }
  }
);

function submit() {
  const val = parseFloat(inputEl.value?.value ?? '');
  if (isNaN(val) || val <= 0) return;
  emit('submit', val);
}
</script>

<style scoped>
.bpm-input {
  background: var(--color-bg);
  border: 1px solid #444;
  border-radius: 4px;
  color: var(--color-text);
  font-family: var(--font);
  font-size: 1.2rem;
  padding: 10px 12px;
  text-align: center;
  outline: none;
}

.bpm-input:focus {
  border-color: #888;
}

.bpm-input::placeholder {
  color: var(--color-border);
}
</style>
