<template>
  <Transition name="bpm-modal">
    <div v-if="open" class="bpm-modal__backdrop" @click.self="emit('cancel')">
      <div class="bpm-modal">
        <div class="bpm-modal__title">Set track BPM</div>
        <input
          ref="inputEl"
          class="bpm-modal__input"
          type="number"
          min="60"
          max="200"
          step="0.1"
          placeholder="e.g. 128"
          @keydown.enter="submit"
          @keydown.escape="emit('cancel')"
        />
        <div class="bpm-modal__actions">
          <button class="bpm-modal__btn bpm-modal__btn--cancel" @click="emit('cancel')">Cancel</button>
          <button class="bpm-modal__btn bpm-modal__btn--submit" @click="submit">Set BPM</button>
        </div>
      </div>
    </div>
  </Transition>
</template>

<script setup lang="ts">
import { ref, watch, nextTick } from 'vue'

const props = defineProps<{ open: boolean; currentBpm: number | null }>()
const emit = defineEmits<{ submit: [bpm: number]; cancel: [] }>()

const inputEl = ref<HTMLInputElement | null>(null)

watch(() => props.open, async (isOpen) => {
  if (isOpen) {
    await nextTick()
    if (inputEl.value) {
      inputEl.value.value = props.currentBpm ? props.currentBpm.toFixed(1) : ''
      inputEl.value.select()
    }
  }
})

function submit() {
  const val = parseFloat(inputEl.value?.value ?? '')
  if (isNaN(val) || val <= 0) return
  emit('submit', val)
}
</script>

<style scoped>
.bpm-modal__backdrop {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.7);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
}

.bpm-modal {
  background: #1a1a1a;
  border: 1px solid #333;
  border-radius: 8px;
  padding: 24px;
  min-width: 280px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.bpm-modal__title {
  font-size: 0.85rem;
  font-weight: 700;
  letter-spacing: 0.1em;
  color: #ccc;
}

.bpm-modal__input {
  background: #0d0d0d;
  border: 1px solid #444;
  border-radius: 4px;
  color: #fff;
  font-family: var(--font-mono);
  font-size: 1.2rem;
  padding: 10px 12px;
  text-align: center;
  outline: none;
}

.bpm-modal__input:focus {
  border-color: #888;
}

.bpm-modal__input::placeholder {
  color: #444;
}

.bpm-modal__actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
}

.bpm-modal__btn {
  font-family: var(--font-mono);
  font-size: 0.7rem;
  letter-spacing: 0.1em;
  padding: 8px 16px;
  border-radius: 4px;
  cursor: pointer;
  border: 1px solid #333;
}

.bpm-modal__btn--cancel {
  background: transparent;
  color: #777;
}

.bpm-modal__btn--cancel:hover {
  color: #aaa;
  border-color: #555;
}

.bpm-modal__btn--submit {
  background: #2a2a2a;
  color: #eee;
}

.bpm-modal__btn--submit:hover {
  background: #333;
  border-color: #555;
}

.bpm-modal-enter-active,
.bpm-modal-leave-active {
  transition: opacity 0.15s ease;
}

.bpm-modal-enter-from,
.bpm-modal-leave-to {
  opacity: 0;
}
</style>
