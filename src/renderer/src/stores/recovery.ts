import { defineStore } from 'pinia';
import { ref } from 'vue';
import { call } from '@renderer/tauriCommands';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';
import type { Recoverable } from '@renderer/utils/types';

export type RecoverableFile = 'audio' | 'log';

function isRecoverable(value: unknown): value is Recoverable {
  if (typeof value !== 'object' || value === null) return false;
  const item: Record<string, unknown> = { ...value };
  return (
    typeof item.id === 'string' &&
    (item.kind === 'recording' || item.kind === 'render') &&
    typeof item.startedAt === 'number' &&
    typeof item.suggestedName === 'string' &&
    typeof item.audioBytes === 'number'
  );
}

export const useRecoveryStore = defineStore('recovery', () => {
  // Everything the last run left unfinished. Re-read after every decision, because
  // saving a file removes it from the job and a job holding nothing is swept.
  const pending = ref<Recoverable[]>([]);

  async function refresh(): Promise<void> {
    const listed = await call('list_recoverable');
    pending.value = listed.filter(isRecoverable);
  }

  async function saveFile(item: Recoverable, file: RecoverableFile): Promise<boolean> {
    const format = file === 'log' ? 'session' : item.audioPath?.endsWith('.flac') ? 'flac' : 'wav';
    const dest = await call('pick_save_path', { format, baseName: item.suggestedName });
    if (!dest) return false;
    await call('recover_save_file', { id: item.id, file, dest });
    await refresh();
    return true;
  }

  async function discard(item: Recoverable): Promise<void> {
    await call('recover_discard', { id: item.id });
    await refresh();
  }

  // Suppresses the confirmation, never the recovery prompt itself: a set that is not
  // offered back is a set that is silently gone.
  const skipDiscardConfirm = ref(storageGet(STORAGE_KEYS.skipDiscardConfirm, false));

  function alwaysSkipDiscardConfirm(): void {
    skipDiscardConfirm.value = true;
    storageSet(STORAGE_KEYS.skipDiscardConfirm, true);
  }

  return { pending, refresh, saveFile, discard, skipDiscardConfirm, alwaysSkipDiscardConfirm };
});
