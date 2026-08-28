<template>
  <label class="checkbox" :class="{ 'checkbox--disabled': disabled }">
    <input type="checkbox" :checked="modelValue" :disabled="disabled" @change="onChange" />
    <span><slot /></span>
  </label>
</template>

<script setup lang="ts">
defineProps<{ modelValue: boolean; disabled?: boolean }>();

const emit = defineEmits<{ 'update:modelValue': [value: boolean] }>();

function onChange(event: Event) {
  const target = event.target;
  if (!(target instanceof HTMLInputElement)) return;
  emit('update:modelValue', target.checked);
}
</script>

<style scoped>
/* Font and colour are left to inherit, so a call site places it in its own
   context rather than fighting a default. */
.checkbox {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  user-select: none;
}

.checkbox--disabled {
  opacity: 0.4;
  cursor: default;
  pointer-events: none;
}

/* Gated like the buttons: see `utils/keyboardNav.ts`. */
:root[data-keyboard-nav] .checkbox input:focus {
  outline: 2px solid var(--color-text);
  outline-offset: 2px;
}
</style>
