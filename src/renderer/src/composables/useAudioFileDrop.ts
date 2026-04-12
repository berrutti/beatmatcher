import { ref, onMounted, onUnmounted } from 'vue';
import type { Ref } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';

type DragDropPayload = {
  paths: string[];
  position: { x: number; y: number };
};

type DragOverPayload = {
  position: { x: number; y: number };
};

function tauriReady(): Promise<void> {
  return new Promise((resolve) => {
    function check() {
      if ((window as Record<string, unknown>).__TAURI_INTERNALS__) {
        resolve();
      } else {
        setTimeout(check, 25);
      }
    }
    check();
  });
}

function hitTest(el: HTMLElement | null, x: number, y: number): boolean {
  if (!el) return false;
  const rect = el.getBoundingClientRect();
  return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
}

export function useAudioFileDrop(
  elRef: Ref<HTMLElement | null>,
  onFilePath: (path: string) => void
) {
  const isDragOver = ref(false);
  const unlisteners: Array<() => void> = [];

  onMounted(async () => {
    await tauriReady();

    const unOver = await listen<DragOverPayload>('tauri://drag-over', ({ payload }) => {
      isDragOver.value = hitTest(elRef.value, payload.position.x, payload.position.y);
    });

    const unLeave = await listen('tauri://drag-leave', () => {
      isDragOver.value = false;
    });

    const unDrop = await listen<DragDropPayload>('tauri://drag-drop', ({ payload }) => {
      isDragOver.value = false;
      if (!payload.paths.length) return;
      if (hitTest(elRef.value, payload.position.x, payload.position.y)) {
        onFilePath(payload.paths[0]);
      }
    });

    unlisteners.push(unOver, unLeave, unDrop);
  });

  onUnmounted(() => {
    for (const u of unlisteners) u();
  });

  async function openFileDialog() {
    await tauriReady();
    const path = await invoke<string | null>('open_file_dialog');
    if (path) onFilePath(path);
  }

  return { isDragOver, openFileDialog };
}
