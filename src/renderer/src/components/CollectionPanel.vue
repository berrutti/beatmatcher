<template>
  <div
    class="collection"
    :class="{ 'collection--drag-over': isDragOver }"
    @dragover="onDragOver"
    @dragleave="onDragLeave"
    @drop="onDrop"
  >
    <div class="collection__header">
      <span class="collection__title">COLLECTION</span>
      <span v-if="!isTauri" class="collection__web-info">
        ℹ
        <span class="collection__web-tooltip"
          >You are running BeatMatcher in the browser. Files added to your collection need to be
          re-added after a page refresh.</span
        >
      </span>
      <span v-if="store.tracks.length > 0" class="collection__count">{{
        store.tracks.length
      }}</span>
      <button v-if="store.hasPending" class="collection__header-btn" @click="store.analyzeAll()">
        ANALYZE ALL
      </button>
      <button class="collection__header-btn" @click="openFileDialog">ADD</button>
      <button
        v-if="store.tracks.length > 0"
        class="collection__header-btn collection__header-btn--muted"
        @click="store.clearAll()"
      >
        CLEAR
      </button>
    </div>
    <div class="collection__body">
      <div v-if="store.tracks.length === 0" class="collection__empty">
        Drop audio files or folders here, or use ADD
      </div>
      <div v-else class="collection__list">
        <div
          v-for="track in store.tracks"
          :key="track.id"
          class="collection__item"
          :class="`collection__item--${track.status}`"
          @pointerdown="onItemPointerDown($event, track)"
        >
          <span class="collection__item-name" :title="track.name">{{
            displayName(track.name)
          }}</span>
          <span v-if="store.bpmFor(track) !== null" class="collection__item-bpm">
            {{ store.bpmFor(track)!.toFixed(1) }} BPM
          </span>
          <span v-else-if="track.status === 'analyzing'" class="collection__item-tag">
            detecting...
          </span>
          <span
            v-else-if="track.status === 'error'"
            class="collection__item-tag collection__item-tag--error"
          >
            error
          </span>
          <span
            v-if="track.status === 'missing'"
            class="collection__item-tag collection__item-tag--missing"
          >
            missing
          </span>
          <button
            v-if="track.status === 'idle'"
            class="collection__item-btn"
            @click.stop="store.analyzeTrack(track.id)"
          >
            ANALYZE
          </button>
          <button
            v-if="track.status === 'error'"
            class="collection__item-btn"
            @click.stop="openBpmModal(track.id)"
          >
            SET BPM
          </button>
          <button class="collection__item-remove" @click.stop="store.removeTrack(track.id)">
            ✕
          </button>
        </div>
      </div>
    </div>
    <BpmModal
      :open="bpmModalTrackId !== null"
      :current-bpm="null"
      @submit="onBpmSubmit"
      @cancel="bpmModalTrackId = null"
    />
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import { useCollectionStore } from '@renderer/stores/collection';
import type { CollectionEntry } from '@renderer/stores/collection';
import BpmModal from '@renderer/components/BpmModal.vue';

const store = useCollectionStore();
const isDragOver = ref(false);
const bpmModalTrackId = ref<string | null>(null);
const isTauri = '__TAURI_INTERNALS__' in window;

function openBpmModal(id: string) {
  bpmModalTrackId.value = id;
}

function onBpmSubmit(bpm: number) {
  if (bpmModalTrackId.value) store.setBpm(bpmModalTrackId.value, bpm);
  bpmModalTrackId.value = null;
}

function isAudio(file: File): boolean {
  return file.type.startsWith('audio/') || /\.(mp3|wav|flac|aac|ogg|m4a|aiff?)$/i.test(file.name);
}

function displayName(filename: string): string {
  return filename.replace(/\.(mp3|wav|flac|aac|ogg|m4a|aiff?)$/i, '');
}

