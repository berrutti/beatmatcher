<template>
  <div class="app">
    <AppBar />
    <TopStrip />
    <SettingsModal v-if="settingsStore.isOpen" />
    <ConfirmModal
      :open="quitModalOpen"
      :title="t('quitModal.title')"
      :body="quitModalBody"
      :confirm-label="t('quitModal.confirm')"
      @confirm="onQuitConfirmed"
      @cancel="onQuitCancelled"
    />
    <EditView v-if="appMode.mode === 'edit'" class="app__view" :deck="decksStore.deckE" />
    <Performance v-else-if="appMode.mode === 'performance'" class="app__view" />
    <Session v-else-if="appMode.mode === 'session'" class="app__view" />
    <Tooltip />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';
import { useDecksStore } from '@renderer/stores/decks';
import { useSettingsStore } from '@renderer/stores/settings';
import { useAppModeStore } from '@renderer/stores/appMode';
import { useMidiStore } from '@renderer/stores/midi';
import { useSessionStore } from '@renderer/stores/session';
import { useSessionEditStore } from '@renderer/stores/sessionEdit';
import { useKeyboard } from '@renderer/composables/useKeyboard';
import AppBar from '@renderer/components/AppBar.vue';
import TopStrip from '@renderer/components/TopStrip.vue';
import EditView from '@renderer/components/deck/EditView.vue';
import Performance from '@renderer/components/performance/Performance.vue';
import Session from '@renderer/components/session/Session.vue';
import SettingsModal from '@renderer/components/Settings.vue';
import ConfirmModal from '@renderer/components/modals/ConfirmModal.vue';
import Tooltip from '@renderer/components/Tooltip.vue';

const { t } = useI18n();
const decksStore = useDecksStore();
const settingsStore = useSettingsStore();
const appMode = useAppModeStore();
// Created here so a plugged-in controller reconnects at launch rather than on
// the first visit to Settings.
useMidiStore();
const sessionStore = useSessionStore();
const sessionEditStore = useSessionEditStore();

useKeyboard();

const quitModalOpen = ref(false);

function isPlayingNow(): boolean {
  const mode = appMode.mode;
  if (mode === 'session') return sessionStore.isPlaying;
  if (mode === 'performance') return decksStore.anyDeckActive;
  if (mode === 'edit') return decksStore.deckE.loopPlaying;
  return false;
}

function needsQuitConfirm(): boolean {
  return isPlayingNow() || sessionEditStore.dirty;
}

const quitModalBody = computed(() => {
  if (sessionEditStore.dirty && !isPlayingNow()) return t('quitModal.bodyDirty');
  return appMode.mode === 'session' ? t('quitModal.bodySession') : t('quitModal.bodyPlaying');
});

async function onQuitConfirmed(): Promise<void> {
  quitModalOpen.value = false;
  await appMode.confirmQuit();
}

function onQuitCancelled(): void {
  quitModalOpen.value = false;
}

function handleQuitRequested(): void {
  if (!needsQuitConfirm()) {
    appMode.confirmQuit().catch(() => {});
    return;
  }
  quitModalOpen.value = true;
}

let unlistenClose: (() => void) | null = null;
let unlistenQuit: (() => void) | null = null;

onMounted(async () => {
  settingsStore.init();
  // Closing the window must quit the app, same as Cmd+Q. Letting the default
  // close happen would leave the app running without a window on macOS.
  unlistenClose = await getCurrentWindow().onCloseRequested((event) => {
    event.preventDefault();
    handleQuitRequested();
  });
  unlistenQuit = await listen('quit-requested', handleQuitRequested);
});

onUnmounted(() => {
  decksStore.destroy();
  unlistenClose?.();
  unlistenQuit?.();
});
</script>

<style scoped>
.app {
  width: 100dvw;
  height: 100dvh;
  display: flex;
  flex-direction: column;
  /* 28px content + 1px bottom border: the border lives inside the box, and a
     27px content area cannot center the 22px buttons evenly (3px above, 2px
     below). 29px gives exactly 3px on both sides. */
  --appbar-h: 29px;
  --topstrip-h: 29px;
  --collection-bar-h: 22px;
}

.app__view {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}
</style>
