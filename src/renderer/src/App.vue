<template>
  <Session v-if="mode === 'session'" @exit="onExitSession" />
  <Performance v-else @exit="onExitPerformance" />
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { useDecksStore } from '@renderer/stores/decks';
import { useMixerStore } from '@renderer/stores/mixer';
import { useSettingsStore } from '@renderer/stores/settings';
import { useSessionStore } from '@renderer/stores/session';
import Session from '@renderer/components/session/Session.vue';
import Performance from '@renderer/components/performance/Performance.vue';

const decksStore = useDecksStore();
const mixerStore = useMixerStore();
const settingsStore = useSettingsStore();
const sessionStore = useSessionStore();

const mode = ref<'performance' | 'session'>('performance');

onMounted(() => settingsStore.init());
onUnmounted(() => decksStore.destroy());

async function onExitPerformance() {
  await decksStore.ejectAll();
  mode.value = 'session';
}

async function onExitSession() {
  await sessionStore.exit();
  mixerStore.reset();
  mode.value = 'performance';
}
</script>
