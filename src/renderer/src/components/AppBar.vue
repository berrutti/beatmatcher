<template>
  <Modal
    :open="confirmPending !== null"
    :title="confirmPending?.title ?? ''"
    confirm-label="Continue"
    @confirm="onConfirm"
    @cancel="confirmPending = null"
  >
    <p class="appbar__modal-body">{{ confirmPending?.body }}</p>
  </Modal>

  <div class="appbar">
    <Dropdown
      :label="modeLabel"
      :model-value="appMode.mode"
      :items="modeItems"
      @select="onSelectMode"
    />
    <div class="appbar__spacer" />
    <button
      class="appbar__settings-btn"
      tabindex="-1"
      title="Settings (⌘,)"
      @click="settingsStore.isOpen = true"
    >
      SETTINGS ⚙
    </button>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue';
import { useAppModeStore, type AppMode } from '@renderer/stores/appMode';
import { useDecksStore } from '@renderer/stores/decks';
import { useSessionStore } from '@renderer/stores/session';
import { useSettingsStore } from '@renderer/stores/settings';
import Dropdown from '@renderer/components/Dropdown.vue';
import Modal from '@renderer/components/modals/Modal.vue';

const appMode = useAppModeStore();
const decksStore = useDecksStore();
const sessionStore = useSessionStore();
const settingsStore = useSettingsStore();

type ConfirmPayload = { title: string; body: string; next: AppMode };
const confirmPending = ref<ConfirmPayload | null>(null);

const modeItems = [
  { value: 'performance', label: 'PERFORMANCE' },
  { value: 'edit', label: 'EDIT' },
  { value: 'session', label: 'SESSION' }
];

const modeLabel = computed(() => modeItems.find((m) => m.value === appMode.mode)?.label ?? '');

function needsConfirm(next: AppMode): ConfirmPayload | null {
  const prev = appMode.mode;
  if (prev === next) return null;

  if (next === 'edit' && decksStore.anyDeckActive) {
    return {
      title: 'Enter Edit mode?',
      body: 'Playback is running. Playback will stop when entering Edit mode.',
      next
    };
  }

  if (next === 'session' && decksStore.anyDeckLoaded) {
    return {
      title: 'Enter Session view?',
      body: 'All loaded tracks will be unloaded. You can reload them when you return.',
      next
    };
  }

  if (prev === 'session' && sessionStore.session !== null) {
    return {
      title: 'Leave Session view?',
      body: 'The loaded session will be closed and playback will stop.',
      next
    };
  }

  return null;
}

async function onSelectMode(next: string) {
  const typedNext = next as AppMode;
  if (appMode.mode === typedNext) return;
  const confirm = needsConfirm(typedNext);
  if (confirm) {
    confirmPending.value = confirm;
    return;
  }
  await appMode.switchTo(typedNext);
}

async function onConfirm() {
  const next = confirmPending.value?.next;
  confirmPending.value = null;
  if (next) await appMode.switchTo(next);
}
</script>

<style scoped>
.appbar {
  width: 100%;
  height: 28px;
  display: flex;
  align-items: center;
  padding: 0 12px;
  border-bottom: 1px solid var(--color-border);
  background: var(--color-bg);
  font-family: var(--font);
  flex-shrink: 0;
  gap: 6px;
  position: relative;
  z-index: 100;
}

.appbar__spacer {
  flex: 1;
}

.appbar__settings-btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 10px;
  letter-spacing: 0.12em;
  height: 22px;
  border-radius: 3px;
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 0 8px;
  flex-shrink: 0;
  white-space: nowrap;
}

.appbar__settings-btn:hover {
  border-color: var(--color-text);
  color: var(--color-text);
}

.appbar__modal-body {
  font-size: 0.75rem;
  color: var(--color-muted);
  line-height: 1.5;
  margin: 0;
}
</style>