async function readEntry(entry: FileSystemEntry): Promise<File[]> {
  if (entry.isFile) {
    return new Promise((resolve) => {
      (entry as FileSystemFileEntry).file(
        (f) => resolve(isAudio(f) ? [f] : []),
        () => resolve([])
      );
    });
  }
  if (entry.isDirectory) {
    const reader = (entry as FileSystemDirectoryEntry).createReader();
    const all: FileSystemEntry[] = [];
    while (true) {
      const batch = await new Promise<FileSystemEntry[]>((resolve) => {
        reader.readEntries(resolve, () => resolve([]));
      });
      if (batch.length === 0) break;
      all.push(...batch);
    }
    return (await Promise.all(all.map(readEntry))).flat();
  }
  return [];
}

function onDragOver(e: DragEvent) {
  if (store.draggingFile) return;
  e.preventDefault();
  isDragOver.value = true;
}

function onDragLeave() {
  isDragOver.value = false;
}

async function onDrop(e: DragEvent) {
  if (store.draggingFile) return;
  e.preventDefault();
  e.stopPropagation();
  isDragOver.value = false;
  const items = Array.from(e.dataTransfer?.items ?? []);
  const entries = items.map((i) => i.webkitGetAsEntry()).filter(Boolean) as FileSystemEntry[];
  const files = (await Promise.all(entries.map(readEntry))).flat();
  if (files.length > 0) store.addFiles(files);
}

const DRAG_THRESHOLD = 5;

function onItemPointerDown(e: PointerEvent, track: CollectionEntry) {
  if (e.button !== 0 || track.status !== 'ready' || !track.file || !track.path) return;
  if ((e.target as HTMLElement).closest('button')) return;

  const startX = e.clientX;
  const startY = e.clientY;
  const file = track.file;
  const path = track.path;
  let active = false;

  function onMove(ev: PointerEvent) {
    if (!active) {
      if (
        Math.abs(ev.clientX - startX) < DRAG_THRESHOLD &&
        Math.abs(ev.clientY - startY) < DRAG_THRESHOLD
      )
        return;
      active = true;
      store.startDrag(file, path);
      document.body.style.cursor = 'grabbing';
    }
  }

  function cleanup() {
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
    window.removeEventListener('pointercancel', onCancel);
  }

  function onUp(ev: PointerEvent) {
    cleanup();
    if (!active) return;
    document.body.style.cursor = '';
    const el = document.elementFromPoint(ev.clientX, ev.clientY);
    const deckEl = el?.closest('[data-deck-id]') as HTMLElement | null;
    const deckId = deckEl?.dataset.deckId;
    if (deckId) {
      window.dispatchEvent(new CustomEvent('bm:collection-drop', { detail: { deckId, path } }));
    }
    store.endDrag();
  }

  function onCancel() {
    cleanup();
    if (!active) return;
    document.body.style.cursor = '';
    store.endDrag();
  }

  window.addEventListener('pointermove', onMove);
  window.addEventListener('pointerup', onUp);
  window.addEventListener('pointercancel', onCancel);
}

async function openFileDialog() {
  if (isTauri) {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const result = await open({
      multiple: true,
      filters: [
        { name: 'Audio', extensions: ['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a', 'aif', 'aiff'] }
      ]
    });
    if (result) {
      const paths = Array.isArray(result) ? result : [result];
      store.addFilesFromPaths(paths);
    }
    return;
  }
  const input = document.createElement('input');
  input.type = 'file';
  input.accept = 'audio/*';
  input.multiple = true;
  input.style.display = 'none';
  document.body.appendChild(input);
  input.onchange = () => {
    document.body.removeChild(input);
    if (input.files) store.addFiles(Array.from(input.files).filter(isAudio));
  };
  input.click();
}
</script>

<style scoped>
.collection {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--color-bg);
  border-top: 1px solid var(--color-border);
  transition: outline 0.1s;
}

