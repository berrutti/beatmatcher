<template>
  <Knob
    :spec="spec"
    :value="mixer.paramValue(deckId, key())"
    :accent="decks.decks[deckId].accent"
    :size="size"
    :click-resets="settings.sliderClickResets"
    :tooltip="tooltip"
    @input="(value) => mixer.swarmAdjust(deckId, key(), value)"
    @reset="mixer.swarmReset(deckId, key(), spec.defaultValue)"
  />
</template>

<script setup lang="ts">
import Knob from '@renderer/components/Knob.vue';
import { useDecksStore } from '@renderer/stores/decks';
import { useMixerStore, paramKey } from '@renderer/stores/mixer';
import { useSettingsStore } from '@renderer/stores/settings';
import type { DeckId } from '@renderer/utils/types';
import type { MixerParamSpec } from '@renderer/utils/sessionCore';

const {
  deckId,
  spec,
  size = 30,
  tooltip = ''
} = defineProps<{
  deckId: DeckId;
  spec: MixerParamSpec;
  size?: number;
  tooltip?: string;
}>();

const mixer = useMixerStore();
const decks = useDecksStore();
const settings = useSettingsStore();

function key(): string {
  return paramKey(spec.slot, spec.param);
}
</script>
