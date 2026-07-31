<template>
  <Transition name="modal">
    <div v-if="open" class="modal__backdrop" @click.self="dismiss" @keydown.escape="dismiss">
      <div class="modal">
        <div class="modal__title">{{ title }}</div>
        <p v-if="body" class="modal__body">{{ body }}</p>
        <slot />
        <div v-if="dismissable" class="modal__actions">
          <button class="modal__btn modal__btn--cancel" @click="emit('cancel')">
            {{ $t('modal.cancel') }}
          </button>
          <button ref="confirmBtn" class="modal__btn modal__btn--confirm" @click="emit('confirm')">
            {{ confirmLabel ?? $t('modal.confirm') }}
          </button>
        </div>
      </div>
    </div>
  </Transition>
</template>

<script setup lang="ts">
import { ref, watch, nextTick } from 'vue';

const {
  open,
  autoFocusEl = null,
  dismissable = true
} = defineProps<{
  open: boolean;
  title: string;
  // The explanatory line under the title. Here rather than in each caller's slot
  // because all of them want the same one paragraph of muted text.
  body?: string;
  confirmLabel?: string;
  // Element to focus when the modal opens, e.g. a form input inside the
  // slot, passed down as a template ref (Vue auto-unwraps it in the
  // template, so this receives the element itself). Falls back to the
  // confirm button when not given.
  autoFocusEl?: HTMLElement | null;
  // Off for a gate the user must wait out: the buttons are exits too, so they
  // go with the backdrop click and the escape key rather than separately.
  dismissable?: boolean;
}>();
const emit = defineEmits<{ confirm: []; cancel: [] }>();

function dismiss(): void {
  if (dismissable) emit('cancel');
}

const confirmBtn = ref<HTMLButtonElement | null>(null);
watch(
  () => open,
  (val) => {
    if (val) nextTick(() => (autoFocusEl ?? confirmBtn.value)?.focus());
  }
);
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
  letter-spacing: 0.04em;
  color: var(--color-text);
}

.modal__body {
  font-size: 0.75rem;
  line-height: 1.5;
  color: var(--color-muted);
  margin: 0;
}

.modal__actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
}

.modal__btn {
  font-family: var(--font);
  font-size: 0.7rem;
  letter-spacing: 0.04em;
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
