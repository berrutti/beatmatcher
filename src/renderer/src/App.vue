<template>
  <Session v-if="mode" @exit="onExitSession" />
  <Performance v-else @exit="onExitPerformance" />
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { useDecksStore } from '@renderer/stores/decks';
import { useSettingsStore } from '@renderer/stores/settings';
import { useSessionStore } from '@renderer/stores/session';
import Session from '@renderer/components/session/Session.vue';
import Performance from '@renderer/components/performance/Performance.vue';

const decksStore = useDecksStore();
const settingsStore = useSettingsStore();
const sessionStore = useSessionStore();

const mode = ref<'performance' | 'session'>('performance');

onMounted(() => settingsStore.init());
onUnmounted(() => decksStore.destroy());

async function onExitPerformance() {
  await sessionStore.ejectAllDecks();
  mode.value= 'session'
}

async function onExitSession() {
  await sessionStore.exit();
  mode.value= 'performance'
}
</script>
