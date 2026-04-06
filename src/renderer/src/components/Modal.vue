<template>
  <Transition name="modal">
    <div v-if="open" class="modal__backdrop" @click.self="emit('cancel')">
      <div class="modal">
        <div class="modal__title">{{ title }}</div>
        <slot />
        <div class="modal__actions">
          <button class="modal__btn modal__btn--cancel" @click="emit('cancel')">Cancel</button>
          <button class="modal__btn modal__btn--confirm" @click="emit('confirm')">
            {{ confirmLabel }}
          </button>
        </div>
      </div>
    </div>
  </Transition>
</template>

<script setup lang="ts">
withDefaults(defineProps<{ open: boolean; title: string; confirmLabel?: string }>(), {
  confirmLabel: 'Confirm'
});
const emit = defineEmits<{ confirm: []; cancel: [] }>();
</script>

<style scoped>
.modal__backdrop {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.7);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
}

.modal {
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 8px;
  padding: 24px;
  min-width: 280px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.modal__title {
  font-size: 0.85rem;
  font-weight: 700;
  letter-spacing: 0.1em;
  color: var(--color-text);
}

.modal__actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
}

.modal__btn {
  font-family: var(--font);
  font-size: 0.7rem;
  letter-spacing: 0.1em;
  padding: 8px 16px;
  border-radius: 4px;
  cursor: pointer;
  border: 1px solid var(--color-border);
}

.modal__btn--cancel {
  background: transparent;
  color: var(--color-muted);
}

.modal__btn--cancel:hover {
  color: var(--color-text);
  border-color: #555;
}

.modal__btn--confirm {
  background: #2a2a2a;
  color: var(--color-text);
}

.modal__btn--confirm:hover {
  background: #333;
  border-color: #555;
}

.modal-enter-active,
.modal-leave-active {
  transition: opacity 0.15s ease;
}

.modal-enter-from,
.modal-leave-to {
  opacity: 0;
}
</style>
