<template>
  <div class="search-wrap" :class="{ 'search-wrap--full': fullWidth }">
    <input
      :value="modelValue"
      class="search__input"
      type="text"
      :placeholder="placeholder"
      spellcheck="false"
      @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
      @pointerdown="onPointerDown"
      @keydown.esc="emit('update:modelValue', '')"
    />
    <button
      v-if="modelValue"
      class="search__clear"
      tabindex="-1"
      @click="emit('update:modelValue', '')"
    >
      ✕
    </button>
  </div>
</template>

<script setup lang="ts">
import { useCollectionStore } from '@renderer/stores/collection';

withDefaults(
  defineProps<{
    modelValue: string;
    placeholder?: string;
    fullWidth?: boolean;
  }>(),
  {
    placeholder: 'Search',
    fullWidth: false
  }
);

const emit = defineEmits<{
  'update:modelValue': [value: string];
}>();

const store = useCollectionStore();

function onPointerDown(e: PointerEvent) {
  if (store.draggingPath) e.preventDefault();
}
</script>

<style scoped>
.search-wrap {
  position: relative;
  display: flex;
  align-items: center;
}

.search-wrap--full {
  padding: 6px 1em;
  border-bottom: 1px solid var(--color-border);
}

.search__input {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-text);
  font-family: var(--font);
  font-size: 0.8em;
  padding: 0.25em 1.6em 0.25em 0.5em;
  border-radius: 3px;
  outline: none;
  width: 8em;
}

.search-wrap--full .search__input {
  width: 100%;
}

.search__input::placeholder {
  color: var(--color-muted);
  opacity: 0.5;
}

.search__input:focus {
  border-color: #555;
}

.search__clear {
  position: absolute;
  right: 0.3em;
  background: transparent;
  border: none;
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.72em;
  cursor: pointer;
  padding: 0;
  line-height: 1;
  opacity: 0.6;
}

.search-wrap--full .search__clear {
  right: calc(1em + 0.3em);
}

.search__clear:hover {
  opacity: 1;
  color: var(--color-text);
}
</style>
