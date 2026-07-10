import { ref, onUnmounted, type Ref, type ComponentPublicInstance } from 'vue';

type TemplateRefEl = Element | ComponentPublicInstance | null;

export function useElementSize(): {
  setEl: (el: TemplateRefEl) => void;
  el: Ref<HTMLElement | null>;
  width: Ref<number>;
  height: Ref<number>;
} {
  const el = ref<HTMLElement | null>(null);
  const width = ref(0);
  const height = ref(0);
  let observer: ResizeObserver | null = null;

  function setEl(newEl: TemplateRefEl) {
    observer?.disconnect();
    observer = null;
    const target = newEl instanceof HTMLElement ? newEl : null;
    el.value = target;
    if (!target) return;
    width.value = target.clientWidth;
    height.value = target.clientHeight;
    observer = new ResizeObserver(() => {
      width.value = target.clientWidth;
      height.value = target.clientHeight;
    });
    observer.observe(target);
  }

  onUnmounted(() => observer?.disconnect());

  return { setEl, el, width, height };
}
