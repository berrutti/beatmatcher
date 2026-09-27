// @vitest-environment happy-dom
import { describe, it, expect, afterEach } from 'vitest';
import { mount, type VueWrapper } from '@vue/test-utils';
import Knob from '@renderer/components/Knob.vue';
import type { MixerParamSpec } from '@renderer/utils/sessionCore';

const SEND: MixerParamSpec = {
  slot: 'send',
  param: 'amount',
  min: 0,
  max: 1,
  defaultValue: 0,
  step: 0.01,
  unit: 'normalized'
};

function pointer(type: string, x: number, y: number, init: PointerEventInit = {}): PointerEvent {
  const event = new PointerEvent(type, { button: 0, bubbles: true, ...init });
  Object.defineProperty(event, 'clientX', { value: x });
  Object.defineProperty(event, 'clientY', { value: y });
  return event;
}

let wrapper: VueWrapper | null = null;

function knob(value: number, clickResets = false) {
  wrapper = mount(Knob, { props: { spec: SEND, value, clickResets } });
  return wrapper;
}

function drag(element: Element, fromY: number, toY: number, init: PointerEventInit = {}) {
  element.dispatchEvent(pointer('pointerdown', 10, fromY));
  window.dispatchEvent(pointer('pointermove', 10, toY, init));
  window.dispatchEvent(pointer('pointerup', 10, toY));
}

function emitted(name: string): unknown[][] {
  return wrapper?.emitted(name) ?? [];
}

afterEach(() => {
  wrapper?.unmount();
  wrapper = null;
});

describe('knob', () => {
  it('turns up when dragged up and down when dragged down', () => {
    const up = knob(0.5);
    drag(up.element, 100, 60);
    const [[raised]] = emitted('input');
    expect(raised).toBeGreaterThan(0.5);
    up.unmount();

    const down = knob(0.5);
    drag(down.element, 100, 140);
    const [[lowered]] = emitted('input');
    expect(lowered).toBeLessThan(0.5);
  });

  it('moves less for the same drag while shift is held', () => {
    const coarse = knob(0.5);
    drag(coarse.element, 100, 60);
    const [[coarseValue]] = emitted('input');
    coarse.unmount();

    const fine = knob(0.5);
    drag(fine.element, 100, 60, { shiftKey: true });
    const [[fineValue]] = emitted('input');

    expect(Number(fineValue) - 0.5).toBeLessThan(Number(coarseValue) - 0.5);
    expect(Number(fineValue)).toBeGreaterThan(0.5);
  });

  it('stops at the end of its travel however far it is dragged', () => {
    const top = knob(0.9);
    drag(top.element, 500, 0);
    expect(emitted('input').at(-1)?.[0]).toBe(1);
  });

  it('resets on a click when clicks reset', () => {
    const clicked = knob(0.7, true);
    clicked.element.dispatchEvent(pointer('pointerdown', 10, 100));
    window.dispatchEvent(pointer('pointerup', 10, 101));
    expect(emitted('reset')).toHaveLength(1);
    expect(emitted('input')).toHaveLength(0);
  });

  it('does not reset on a click when clicks do not', () => {
    const clicked = knob(0.7, false);
    clicked.element.dispatchEvent(pointer('pointerdown', 10, 100));
    window.dispatchEvent(pointer('pointerup', 10, 100));
    expect(emitted('reset')).toHaveLength(0);
  });

  it('does not reset at the end of a drag', () => {
    const dragged = knob(0.5, true);
    drag(dragged.element, 100, 40);
    expect(emitted('reset')).toHaveLength(0);
  });

  it('resets on a double click when clicks do not', async () => {
    const doubled = knob(0.7, false);
    await doubled.trigger('dblclick');
    expect(emitted('reset')).toHaveLength(1);
  });

  it('turns by wheel', async () => {
    const wheeled = knob(0.5);
    await wheeled.trigger('wheel', { deltaY: -10 });
    const [[value]] = emitted('input');
    expect(value).toBeGreaterThan(0.5);
  });
});
