<template>
  <div class="dropdown" ref="rootEl">
    <button class="dropdown__trigger" @click="open = !open">
      {{ label }} <span class="dropdown__chevron">▾</span>
    </button>
    <div v-if="open" class="dropdown__menu">
      <button
        v-for="item in items"
        :key="item.value"
        class="dropdown__item"
        :class="{ 'dropdown__item--active': item.value === modelValue }"
        @click="onSelect(item.value)"
      >
        {{ item.label }}
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';

defineProps<{
  label: string;
  modelValue?: string;
  items: { value: string; label: string }[];
}>();

const emit = defineEmits<{ 'update:modelValue': [value: string]; select: [value: string] }>();

const open = ref(false);
const rootEl = ref<HTMLElement | null>(null);

function onSelect(value: string) {
  open.value = false;
  emit('update:modelValue', value);
  emit('select', value);
}

function onDocumentClick(e: MouseEvent) {
  if (rootEl.value && !rootEl.value.contains(e.target as Node)) {
    open.value = false;
  }
}

onMounted(() => document.addEventListener('click', onDocumentClick, true));
onUnmounted(() => document.removeEventListener('click', onDocumentClick, true));
</script>

<style scoped>
.dropdown {
  position: relative;
}

.dropdown__trigger {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-text);
  font-family: var(--font);
  font-size: 10px;
  letter-spacing: 0.12em;
  height: 22px;
  padding: 0 8px;
  border-radius: 3px;
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 4px;
  white-space: nowrap;
}

.dropdown__trigger:hover {
  border-color: #06b6d4;
  color: #06b6d4;
}

.dropdown__chevron {
  font-size: 8px;
  opacity: 0.7;
}

.dropdown__menu {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 4px;
  min-width: 130px;
  z-index: 200;
  overflow: hidden;
}

.dropdown__item {
  display: block;
  width: 100%;
  background: transparent;
  border: none;
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 10px;
  letter-spacing: 0.12em;
  padding: 7px 12px;
  text-align: left;
  cursor: pointer;
}

.dropdown__item:hover {
  background: var(--color-bg);
  color: var(--color-text);
}

.dropdown__item--active {
  color: #06b6d4;
}
</style>
