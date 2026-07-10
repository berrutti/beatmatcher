import { ref } from 'vue';
import ColumnVisibilityMenu from '@renderer/components/collection/ColumnVisibilityMenu.vue';

// Each table owns its own instance, same reasoning as useBpmModal: the ref
// has to live wherever <ColumnVisibilityMenu> is actually rendered.
export function useColumnVisibilityMenuTrigger() {
  const columnMenuEl = ref<InstanceType<typeof ColumnVisibilityMenu> | null>(null);

  function onHeaderContextmenu(e: MouseEvent) {
    columnMenuEl.value?.open(e);
  }

  return { columnMenuEl, onHeaderContextmenu };
}
