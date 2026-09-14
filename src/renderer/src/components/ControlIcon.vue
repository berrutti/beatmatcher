<template>
  <svg class="control-icon" viewBox="0 0 24 24" aria-hidden="true">
    <template v-if="shape === 'play'">
      <polygon class="control-icon__fill" points="8,5 19,12 8,19" />
    </template>

    <!-- The marker a deck returns to, drawn as the playhead flag the waveform shows. -->
    <template v-else-if="shape === 'cue'">
      <line x1="7" y1="4" x2="7" y2="20" />
      <polygon class="control-icon__fill" points="7,4 17,8 7,12" />
    </template>

    <template v-else-if="shape === 'headphones'">
      <path d="M4.5 15v-2a7.5 7.5 0 0 1 15 0v2" />
      <rect class="control-icon__fill" x="3" y="14" width="3.5" height="6" rx="1.5" />
      <rect class="control-icon__fill" x="17.5" y="14" width="3.5" height="6" rx="1.5" />
    </template>

    <template v-else-if="shape === 'loopIn'">
      <polyline points="10,5 5,5 5,19 10,19" />
      <line class="control-icon__fill" x1="14" y1="7" x2="14" y2="17" stroke-width="3" />
    </template>

    <template v-else-if="shape === 'loopOut'">
      <polyline points="14,5 19,5 19,19 14,19" />
      <line class="control-icon__fill" x1="10" y1="7" x2="10" y2="17" stroke-width="3" />
    </template>

    <template v-else-if="shape === 'reloop'">
      <path d="M19 9a7.5 7.5 0 1 0 .8 6" />
      <polyline points="19,4 19,9.5 13.5,9.5" />
    </template>

    <!-- Snapping to a grid, which is what quantize does to a press. -->
    <template v-else-if="shape === 'quantize'">
      <line x1="4" y1="5" x2="4" y2="19" />
      <line x1="12" y1="5" x2="12" y2="19" />
      <line x1="20" y1="5" x2="20" y2="19" />
      <polyline points="8.5,9.5 12,6 15.5,9.5" />
    </template>

    <template v-else-if="shape === 'shift'">
      <polyline points="5,12 12,5 19,12" />
      <line x1="12" y1="5" x2="12" y2="19" />
    </template>

    <template v-else-if="shape === 'load'">
      <polyline points="8,10 12,14 16,10" />
      <line x1="12" y1="3.5" x2="12" y2="14" />
      <polyline points="4.5,17 4.5,20.5 19.5,20.5 19.5,17" />
    </template>

    <template v-else-if="shape === 'eject'">
      <polygon class="control-icon__fill" points="12,4 20,14 4,14" />
      <line x1="4" y1="19" x2="20" y2="19" stroke-width="2.4" />
    </template>

    <template v-else-if="shape === 'enter'">
      <polyline points="9,8 4.5,12.5 9,17" />
      <path d="M4.5 12.5h11a4 4 0 0 0 4-4V5" />
    </template>

    <template v-else-if="shape === 'back'">
      <polyline points="10,6 4,12 10,18" />
      <path d="M4 12h9a6 6 0 0 1 0 12" />
    </template>

    <template v-else-if="shape === 'view'">
      <rect x="3.5" y="5" width="17" height="14" rx="2" />
      <line x1="11" y1="5" x2="11" y2="19" />
    </template>

    <template v-else-if="shape === 'button'">
      <rect x="4" y="4" width="16" height="16" rx="3" />
      <rect class="control-icon__fill" x="7.5" y="7.5" width="9" height="9" rx="1.5" />
    </template>

    <template v-else-if="shape === 'knob'">
      <circle cx="12" cy="12" r="7.5" />
      <line x1="12" y1="12" x2="16" y2="7.4" />
      <line class="control-icon__tick" x1="4.6" y1="18.4" x2="6.2" y2="16.8" />
      <line class="control-icon__tick" x1="19.4" y1="18.4" x2="17.8" y2="16.8" />
    </template>

    <!-- The cap sits off centre: centred on a full-length track it reads as a plus. -->
    <template v-else-if="shape === 'faderVertical'">
      <line x1="12" y1="3.5" x2="12" y2="20.5" />
      <rect class="control-icon__fill" x="6" y="7" width="12" height="3.5" rx="1.2" />
    </template>

    <template v-else-if="shape === 'faderHorizontal'">
      <line x1="3.5" y1="12" x2="20.5" y2="12" />
      <rect class="control-icon__fill" x="13.5" y="6" width="3.5" height="12" rx="1.2" />
    </template>

    <template v-else-if="shape === 'wheel'">
      <circle cx="12" cy="12" r="8.5" />
      <circle class="control-icon__fill" cx="12" cy="12" r="3" />
      <line x1="12" y1="3.5" x2="12" y2="6.5" />
    </template>

    <template v-else-if="shape === 'encoder'">
      <circle cx="12" cy="12" r="7" />
      <line x1="12" y1="2.5" x2="12" y2="5" />
      <line x1="12" y1="19" x2="12" y2="21.5" />
      <line x1="2.5" y1="12" x2="5" y2="12" />
      <line x1="19" y1="12" x2="21.5" y2="12" />
    </template>

    <template v-else-if="shape === 'stepBack'">
      <rect x="4" y="4" width="16" height="16" rx="3" />
      <polyline points="14,8 9.5,12 14,16" />
    </template>

    <template v-else>
      <rect x="4" y="4" width="16" height="16" rx="3" />
      <polyline points="10,8 14.5,12 10,16" />
    </template>
  </svg>
</template>

<script setup lang="ts">
import type { ControlShape } from '@renderer/utils/midiControlShape';

defineProps<{ shape: ControlShape }>();
</script>

<style scoped>
.control-icon {
  width: 22px;
  height: 22px;
  flex: none;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.4;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.control-icon__fill {
  fill: currentColor;
  stroke: none;
}

.control-icon__tick {
  opacity: 0.45;
}
</style>
