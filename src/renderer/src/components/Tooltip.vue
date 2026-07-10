<template>
  <Teleport to="body">
    <div
      v-if="state.visible && state.targetRect"
      ref="tooltipEl"
      class="tooltip"
      :class="{ 'tooltip--below': below }"
      :style="{ left: `${x}px`, top: `${y}px` }"
    >
      {{ state.text }}
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { ref, nextTick, watch } from 'vue';
import { useTooltip } from '@renderer/composables/useTooltip';

const GAP_PX = 6;

const { state } = useTooltip();
const tooltipEl = ref<HTMLElement | null>(null);
const below = ref(false);
const x = ref(0);
const y = ref(0);

watch(
  () => state.visible,
  async (visible) => {
    if (!visible || !state.targetRect) return;
    const rect = state.targetRect;
    x.value = rect.left + rect.width / 2;
    below.value = false;
    y.value = rect.top - GAP_PX;

    await nextTick();
    const el = tooltipEl.value;
    if (!el) return;

    const tooltipRect = el.getBoundingClientRect();
    if (rect.top - tooltipRect.height - GAP_PX < 0) {
      below.value = true;
      y.value = rect.bottom + GAP_PX;
    }

    const halfWidth = tooltipRect.width / 2;
    x.value = Math.min(
      Math.max(x.value, halfWidth + GAP_PX),
      window.innerWidth - halfWidth - GAP_PX
    );
  }
);
</script>

<style scoped>
.tooltip {
  position: fixed;
  transform: translate(-50%, -100%);
  background: var(--color-surface);
  color: var(--color-text);
  border: 1px solid var(--color-border);
  border-radius: var(--radius);
  padding: 0.35em 0.6em;
  font-family: var(--font);
  font-size: 11px;
  line-height: 1.3;
  white-space: nowrap;
  pointer-events: none;
  z-index: 10000;
}

.tooltip--below {
  transform: translate(-50%, 0);
}
</style>
