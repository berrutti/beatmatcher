<template>
  <Teleport to="body">
    <div
      v-if="contextMenu"
      ref="contextMenuEl"
      class="context-menu"
      :style="{ left: contextMenu.x + 'px', top: contextMenu.y + 'px' }"
      @click.stop
    >
      <button tabindex="-1" class="context-menu__item" @click="onReanalyze">
        {{ $t('browser.recalcBpm') }}
      </button>
      <button tabindex="-1" class="context-menu__item" @click="onSetBpm">
        {{ $t('browser.setBpm') }}
      </button>
      <div
        v-if="store.playlists.length > 0"
        class="context-menu__item context-menu__item--sub"
        @mouseenter="onSubEnter"
      >
        <span>{{ $t('browser.addToPlaylist') }}</span>
        <span class="context-menu__arrow">▶</span>
        <div class="context-menu__submenu" :class="{ 'context-menu__submenu--flip': subFlipped }">
          <button
            tabindex="-1"
            v-for="playlist in store.playlists"
            :key="playlist.id"
            class="context-menu__item"
            @click="onAddToPlaylist(playlist.id)"
          >
            {{ playlist.name }}
          </button>
        </div>
      </div>
      <div v-else class="context-menu__item context-menu__item--disabled">
        <span>{{ $t('browser.addToPlaylist') }}</span>
        <span class="context-menu__item-hint">{{ $t('browser.noPlaylistsShort') }}</span>
      </div>
    </div>
    <div
      v-if="contextMenu"
      class="context-menu__backdrop"
      @click="close"
      @contextmenu.prevent="close"
    />
  </Teleport>
</template>

<script setup lang="ts">
import { ref, nextTick } from 'vue';
import { useCollectionStore } from '@renderer/stores/collection';
import { clampToViewport } from '@renderer/utils/menuPosition';

const store = useCollectionStore();
const emit = defineEmits<{ 'set-bpm': [trackId: string] }>();

type ContextMenu = { trackId: string; x: number; y: number };
const contextMenu = ref<ContextMenu | null>(null);
const contextMenuEl = ref<HTMLElement | null>(null);
const subFlipped = ref(false);

async function open(e: MouseEvent, trackId: string) {
  contextMenu.value = { trackId, x: e.clientX, y: e.clientY };
  await nextTick();
  if (!contextMenuEl.value || !contextMenu.value) return;
  const rect = contextMenuEl.value.getBoundingClientRect();
  contextMenu.value = { ...contextMenu.value, ...clampToViewport(e, rect) };
}

function close() {
  contextMenu.value = null;
}

function onSubEnter(e: MouseEvent) {
  const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
  const submenuHeight = store.playlists.length * 32 + 8;
  subFlipped.value = rect.top + submenuHeight > window.innerHeight;
}

function onReanalyze() {
  if (contextMenu.value) store.reanalyzeTrack(contextMenu.value.trackId);
  close();
}

function onSetBpm() {
  if (contextMenu.value) emit('set-bpm', contextMenu.value.trackId);
  close();
}

function onAddToPlaylist(playlistId: string) {
  const trackId = contextMenu.value?.trackId;
  const track = store.tracks.find((t) => t.id === trackId);
  if (track?.path) store.addToPlaylist(playlistId, track.path);
  close();
}

defineExpose({ open });
</script>
