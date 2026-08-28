import type { ObjectDirective } from 'vue';
import { pressIsOnThumb, type ThumbGeometry } from '@renderer/utils/sliderThumb';

export type SliderResetBinding = {
  enabled: boolean;
  reset: () => void;
};

// A press that travels this far is a drag, not a click.
const CLICK_SLOP_PX = 3;

type Press = {
  value: string;
  x: number;
  y: number;
  // Null when the slider declares no thumb length, which falls back to reading
  // the value for a jump.
  onThumb: boolean | null;
  dragging: boolean;
  onMove: (event: PointerEvent) => void;
  onUp: () => void;
};

const presses = new WeakMap<HTMLInputElement, Press>();
const teardown = new WeakMap<HTMLInputElement, () => void>();
// A drag marks every slider: `*` does not reach a `::-webkit-slider-thumb`, so
// another slider's own thumb rule would keep offering its cursor.
const mounted = new Set<HTMLInputElement>();

function markDragging(dragging: boolean) {
  for (const slider of mounted) {
    if (dragging) slider.dataset.dragging = '';
    else delete slider.dataset.dragging;
  }
  document.body.classList.toggle('slider-dragging', dragging);
}

// A slider opts in by declaring how long its thumb is along the track, which is
// the one thing this cannot read: a pseudo-element has no box to measure.
function geometryOf(
  el: HTMLInputElement
): { geometry: ThumbGeometry; vertical: boolean; grace: number } | null {
  const style = getComputedStyle(el);
  const thumbLength = parseFloat(style.getPropertyValue('--slider-thumb-length'));
  if (!Number.isFinite(thumbLength) || thumbLength <= 0) return null;
  const grace = parseFloat(style.getPropertyValue('--slider-thumb-grace'));
  const vertical = style.writingMode.startsWith('vertical');
  const trackLength = vertical ? el.clientHeight : el.clientWidth;
  if (trackLength <= 0) return null;
  return {
    vertical,
    grace: Number.isFinite(grace) && grace > 0 ? grace : 0,
    geometry: {
      min: Number(el.min === '' ? 0 : el.min),
      max: Number(el.max === '' ? 100 : el.max),
      value: Number(el.value),
      trackLength,
      thumbLength,
      maxAtStart: vertical
    }
  };
}

function pressedThumb(el: HTMLInputElement, event: PointerEvent): boolean | null {
  const resolved = geometryOf(el);
  if (!resolved) return null;
  const rect = el.getBoundingClientRect();
  const along = resolved.vertical ? event.clientY - rect.top : event.clientX - rect.left;
  return pressIsOnThumb(resolved.geometry, along, resolved.grace);
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

export const vSliderReset: ObjectDirective<HTMLInputElement, SliderResetBinding> = {
  mounted(el, binding) {
    mounted.add(el);
    if (binding.value.enabled) el.dataset.clickResets = '';

    const onMove = (event: PointerEvent) => {
      const press = presses.get(el);
      if (!press || press.dragging || !travelled(press, event)) return;
      press.dragging = true;
      // On the body too, because the pointer leaves the slider long before the
      // gesture does and every element it crosses would show its own cursor.
      markDragging(true);
    };

    const onUp = () => {
      const press = endPress(el);
      // The sticky flag, not the release point: a drag that wandered and came
      // back near where it started is still a drag.
      if (!press || !binding.value.enabled || press.dragging) return;
      // WebKit jumps the value for a press outside the thumb's box, which
      // excludes the shadow the eye reads as part of it.
      if (press.onThumb === false) return;
      if (press.onThumb === null && press.value !== el.value) return;
      el.value = press.value;
      binding.value.reset();
    };

    const onDown = (event: PointerEvent) => {
      if (event.button !== 0) return;
      endPress(el);
      presses.set(el, {
        value: el.value,
        x: event.clientX,
        y: event.clientY,
        // Resolved only where it is read: it forces a style and layout pass.
        onThumb: binding.value.enabled ? pressedThumb(el, event) : null,
        dragging: false,
        onMove,
        onUp
      });
      // Tracked on the window, so a release outside the slider still ends the
      // gesture rather than leaving the cursor stuck.
      window.addEventListener('pointermove', onMove);
      window.addEventListener('pointerup', onUp);
      window.addEventListener('pointercancel', onUp);
    };

    // The first click of the pair already reset it, so the second would fire
    // against a value the user never chose.
    const onDoubleClick = (event: MouseEvent) => {
      if (binding.value.enabled) event.stopImmediatePropagation();
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
    // Every re-render of the host reaches this, so the common no-change case
    // must not touch the DOM.
    if (binding.value.enabled === binding.oldValue?.enabled) return;
    if (binding.value.enabled) el.dataset.clickResets = '';
    else delete el.dataset.clickResets;
  },

  unmounted(el) {
    teardown.get(el)?.();
    teardown.delete(el);
    mounted.delete(el);
  }
};
