import type { ObjectDirective } from 'vue';

export type FaderResetBinding = {
  enabled: () => boolean;
  reset: () => void;
};

// A press that travels this far is a drag, not a click.
const CLICK_SLOP_PX = 3;

type Press = {
  value: string;
  x: number;
  y: number;
  dragging: boolean;
  onMove: (event: PointerEvent) => void;
  onUp: (event: PointerEvent) => void;
};

const presses = new WeakMap<HTMLInputElement, Press>();
const teardown = new WeakMap<HTMLInputElement, () => void>();
// Every mounted fader, because a drag has to mark all of them: `*` does not reach
// a `::-webkit-slider-thumb`, so another fader's own thumb rule would otherwise
// keep offering its cursor while this one is held.
const mounted = new Set<HTMLInputElement>();

function markDragging(dragging: boolean) {
  for (const fader of mounted) {
    if (dragging) fader.dataset.dragging = '';
    else delete fader.dataset.dragging;
  }
  document.body.classList.toggle('fader-dragging', dragging);
}

function travelled(press: Press, event: PointerEvent): boolean {
  return (
    Math.abs(event.clientX - press.x) > CLICK_SLOP_PX ||
    Math.abs(event.clientY - press.y) > CLICK_SLOP_PX
  );
}

function endPress(el: HTMLInputElement): Press | null {
  const press = presses.get(el);
  if (!press) return null;
  presses.delete(el);
  window.removeEventListener('pointermove', press.onMove);
  window.removeEventListener('pointerup', press.onUp);
  window.removeEventListener('pointercancel', press.onUp);
  markDragging(false);
  return press;
}

export const vFaderReset: ObjectDirective<HTMLInputElement, FaderResetBinding> = {
  mounted(el, binding) {
    mounted.add(el);
    if (binding.value.enabled()) el.dataset.clickResets = '';

    const onMove = (event: PointerEvent) => {
      const press = presses.get(el);
      if (!press || press.dragging || !travelled(press, event)) return;
      press.dragging = true;
      // On the body too, because the pointer leaves the fader long before the
      // gesture does and every element it crosses would show its own cursor.
      markDragging(true);
    };

    const onUp = (event: PointerEvent) => {
      const press = endPress(el);
      if (!press || !binding.value.enabled()) return;
      // WebKit jumps a range input's value to wherever the track was pressed, so
      // a press that left the value untouched is one that landed on the thumb.
      // That is the whole test: reading the thumb's own box would mean mirroring
      // its CSS size in JS.
      if (press.value !== el.value || travelled(press, event)) return;
      binding.value.reset();
    };

    const onDown = (event: PointerEvent) => {
      if (event.button !== 0) return;
      endPress(el);
      presses.set(el, {
        value: el.value,
        x: event.clientX,
        y: event.clientY,
        dragging: false,
        onMove,
        onUp
      });
      // Tracked on the window, so a release outside the fader still ends the
      // gesture rather than leaving the cursor stuck.
      window.addEventListener('pointermove', onMove);
      window.addEventListener('pointerup', onUp);
      window.addEventListener('pointercancel', onUp);
    };

    // Swallowed rather than left to reset again: the first click of the pair has
    // already moved the fader, so the second would fire against a value the user
    // never chose.
    const onDoubleClick = (event: MouseEvent) => {
      if (binding.value.enabled()) event.stopImmediatePropagation();
    };

    el.addEventListener('pointerdown', onDown);
    el.addEventListener('dblclick', onDoubleClick, { capture: true });
    teardown.set(el, () => {
      endPress(el);
      el.removeEventListener('pointerdown', onDown);
      el.removeEventListener('dblclick', onDoubleClick, { capture: true });
    });
  },

  updated(el, binding) {
    if (binding.value.enabled()) el.dataset.clickResets = '';
    else delete el.dataset.clickResets;
  },

  unmounted(el) {
    teardown.get(el)?.();
    teardown.delete(el);
    mounted.delete(el);
  }
};
