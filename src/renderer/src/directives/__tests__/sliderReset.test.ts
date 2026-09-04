// @vitest-environment happy-dom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { mount } from '@vue/test-utils';
import { defineComponent, h, withDirectives } from 'vue';
import { vSliderReset } from '@renderer/directives/sliderReset';

// happy-dom drops the MouseEvent init fields from a PointerEvent, so the
// coordinates the directive reads have to be put back by hand.
function pointer(type: string, x: number, y: number): PointerEvent {
  const event = new Event(type, { bubbles: true });
  Object.defineProperties(event, {
    clientX: { value: x },
    clientY: { value: y },
    button: { value: 0 }
  });
  return event as PointerEvent;
}

function render(enabled = true) {
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

function renderPair() {
  const reset = vi.fn();
  const binding = { enabled: true, reset };
  const component = defineComponent({
    render: () =>
      h('div', [
        withDirectives(h('input', { type: 'range', id: 'first' }), [[vSliderReset, binding]]),
        withDirectives(h('input', { type: 'range', id: 'second' }), [[vSliderReset, binding]])
      ])
  });
  const wrapper = mount(component, { attachTo: document.body });
  const first = wrapper.element.querySelector('#first');
  const second = wrapper.element.querySelector('#second');
  if (!(first instanceof HTMLInputElement) || !(second instanceof HTMLInputElement)) {
    throw new Error('the directive needs real inputs');
  }
  return { first, second };
}

// Mounted through a prop so the binding can change without remounting, which is
// what a settings toggle does.
function renderToggleable(enabled: boolean) {
  const reset = vi.fn();
  const component = defineComponent({
    props: { enabled: { type: Boolean, required: true } },
    setup(props) {
      return () =>
        withDirectives(h('input', { type: 'range' }), [
          [vSliderReset, { enabled: props.enabled, reset }]
        ]);
    }
  });
  const wrapper = mount(component, { props: { enabled }, attachTo: document.body });
  const el = wrapper.element;
  if (!(el instanceof HTMLInputElement)) throw new Error('the directive needs a real input');
  return { el, reset, wrapper };
}

function press(el: HTMLInputElement, x: number, y: number) {
  el.dispatchEvent(pointer('pointerdown', x, y));
}

function release(x: number, y: number) {
  window.dispatchEvent(pointer('pointerup', x, y));
}

function move(x: number, y: number) {
  window.dispatchEvent(pointer('pointermove', x, y));
}

describe('the slider reset directive', () => {
  beforeEach(() => {
    document.body.className = '';
  });

  afterEach(() => {
    document.body.innerHTML = '';
    document.body.className = '';
  });

  it('resets when a press leaves the value untouched, which is a press on the thumb', () => {
    const { el, reset } = render();
    el.value = '0.4';
    press(el, 10, 10);
    release(10, 10);
    expect(reset).toHaveBeenCalledTimes(1);
  });

  it('does not reset when the press moved the value, which is a press on the track', () => {
    const { el, reset } = render();
    el.value = '0.4';
    press(el, 10, 10);
    el.value = '0.9';
    release(10, 10);
    expect(reset).not.toHaveBeenCalled();
  });

  it('does not reset a drag, however little the value ended up changing', () => {
    const { el, reset } = render();
    el.value = '0.4';
    press(el, 10, 10);
    move(40, 10);
    release(40, 10);
    expect(reset).not.toHaveBeenCalled();
  });

  it('does not reset a drag that wandered and came back to where it started', () => {
    const { el, reset } = render();
    el.value = '0.4';
    press(el, 10, 10);
    move(60, 10);
    move(11, 10);
    release(11, 10);
    expect(reset).not.toHaveBeenCalled();
  });

  it('leaves the thumb unmarked while the setting is off, so it keeps its own cursor', () => {
    const { el } = render(false);
    expect(el.dataset.clickResets).toBeUndefined();
  });

  it('marks the thumb again when the setting is turned back on', async () => {
    const { el, wrapper } = renderToggleable(false);
    expect(el.dataset.clickResets).toBeUndefined();

    await wrapper.setProps({ enabled: true });
    expect(el.dataset.clickResets).toBe('');

    await wrapper.setProps({ enabled: false });
    expect(el.dataset.clickResets).toBeUndefined();
  });

  it('lets a double-click through while the setting is off, which is how it used to reset', () => {
    const { el } = render(false);
    const dblclick = vi.fn();
    el.addEventListener('dblclick', dblclick);

    el.dispatchEvent(new Event('dblclick', { bubbles: true }));

    expect(dblclick).toHaveBeenCalledTimes(1);
  });

  it('swallows the double-click only while the setting is on', () => {
    const { el } = render(true);
    const dblclick = vi.fn();
    el.addEventListener('dblclick', dblclick);

    el.dispatchEvent(new Event('dblclick', { bubbles: true }));

    expect(dblclick).not.toHaveBeenCalled();
  });

  it('keeps a value the browser set while the setting is off, rather than restoring it', () => {
    const { el } = render(false);
    el.value = '0.4';
    press(el, 10, 10);
    el.value = '0.9';
    release(10, 10);
    expect(el.value).toBe('0.9');
  });

  it('does nothing at all while the setting is off', () => {
    const { el, reset } = render(false);
    el.value = '0.4';
    press(el, 10, 10);
    release(10, 10);
    expect(reset).not.toHaveBeenCalled();
  });

  it('marks the document while dragging, so every cursor is the held one', () => {
    const { el } = render();
    press(el, 10, 10);
    expect(document.body.classList.contains('slider-dragging')).toBe(false);
    move(40, 10);
    expect(document.body.classList.contains('slider-dragging')).toBe(true);
  });

  it('unmarks the document when the gesture ends outside the slider', () => {
    const { el } = render();
    press(el, 10, 10);
    move(400, 400);
    release(400, 400);
    expect(document.body.classList.contains('slider-dragging')).toBe(false);
    expect(el.dataset.dragging).toBeUndefined();
  });

  it('leaves the document unmarked when a press never becomes a drag', () => {
    const { el } = render();
    press(el, 10, 10);
    release(11, 11);
    expect(document.body.classList.contains('slider-dragging')).toBe(false);
  });

  it('marks every slider while one is dragged, so no other thumb offers its own cursor', () => {
    const { first, second } = renderPair();
    press(first, 10, 10);
    move(40, 10);
    expect(first.dataset.dragging).toBe('');
    expect(second.dataset.dragging).toBe('');
  });

  it('unmarks every slider when the drag ends', () => {
    const { first, second } = renderPair();
    press(first, 10, 10);
    move(40, 10);
    release(40, 10);
    expect(first.dataset.dragging).toBeUndefined();
    expect(second.dataset.dragging).toBeUndefined();
  });

  it('unmarks the document when the gesture is cancelled', () => {
    const { el } = render();
    press(el, 10, 10);
    move(40, 10);
    window.dispatchEvent(pointer('pointercancel', 40, 10));
    expect(document.body.classList.contains('slider-dragging')).toBe(false);
  });
});
