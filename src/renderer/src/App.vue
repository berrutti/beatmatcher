<template>
  <div class="app">
    <AppBar />
    <TopStrip />
    <SettingsModal v-if="settingsStore.isOpen" />
    <EditView v-if="appMode.mode === 'edit'" class="app__view" :deck="decksStore.deckE" />
    <Performance v-else-if="appMode.mode === 'performance'" class="app__view" />
    <Session v-else-if="appMode.mode === 'session'" class="app__view" />
  </div>
</template>

<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue';
import { useDecksStore } from '@renderer/stores/decks';
import { useSettingsStore } from '@renderer/stores/settings';
import { useAppModeStore } from '@renderer/stores/appMode';
import { useKeyboard } from '@renderer/composables/useKeyboard';
import AppBar from '@renderer/components/AppBar.vue';
import TopStrip from '@renderer/components/TopStrip.vue';
import EditView from '@renderer/components/deck/EditView.vue';
import Performance from '@renderer/components/performance/Performance.vue';
import Session from '@renderer/components/session/Session.vue';
import SettingsModal from '@renderer/components/Settings.vue';

const decksStore = useDecksStore();
const settingsStore = useSettingsStore();
const appMode = useAppModeStore();

useKeyboard();

onMounted(() => settingsStore.init());
onUnmounted(() => decksStore.destroy());
</script>

<style scoped>
.app {
  width: 100dvw;
  height: 100dvh;
  display: flex;
  flex-direction: column;
  --appbar-h: 28px;
  --topstrip-h: 28px;
}

.app__view {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}
</style>