.collection--drag-over {
  outline: 2px dashed var(--color-muted);
  outline-offset: -4px;
}

.collection__header {
  display: flex;
  align-items: center;
  gap: 0.8em;
  padding: 0 1em;
  height: 32px;
  flex-shrink: 0;
  border-bottom: 1px solid var(--color-border);
}

.collection__title {
  font-size: 0.65em;
  font-weight: 700;
  letter-spacing: 0.2em;
  color: var(--color-muted);
}

.collection__count {
  font-size: 0.65em;
  color: var(--color-muted);
  opacity: 0.6;
}

.collection__web-info {
  position: relative;
  font-size: 0.65em;
  color: var(--color-muted);
  opacity: 0.5;
  cursor: default;
}
.collection__web-info:hover {
  opacity: 1;
}
.collection__web-tooltip {
  display: none;
  position: absolute;
  top: calc(100% + 6px);
  left: 0;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  color: var(--color-text);
  font-size: 1.2em;
  letter-spacing: 0.03em;
  padding: 0.5em 0.8em;
  border-radius: 4px;
  width: 22em;
  line-height: 1.5;
  z-index: 10;
  pointer-events: none;
}
.collection__web-info:hover .collection__web-tooltip {
  display: block;
}

.collection__header-btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.6em;
  letter-spacing: 0.12em;
  padding: 0.25em 0.7em;
  border-radius: 3px;
  cursor: pointer;
  margin-left: auto;
}
.collection__header-btn + .collection__header-btn {
  margin-left: 0;
}
.collection__header-btn:hover {
  border-color: #555;
  color: var(--color-text);
}
.collection__header-btn--muted {
  margin-left: 0;
  opacity: 0.5;
}
.collection__header-btn--muted:hover {
  opacity: 1;
}

.collection__body {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
}

.collection__empty {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  font-size: 0.65em;
  color: var(--color-muted);
  opacity: 0.5;
  letter-spacing: 0.05em;
}

.collection__list {
  display: flex;
  flex-direction: column;
}

.collection__item {
  display: flex;
  align-items: center;
  gap: 0.6em;
  padding: 0 1em;
  height: 32px;
  border-bottom: 1px solid var(--color-border);
  cursor: default;
  transition: background 0.1s;
  font-size: 0.7em;
}
.collection__item:hover {
  background: var(--color-surface);
}
.collection__item--ready {
  cursor: grab;
}
.collection__item--ready:active {
  cursor: grabbing;
}

.collection__item-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--color-text);
}

.collection__item-bpm {
  color: var(--color-muted);
  font-size: 0.9em;
  white-space: nowrap;
  letter-spacing: 0.05em;
}

.collection__item-tag {
  font-size: 0.85em;
  color: var(--color-muted);
  opacity: 0.6;
  white-space: nowrap;
}
.collection__item-tag--error {
  color: #ef4444;
  opacity: 1;
}

.collection__item-tag--missing {
  opacity: 0.4;
}

.collection__item--missing {
  opacity: 0.45;
  cursor: default;
}

.collection__item--analyzing .collection__item-name {
  opacity: 0.5;
}

.collection__item-btn {
  background: transparent;
  border: 1px solid var(--color-border);
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.85em;
  letter-spacing: 0.1em;
  padding: 0.15em 0.5em;
  border-radius: 3px;
  cursor: pointer;
  white-space: nowrap;
}
.collection__item-btn:hover {
  border-color: #555;
  color: var(--color-text);
}

.collection__item-remove {
  background: transparent;
  border: none;
  color: var(--color-muted);
  font-family: var(--font);
  font-size: 0.9em;
  width: 1.4em;
  height: 1.4em;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 3px;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.1s;
  flex-shrink: 0;
  padding: 0;
}
.collection__item:hover .collection__item-remove {
  opacity: 0.5;
}
.collection__item-remove:hover {
  opacity: 1 !important;
  color: var(--color-text);
}
</style>
