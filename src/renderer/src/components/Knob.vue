<template>
  <div
    class="knob"
    :class="{ 'knob--dragging': dragging }"
    :style="{ '--knob-accent': accent, '--knob-size': `${size}px` }"
    role="slider"
    :aria-valuemin="spec.min"
    :aria-valuemax="spec.max"
    :aria-valuenow="value"
    v-tooltip="tooltip"
    @pointerdown="onPointerDown"
    @dblclick="onDoubleClick"
    @wheel.prevent="onWheel"
  >
    <svg class="knob__arcs" viewBox="0 0 40 40" aria-hidden="true">
      <path class="knob__track" :d="arcPath(0, 1)" />
      <path class="knob__value" :d="arcPath(knobOrigin(spec), travel)" />
    </svg>
    <div class="knob__cap" :style="{ transform: `rotate(${capDegrees}deg)` }">
      <span class="knob__pointer" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onUnmounted, ref } from 'vue';
import type { MixerParamSpec } from '@renderer/utils/sessionCore';
import { knobOrigin, knobTravel, valueAtTravel } from '@renderer/utils/knob';
import { CLICK_SLOP_PX } from '@renderer/utils/sliderThumb';

const {
  spec,
  value,
  accent = 'var(--color-text)',
  size = 34,
  clickResets = false,
  tooltip = ''
} = defineProps<{
  spec: MixerParamSpec;
  value: number;
  accent?: string;
  size?: number;
  clickResets?: boolean;
  // A prop rather than `v-tooltip` at the call site: a directive on a component re-renders it
  // on every parent render.
  tooltip?: string;
}>();

const emit = defineEmits<{ input: [value: number]; reset: [] }>();

const SWEEP_DEGREES = 270;
const START_DEGREES = 135;
const ARC_RADIUS = 17;
const DRAG_PX_FOR_FULL_TRAVEL = 200;
const FINE_DRAG_SCALE = 0.2;
const WHEEL_TRAVEL = 0.02;

const travel = computed(() => knobTravel(spec, value));
const capDegrees = computed(() => SWEEP_DEGREES * travel.value - SWEEP_DEGREES / 2);

function pointAt(position: number): string {
  const radians = ((START_DEGREES + SWEEP_DEGREES * position) * Math.PI) / 180;
  const x = 20 + ARC_RADIUS * Math.cos(radians);
  const y = 20 + ARC_RADIUS * Math.sin(radians);
  return `${x.toFixed(3)} ${y.toFixed(3)}`;
}

function arcPath(from: number, to: number): string {
  if (Math.abs(to - from) < 1e-6) return '';
  const start = Math.min(from, to);
  const end = Math.max(from, to);
  const largeArc = (end - start) * SWEEP_DEGREES > 180 ? 1 : 0;
  return `M ${pointAt(start)} A ${ARC_RADIUS} ${ARC_RADIUS} 0 ${largeArc} 1 ${pointAt(end)}`;
}

const dragging = ref(false);
let press: {
  x: number;
  y: number;
  lastY: number;
  travel: number;
  lastValue: number;
} | null = null;

function emitAt(position: number) {
  if (!press) return;
  press.travel = Math.max(0, Math.min(1, position));
  const next = valueAtTravel(spec, press.travel);
  if (next === press.lastValue) return;
  press.lastValue = next;
  emit('input', next);
}

function onPointerMove(event: PointerEvent) {
  if (!press) return;
  if (!dragging.value) {
    const moved =
      Math.abs(event.clientX - press.x) > CLICK_SLOP_PX ||
      Math.abs(event.clientY - press.y) > CLICK_SLOP_PX;
    if (!moved) return;
    dragging.value = true;
    document.body.classList.add('slider-dragging');
  }
  const scale = event.shiftKey ? FINE_DRAG_SCALE : 1;
  const delta = ((press.lastY - event.clientY) / DRAG_PX_FOR_FULL_TRAVEL) * scale;
  press.lastY = event.clientY;
  emitAt(press.travel + delta);
}

function endPress() {
  window.removeEventListener('pointermove', onPointerMove);
  window.removeEventListener('pointerup', onPointerUp);
  window.removeEventListener('pointercancel', onPointerUp);
  document.body.classList.remove('slider-dragging');
  press = null;
}

function onPointerUp() {
  const wasClick = press !== null && !dragging.value;
  dragging.value = false;
  endPress();
  if (wasClick && clickResets) emit('reset');
}

function onPointerDown(event: PointerEvent) {
  if (event.button !== 0) return;
  endPress();
  press = {
    x: event.clientX,
    y: event.clientY,
    lastY: event.clientY,
    travel: travel.value,
    lastValue: value
  };
  window.addEventListener('pointermove', onPointerMove);
  window.addEventListener('pointerup', onPointerUp);
  window.addEventListener('pointercancel', onPointerUp);
}

// The first click of the pair already reset it when clicks reset.
function onDoubleClick() {
  if (!clickResets) emit('reset');
}

function onWheel(event: WheelEvent) {
  if (event.deltaY === 0) return;
  const next = valueAtTravel(spec, travel.value - Math.sign(event.deltaY) * WHEEL_TRAVEL);
  if (next !== value) emit('input', next);
}

onUnmounted(endPress);
</script>

<style scoped>
.knob {
  position: relative;
  width: var(--knob-size);
  height: var(--knob-size);
  flex: none;
  cursor: grab;
  touch-action: none;
}

.knob--dragging {
  cursor: grabbing;
}

.knob__arcs {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  overflow: visible;
}

.knob__track,
.knob__value {
  fill: none;
  stroke-width: 2.5;
  stroke-linecap: round;
}

.knob__track {
  stroke: #262626;
}

.knob__value {
  stroke: var(--knob-accent);
}

/* The same brushed grey as the slider thumbs, so a knob and a fader read as one set. */
.knob__cap {
  position: absolute;
  inset: 22%;
  border-radius: 50%;
  background: radial-gradient(circle at 35% 30%, #808080, #4a4a4a 60%, #2e2e2e);
  border: 1px solid #5a5a5a;
  box-shadow:
    0 2px 5px rgba(0, 0, 0, 0.7),
    inset 0 1px 0 rgba(255, 255, 255, 0.12);
}

.knob__pointer {
  position: absolute;
  left: 50%;
  top: 8%;
  width: 2px;
  height: 36%;
  margin-left: -1px;
  border-radius: 1px;
  background: #e8e8e8;
}
</style>
