// @vitest-environment happy-dom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { mount } from '@vue/test-utils';
import { defineComponent, h, withDirectives } from 'vue';
import { vSliderReset } from '@renderer/directives/sliderReset';

function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

// happy-dom drops the MouseEvent init fields from a PointerEvent, so the
// coordinates the directive reads have to be put back by hand.
function pointer(type: string, x: number, y: number, button = 0): PointerEvent {
  const event = new Event(type, { bubbles: true });
  Object.defineProperties(event, {
    clientX: { value: x },
    clientY: { value: y },
    button: { value: button }
  });
  return event as PointerEvent;
}

function render(enabled: boolean) {
  const reset = vi.fn();
  const component = defineComponent({
    render: () =>
      withDirectives(h('input', { type: 'range', min: '0', max: '1', step: '0.01' }), [
        [vSliderReset, { enabled, reset }]
      ])
  });
  const wrapper = mount(component, { attachTo: document.body });
  const el = wrapper.element;
  if (!(el instanceof HTMLInputElement)) throw new Error('the directive needs a real input');
  return { el, reset, wrapper };
}

// happy-dom lays nothing out, so the input reports a zero-sized box and the
// directive falls back to reading the value for a jump. That is the path a
// slider declaring no thumb length takes, and the one these drive.
const SLOP = 3;

type Gesture = {
  from: { x: number; y: number };
  waypoints: { x: number; y: number }[];
  to: { x: number; y: number };
  button: number;
  jumped: boolean;
};

function fuzzGesture(random: () => number): Gesture {
  const from = { x: Math.round(random() * 200), y: Math.round(random() * 200) };
  const waypointCount = Math.floor(random() * 4);
  const waypoints = Array.from({ length: waypointCount }, () => ({
    x: Math.round(from.x + (random() - 0.5) * 60),
    y: Math.round(from.y + (random() - 0.5) * 60)
  }));
  // Half the releases land back near the press, which is the case a release-point
  // check gets wrong.
  const nearStart = random() < 0.5;
  return {
    from,
    waypoints,
    to: nearStart
      ? {
          x: from.x + Math.round((random() - 0.5) * 4),
          y: from.y + Math.round((random() - 0.5) * 4)
        }
      : { x: Math.round(random() * 200), y: Math.round(random() * 200) },
    button: random() < 0.15 ? 2 : 0,
    jumped: random() < 0.5
  };
}

function travelled(from: { x: number; y: number }, at: { x: number; y: number }): boolean {
  return Math.abs(at.x - from.x) > SLOP || Math.abs(at.y - from.y) > SLOP;
}

function play(el: HTMLInputElement, gesture: Gesture): void {
  el.dispatchEvent(pointer('pointerdown', gesture.from.x, gesture.from.y, gesture.button));
  // WebKit sets the value on a press that missed the thumb, before any movement.
  if (gesture.jumped && gesture.button === 0) el.value = '0.77';
  for (const at of gesture.waypoints) window.dispatchEvent(pointer('pointermove', at.x, at.y));
  window.dispatchEvent(pointer('pointerup', gesture.to.x, gesture.to.y));
}

describe('the slider reset directive under fuzzed gestures', () => {
  beforeEach(() => {
    document.body.className = '';
  });

  afterEach(() => {
    document.body.innerHTML = '';
    document.body.className = '';
  });

  it('resets exactly when a still press left the value alone', () => {
    const random = makeRandom(31);
    let resets = 0;
    for (let step = 0; step < 2000; step++) {
      const { el, reset, wrapper } = render(true);
      el.value = '0.4';
      const gesture = fuzzGesture(random);

      play(el, gesture);

      const dragged = [...gesture.waypoints].some((at) => travelled(gesture.from, at));
      const expected = gesture.button === 0 && !dragged && !gesture.jumped;
      expect(reset.mock.calls.length > 0, `${step}`).toBe(expected);
      if (expected) resets++;
      wrapper.unmount();
    }
    expect(resets).toBeGreaterThan(0);
  });

  it('never resets a gesture that became a drag, however it ended', () => {
    const random = makeRandom(37);
    let drags = 0;
    for (let step = 0; step < 2000; step++) {
      const { el, reset, wrapper } = render(true);
      el.value = '0.4';
      const gesture = fuzzGesture(random);
      const dragged = gesture.waypoints.some((at) => travelled(gesture.from, at));

      play(el, gesture);

      if (dragged && gesture.button === 0) {
        drags++;
        expect(reset, `${step}`).not.toHaveBeenCalled();
      }
      wrapper.unmount();
    }
    expect(drags).toBeGreaterThan(0);
  });

  it('never resets while the setting is off', () => {
    const random = makeRandom(41);
    for (let step = 0; step < 1000; step++) {
      const { el, reset, wrapper } = render(false);
      el.value = '0.4';

      play(el, fuzzGesture(random));

      expect(reset, `${step}`).not.toHaveBeenCalled();
      wrapper.unmount();
    }
  });

  it('leaves the document unmarked once every gesture has ended', () => {
    const random = makeRandom(43);
    for (let step = 0; step < 2000; step++) {
      const { el, wrapper } = render(true);

      play(el, fuzzGesture(random));

      expect(document.body.classList.contains('slider-dragging'), `${step}`).toBe(false);
      expect(el.dataset.dragging, `${step}`).toBeUndefined();
      wrapper.unmount();
    }
  });

  it('ignores a press from any button but the primary one', () => {
    const random = makeRandom(47);
    let secondary = 0;
    for (let step = 0; step < 1000; step++) {
      const { el, reset, wrapper } = render(true);
      el.value = '0.4';
      const gesture = { ...fuzzGesture(random), button: 2, jumped: false };

      play(el, gesture);

      secondary++;
      expect(reset, `${step}`).not.toHaveBeenCalled();
      expect(document.body.classList.contains('slider-dragging'), `${step}`).toBe(false);
      wrapper.unmount();
    }
    expect(secondary).toBeGreaterThan(0);
  });

  it('survives overlapping and abandoned presses without stranding the cursor', () => {
    const random = makeRandom(53);
    const { el, wrapper } = render(true);
    for (let step = 0; step < 2000; step++) {
      const gesture = fuzzGesture(random);
      el.dispatchEvent(pointer('pointerdown', gesture.from.x, gesture.from.y, gesture.button));
      for (const at of gesture.waypoints) window.dispatchEvent(pointer('pointermove', at.x, at.y));
      // A second press before the first was released, or a cancel instead of an up.
      if (random() < 0.3) {
        el.dispatchEvent(pointer('pointerdown', gesture.to.x, gesture.to.y, 0));
      }
      window.dispatchEvent(
        pointer(random() < 0.2 ? 'pointercancel' : 'pointerup', gesture.to.x, gesture.to.y)
      );
      expect(document.body.classList.contains('slider-dragging'), `${step}`).toBe(false);
    }
    wrapper.unmount();
  });

  it('stops answering once unmounted, however the gesture was left', () => {
    const random = makeRandom(59);
    for (let step = 0; step < 1000; step++) {
      const { el, reset, wrapper } = render(true);
      el.value = '0.4';
      const gesture = fuzzGesture(random);
      el.dispatchEvent(pointer('pointerdown', gesture.from.x, gesture.from.y, 0));

      wrapper.unmount();
      window.dispatchEvent(pointer('pointermove', gesture.to.x, gesture.to.y));
      window.dispatchEvent(pointer('pointerup', gesture.to.x, gesture.to.y));

      expect(reset, `${step}`).not.toHaveBeenCalled();
      expect(document.body.classList.contains('slider-dragging'), `${step}`).toBe(false);
    }
  });
});
