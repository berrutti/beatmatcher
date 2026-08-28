<template>
  <div class="deck__bpm-header">
    <div
      class="deck__bpm-value-wrap"
      :class="{ 'deck__bpm-value-wrap--empty': !editable }"
      @click="onBpmValueClick"
    >
      <input
        v-if="editingBpm"
        ref="bpmInputEl"
        v-model="bpmInputValue"
        class="deck__bpm-input-header"
        type="number"
        min="20"
        step="0.01"
        @blur="onBpmInputBlur"
        @keydown.enter="onBpmInputBlur"
        @keydown.escape="editingBpm = false"
      />
      <span
        class="deck__bpm-value-header"
        :class="{ 'deck__bpm-value-header--empty': !props.deck.trackLoaded }"
        :style="{ visibility: editingBpm ? 'hidden' : 'visible' }"
        >{{ displayValue }}</span
      >
    </div>
    <span class="deck__bpm-unit-header">{{ showsPitch ? '%' : $t('deck.bpm') }}</span>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, nextTick } from 'vue';
import type { Deck } from '@renderer/stores/decks';

const props = defineProps<{
  deck: Deck;
}>();

const editingBpm = ref(false);
const bpmInputEl = ref<HTMLInputElement | null>(null);
const bpmInputValue = ref('');

const showsPitch = computed(() => props.deck.trackLoaded && !props.deck.hasGrid);
const editable = computed(() => props.deck.trackLoaded && props.deck.hasGrid);

const NO_BPM = '--.--';

const displayValue = computed(() => {
  if (!props.deck.trackLoaded) return NO_BPM;
  if (showsPitch.value) {
    const pct = props.deck.pitchOffset;
    return `${pct >= 0 ? '+' : ''}${pct.toFixed(1)}`;
  }
  return props.deck.targetBpm?.toFixed(2) ?? NO_BPM;
});

async function startEditingBpm() {
  bpmInputValue.value = props.deck.targetBpm?.toFixed(2) ?? '';
  editingBpm.value = true;
  await nextTick();
  bpmInputEl.value?.select();
}

function onBpmValueClick() {
  if (!editable.value) return;
  startEditingBpm();
}

function onBpmInputBlur() {
  const val = parseFloat(bpmInputValue.value);
  if (!isNaN(val) && val > 0) props.deck.setTargetBpm(val);
  editingBpm.value = false;
}
</script>

<style scoped>
.deck__bpm-header {
  display: flex;
  align-items: center;
  gap: 0.25em;
  flex-shrink: 0;
}

.deck__bpm-value-wrap {
  position: relative;
  cursor: text;
}

.deck__bpm-value-wrap--empty {
  cursor: default;
}

.deck__bpm-value-header {
  font-size: 0.8em;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  color: var(--deck-accent);
  letter-spacing: -0.01em;
  display: block;
}

.deck__bpm-value-header--empty {
  color: var(--color-muted);
  opacity: 0.6;
}

.deck__bpm-input-header {
  position: absolute;
  inset: 0;
  font-size: 0.8em;
  font-weight: 700;
  font-family: var(--font);
  font-variant-numeric: tabular-nums;
  background: transparent;
  border: none;
  box-shadow: 0 1px 0 0 var(--deck-accent);
  color: var(--deck-accent);
  width: 100%;
  padding: 0;
  outline: none;
  line-height: inherit;
  appearance: textfield;
}
.deck__bpm-input-header::-webkit-inner-spin-button,
.deck__bpm-input-header::-webkit-outer-spin-button {
  display: none;
}
.deck__bpm-input-header::selection {
  background: var(--deck-accent);
  color: var(--color-bg);
}

.deck__bpm-unit-header {
  font-size: 0.6em;
  color: var(--color-muted);
  letter-spacing: 0.04em;
}
</style>
