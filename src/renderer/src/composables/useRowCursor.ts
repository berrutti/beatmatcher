import { computed, watch } from 'vue';
import { useBrowseStore } from '@renderer/stores/browse';

// The cursor is one thing across all three lists, so the list on screen owns
// nothing but the order it presents.
export function useRowCursor(listId: () => string, keys: () => string[]) {
  const browse = useBrowseStore();

  // The active list is part of the source: a view whose own id never changes still has to
  // re-register when it becomes the one on screen.
  watch([() => browse.listId, keys], ([, next]) => browse.setRows(listId(), next), {
    immediate: true
  });

  return {
    cursorIndex: computed(() => browse.cursorIndex),
    cursorKey: computed(() => browse.cursorKey),
    isCursor: (key: string) => browse.cursorKey === key
  };
}
