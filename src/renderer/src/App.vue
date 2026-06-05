<template>
  <Session v-if="sessionMode" @exit="onExitSession" />
  <Performance v-else @enter-session="onEnterSession" />
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

const sessionMode = ref(false);

onMounted(() => settingsStore.init());
onUnmounted(() => decksStore.destroy());

async function onEnterSession() {
  await sessionStore.ejectAllDecks();
  sessionMode.value = true;
}

async function onExitSession() {
  await sessionStore.exit();
  sessionMode.value = false;
}
</script>
