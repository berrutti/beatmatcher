import { defineStore } from 'pinia';
import { ref } from 'vue';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';

export type UpdateStatus = 'idle' | 'checking' | 'available' | 'downloading' | 'ready' | 'error';

export const useUpdaterStore = defineStore('updater', () => {
  const status = ref<UpdateStatus>('idle');
  const newVersion = ref<string | null>(null);
  const notes = ref<string | null>(null);
  const downloadedBytes = ref(0);
  const totalBytes = ref<number | null>(null);

  let pending: Update | null = null;

  async function checkForUpdate(): Promise<void> {
    if (status.value === 'checking' || status.value === 'downloading') return;
    status.value = 'checking';
    try {
      const update = await check();
      if (update) {
        pending = update;
        newVersion.value = update.version;
        notes.value = update.body ?? null;
        status.value = 'available';
      } else {
        status.value = 'idle';
      }
    } catch {
      // A failed check (offline, no release yet) must not interrupt the user;
      // surface errors only for a user-initiated download below.
      status.value = 'idle';
    }
  }

  async function downloadAndInstall(): Promise<void> {
    if (!pending) return;
    status.value = 'downloading';
    downloadedBytes.value = 0;
    totalBytes.value = null;
    try {
      await pending.downloadAndInstall((event) => {
        if (event.event === 'Started') {
          totalBytes.value = event.data.contentLength ?? null;
        } else if (event.event === 'Progress') {
          downloadedBytes.value += event.data.chunkLength;
        } else if (event.event === 'Finished') {
          status.value = 'ready';
        }
      });
      await relaunch();
    } catch {
      status.value = 'error';
    }
  }

  function dismiss(): void {
    status.value = 'idle';
  }

  return {
    status,
    newVersion,
    notes,
    downloadedBytes,
    totalBytes,
    checkForUpdate,
    downloadAndInstall,
    dismiss
  };
});
