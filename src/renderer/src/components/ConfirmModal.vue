<template>
  <Transition name="bpm-modal">
    <div v-if="open" class="confirm-modal__backdrop" @click.self="emit('cancel')">
      <div class="confirm-modal">
        <div class="confirm-modal__title">{{ title }}</div>
        <div class="confirm-modal__body">{{ body }}</div>
        <div class="confirm-modal__actions">
          <button class="confirm-modal__btn confirm-modal__btn--cancel" @click="emit('cancel')">
            Cancel
          </button>
          <button class="confirm-modal__btn confirm-modal__btn--confirm" @click="emit('confirm')">
            Load
          </button>
        </div>
      </div>
    </div>
  </Transition>
</template>

<script setup lang="ts">
defineProps<{ open: boolean; title: string; body: string }>();
const emit = defineEmits<{ confirm: []; cancel: [] }>();
</script>

<style scoped>
.confirm-modal__backdrop {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.7);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
}

.confirm-modal {
  background: #1a1a1a;
  border: 1px solid #333;
  border-radius: 8px;
  padding: 24px;
  min-width: 280px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.confirm-modal__title {
  font-size: 0.85rem;
  font-weight: 700;
  letter-spacing: 0.1em;
  color: #ccc;
}

.confirm-modal__body {
  font-size: 0.75rem;
  color: #777;
  line-height: 1.5;
}

.confirm-modal__actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
}

.confirm-modal__btn {
  font-family: var(--font-mono);
  font-size: 0.7rem;
  letter-spacing: 0.1em;
  padding: 8px 16px;
  border-radius: 4px;
  cursor: pointer;
  border: 1px solid #333;
}

.confirm-modal__btn--cancel {
  background: transparent;
  color: #777;
}

.confirm-modal__btn--cancel:hover {
  color: #aaa;
  border-color: #555;
}

.confirm-modal__btn--confirm {
  background: #2a2a2a;
  color: #eee;
}

.confirm-modal__btn--confirm:hover {
  background: #333;
  border-color: #555;
}
</style>
