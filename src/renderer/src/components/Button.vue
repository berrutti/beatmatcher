<template>
  <button ref="el" class="btn" :class="`btn--${variant}`" :disabled="disabled" type="button">
    <slot />
  </button>
</template>

<script setup lang="ts">
import { ref } from 'vue';

const { variant = 'secondary', disabled = false } = defineProps<{
  variant?: 'primary' | 'secondary' | 'danger';
  disabled?: boolean;
}>();

const el = ref<HTMLButtonElement | null>(null);

// Exposed so a dialog can put the caret on its default action without reaching for
// the element behind the component.
defineExpose({ focus: () => el.value?.focus() });
</script>

<style scoped>
.btn {
  font-family: var(--font);
  font-size: 0.7rem;
  letter-spacing: 0.04em;
  padding: 8px 16px;
  border-radius: 4px;
  cursor: pointer;
  border: 1px solid var(--color-border);
  background: transparent;
  color: var(--color-muted);
}

.btn:disabled {
  cursor: default;
  opacity: 0.5;
}

/* Only while the user is actually tabbing, in the button's own colour. See
   `utils/keyboardNav.ts` for why this is not `:focus-visible`. */
:root[data-keyboard-nav] .btn:focus {
  outline: 2px solid var(--btn-focus);
  outline-offset: 2px;
}

.btn--secondary {
  --btn-focus: var(--color-text);
}

.btn--secondary:hover:not(:disabled) {
  color: var(--color-text);
  border-color: #555;
}

.btn--primary {
  --btn-focus: var(--color-text);
  background: #2a2a2a;
  color: var(--color-text);
}

.btn--primary:hover:not(:disabled) {
  background: #333;
  border-color: #555;
}

.btn--danger {
  --btn-focus: #e0685f;
  color: #e0685f;
  border-color: #5d2f2b;
}

.btn--danger:hover:not(:disabled) {
  color: #ff8078;
  border-color: #8a3f39;
  background: #2a1a19;
}
</style>
