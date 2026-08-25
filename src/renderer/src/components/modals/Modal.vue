<template>
  <Transition name="modal">
    <div v-if="open" class="modal__backdrop" @click.self="dismiss">
      <div ref="panel" class="modal" data-modal-panel tabindex="-1">
        <div class="modal__title">{{ title }}</div>
        <p v-if="body" class="modal__body">{{ body }}</p>
        <slot />
        <div v-if="dismissable" class="modal__actions">
          <Button class="modal__btn modal__btn--cancel" @click="emit('cancel')">
            {{ $t('modal.cancel') }}
          </Button>
          <Button
            ref="confirmBtn"
            variant="primary"
            class="modal__btn modal__btn--confirm"
            @click="emit('confirm')"
          >
            {{ confirmLabel ?? $t('modal.confirm') }}
          </Button>
        </div>
      </div>
    </div>
  </Transition>
</template>

<script setup lang="ts">
import { ref, watch, nextTick, onUnmounted } from 'vue';
import { markModalClosed, markModalOpen } from '@renderer/utils/modalStack';
import Button from '@renderer/components/Button.vue';
import { trapTabWithin } from '@renderer/utils/focusTrap';

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
  autoFocusEl?: { focus: () => void } | null;
  // Off for a gate the user must wait out: the buttons are exits too, so they
  // go with the backdrop click and the escape key rather than separately.
  dismissable?: boolean;
}>();
const emit = defineEmits<{ confirm: []; cancel: [] }>();

function dismiss(): void {
  if (dismissable) emit('cancel');
}

// Paired through a flag rather than the prop, so a close and an unmount cannot both
// release the same modal and drive the count negative.
let counted = false;
watch(
  () => open,
  (val) => {
    if (val === counted) return;
    counted = val;
    if (val) markModalOpen();
    else markModalClosed();
  },
  { immediate: true }
);
// On the document rather than the panel: clicking the backdrop puts focus on the body,
// whose ancestors do not include the panel, so a listener there stops seeing Tab at all.
// Capture, so it runs before anything the page has bound.
// Every open modal listens on the document, so without this the one underneath a
// blocking dialog would answer an Escape aimed at the dialog on top of it.
function isTopmost(): boolean {
  const panels = document.querySelectorAll('[data-modal-panel]');
  return panels.length === 0 || panels[panels.length - 1] === panel.value;
}

function onDocumentKeydown(nativeEvent: KeyboardEvent): void {
  if (!isTopmost()) return;
  if (nativeEvent.key === 'Tab') {
    trapTabWithin(nativeEvent, panel.value);
    return;
  }
  if (nativeEvent.key === 'Escape') dismiss();
}

watch(
  () => open,
  (val) => {
    if (val) document.addEventListener('keydown', onDocumentKeydown, true);
    else document.removeEventListener('keydown', onDocumentKeydown, true);
  },
  { immediate: true }
);

onUnmounted(() => {
  document.removeEventListener('keydown', onDocumentKeydown, true);
  if (counted) {
    counted = false;
    markModalClosed();
  }
});

const confirmBtn = ref<{ focus: () => void } | null>(null);
const panel = ref<HTMLElement | null>(null);
// Immediate, because a modal rendered behind `v-if` mounts with `open` already true and
// would otherwise open with nothing focused at all.
watch(
  () => open,
  (val) => {
    if (val) nextTick(() => (autoFocusEl ?? confirmBtn.value ?? panel.value)?.focus());
  },
  { immediate: true }
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

.modal-enter-active,
.modal-leave-active {
  transition: opacity 0.16s ease;
}

.modal-enter-active .modal,
.modal-leave-active .modal {
  transition:
    opacity 0.16s ease,
    transform 0.16s cubic-bezier(0.2, 0, 0.2, 1);
}

.modal-enter-from,
.modal-leave-to {
  opacity: 0;
}

.modal-enter-from .modal,
.modal-leave-to .modal {
  opacity: 0;
  transform: translateY(-8px) scale(0.97);
}

@media (prefers-reduced-motion: reduce) {
  .modal-enter-active,
  .modal-leave-active,
  .modal-enter-active .modal,
  .modal-leave-active .modal {
    transition-duration: 0.01ms;
  }

  .modal-enter-from .modal,
  .modal-leave-to .modal {
    transform: none;
  }
}
</style>
