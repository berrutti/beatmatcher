import { ref } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import { useDecksStore, DECKS_DISPOSITION } from '@renderer/stores/decks';
import { useMixerStore } from '@renderer/stores/mixer';
import { useSessionStore } from '@renderer/stores/session';

export type AppMode = 'performance' | 'edit' | 'session';

export const useAppModeStore = defineStore('appMode', () => {
  const mode = ref<AppMode>('performance');

  async function switchTo(next: AppMode): Promise<void> {
    const prev = mode.value;
    if (prev === next) return;

    // Ahead of the transition's own awaits so entering session stops MIDI before
    // anything is torn down. Leaving it re-enables MIDI early, which is harmless:
    // the scheduler is already stopping and the mixer is reset right after.
    await invoke('set_app_mode', { mode: next });

    const decks = useDecksStore();
    const mixer = useMixerStore();
    const session = useSessionStore();

    if (prev === 'session') {
      await session.exit();
      mixer.reset();
    }

    if (next === 'session') {
      await decks.ejectAll();
    }

    if (next === 'edit') {
      await Promise.all(
        DECKS_DISPOSITION.filter((id) => decks.decks[id].loopPlaying).map((id) =>
          decks.decks[id].stop()
        )
      );
    }

    mode.value = next;
  }

  async function confirmQuit(): Promise<void> {
    await invoke('confirm_quit');
  }

  return { mode, switchTo, confirmQuit };
});
