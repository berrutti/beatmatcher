import type { ObjectDirective } from 'vue';
import { useTooltip } from '@renderer/composables/useTooltip';
import { overflows } from '@renderer/utils/overflow';

const { scheduleShow, hide } = useTooltip();

function onEnter(e: MouseEvent) {
  if (!(e.currentTarget instanceof HTMLElement)) return;
  const el = e.currentTarget;
  const text = el.dataset.tooltipText;
  if (!text) return;
  // Measured on hover rather than at mount, so a resized column is never stale.
  if ('tooltipTruncated' in el.dataset && !overflows(el)) return;
  scheduleShow(text, el);
}

function onLeave(e: MouseEvent) {
  if (!(e.currentTarget instanceof HTMLElement)) return;
  hide(e.currentTarget);
}

export const vTooltip: ObjectDirective<HTMLElement, string | null | undefined> = {
  mounted(el, binding) {
    el.dataset.tooltipText = binding.value ?? '';
    if (binding.modifiers.truncated) el.dataset.tooltipTruncated = '';
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
    hide(el);
  }
};
