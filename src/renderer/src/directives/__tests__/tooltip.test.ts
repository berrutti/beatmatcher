// @vitest-environment happy-dom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { mount } from '@vue/test-utils';
import { defineComponent, h, withDirectives } from 'vue';
import { vTooltip } from '@renderer/directives/tooltip';
import { useTooltip } from '@renderer/composables/useTooltip';

const { state, hide } = useTooltip();

const SHOW_DELAY_MS = 350;

// happy-dom lays nothing out, so every box reports zero. Each test states only
// the dimensions its case is about.
function setBox(el: Element, axis: 'x' | 'y', client: number, scroll: number) {
  const names = axis === 'x' ? ['clientWidth', 'scrollWidth'] : ['clientHeight', 'scrollHeight'];
  Object.defineProperty(el, names[0], { value: client, configurable: true });
  Object.defineProperty(el, names[1], { value: scroll, configurable: true });
}

function render(modifiers: Record<string, boolean>, value = 'Some Track Name') {
  const component = defineComponent({
    render: () =>
      h('div', [withDirectives(h('span', value), [[vTooltip, value, undefined, modifiers]])])
  });
  const wrapper = mount(component, { attachTo: document.body });
  const span = wrapper.element.querySelector('span');
  if (!(span instanceof HTMLElement)) throw new Error('the directive needs a real element');
  return { span };
}

function hover(el: HTMLElement) {
  el.dispatchEvent(new MouseEvent('mouseenter'));
  vi.advanceTimersByTime(SHOW_DELAY_MS);
}

describe('the tooltip directive', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    hide();
  });

  afterEach(() => {
    vi.useRealTimers();
    document.body.innerHTML = '';
  });

  it('shows a hint that repeats no clipped text, whatever the box measures', () => {
    const { span } = render({});
    setBox(span, 'x', 200, 200);
    hover(span);
    expect(state.visible).toBe(true);
  });

  it('stays hidden when truncated text actually fits', () => {
    const { span } = render({ truncated: true });
    setBox(span, 'x', 200, 200);
    setBox(span, 'y', 18, 18);
    hover(span);
    expect(state.visible).toBe(false);
  });

  it('shows when truncated text overflows its box', () => {
    const { span } = render({ truncated: true });
    setBox(span, 'x', 120, 480);
    setBox(span, 'y', 18, 18);
    hover(span);
    expect(state.visible).toBe(true);
    expect(state.text).toBe('Some Track Name');
  });

  it('shows when a line-clamped box overflows vertically', () => {
    const { span } = render({ truncated: true });
    setBox(span, 'x', 120, 120);
    setBox(span, 'y', 18, 54);
    hover(span);
    expect(state.visible).toBe(true);
  });

  it('does not read a one pixel rounding difference as an overflow', () => {
    const { span } = render({ truncated: true });
    setBox(span, 'x', 200, 201);
    setBox(span, 'y', 18, 19);
    hover(span);
    expect(state.visible).toBe(false);
  });
});
