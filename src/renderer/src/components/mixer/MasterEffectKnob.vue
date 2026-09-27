<template>
  <Knob
    :spec="spec"
    :value="mixer.masterEffectValue(spec)"
    :click-resets="settings.sliderClickResets"
    :tooltip="tooltip"
    @input="(value) => mixer.setMasterEffect(spec, value)"
    @reset="mixer.setMasterEffect(spec, spec.defaultValue)"
  />
</template>

<script setup lang="ts">
import Knob from '@renderer/components/Knob.vue';
import { useMixerStore } from '@renderer/stores/mixer';
import { useSettingsStore } from '@renderer/stores/settings';
import type { MixerParamSpec } from '@renderer/utils/sessionCore';

const { spec, tooltip = '' } = defineProps<{ spec: MixerParamSpec; tooltip?: string }>();

const mixer = useMixerStore();
const settings = useSettingsStore();
</script>
