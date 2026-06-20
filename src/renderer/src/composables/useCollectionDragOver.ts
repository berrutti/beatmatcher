import { onUnmounted, ref, watch, type Ref } from 'vue';
import { useCollectionStore } from '@renderer/stores/collection';

export function useCollectionDragOver(
  elementRef: Ref<HTMLElement | null>,
  excludePath?: () => string | null
) {
  const collectionStore = useCollectionStore();
  const isDragOver = ref(false);

  function onWindowPointerMove(e: PointerEvent) {
    const element = elementRef.value;
    if (!element) return;
    const rect = element.getBoundingClientRect();
    const over =
      e.clientX >= rect.left &&
      e.clientX <= rect.right &&
      e.clientY >= rect.top &&
      e.clientY <= rect.bottom;
    const isExcluded = excludePath ? collectionStore.draggingPath === excludePath() : false;
    isDragOver.value = over && !isExcluded;
  }

  const stopWatch = watch(
    () => collectionStore.draggingPath,
    (path) => {
      if (path) {
        window.addEventListener('pointermove', onWindowPointerMove);
      } else {
        window.removeEventListener('pointermove', onWindowPointerMove);
        isDragOver.value = false;
      }
    }
  );

  onUnmounted(() => {
    stopWatch();
    window.removeEventListener('pointermove', onWindowPointerMove);
  });

  return { isDragOver };
}
