import type { Directive } from 'vue';
import { useTooltip } from '@renderer/composables/useTooltip';

const { scheduleShow, hide } = useTooltip();

function onEnter(e: MouseEvent) {
  const el = e.currentTarget as HTMLElement;
  const text = el.dataset.tooltipText;
  if (!text) return;
  scheduleShow(text, el);
}

function onLeave() {
  hide();
}

export const vTooltip: Directive<HTMLElement, string | null | undefined> = {
  mounted(el, binding) {
    el.dataset.tooltipText = binding.value ?? '';
    el.addEventListener('mouseenter', onEnter);
    el.addEventListener('mouseleave', onLeave);
    el.addEventListener('mousedown', onLeave);
  },
  updated(el, binding) {
    el.dataset.tooltipText = binding.value ?? '';
  },
  unmounted(el) {
    el.removeEventListener('mouseenter', onEnter);
    el.removeEventListener('mouseleave', onLeave);
    el.removeEventListener('mousedown', onLeave);
    hide();
  }
};
