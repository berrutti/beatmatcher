import { describe, it, expect, afterEach } from 'vitest';
import { mount } from '@vue/test-utils';
import { defineComponent, h, withDirectives } from 'vue';
import { vMenuPlacement } from '@renderer/directives/menuPlacement';
import { MENU_VIEWPORT_MARGIN_PX } from '@renderer/utils/menuPlacement';

const originalRect = HTMLElement.prototype.getBoundingClientRect;

// happy-dom lays nothing out, so the size the directive measures is stubbed.
function sizeEveryElement(width: number, height: number): void {
  HTMLElement.prototype.getBoundingClientRect = () =>
    ({ width, height }) as unknown as ReturnType<Element['getBoundingClientRect']>;
}

afterEach(() => {
  HTMLElement.prototype.getBoundingClientRect = originalRect;
});

function render(left: number, top: number): HTMLElement {
  const component = defineComponent({
    render: () =>
      withDirectives(h('div', { style: { left: `${left}px`, top: `${top}px` } }), [
        [vMenuPlacement]
      ])
  });
  const el = mount(component, { attachTo: document.body }).element;
  if (!(el instanceof HTMLElement)) throw new Error('the directive needs a real element');
  return el;
}

describe('vMenuPlacement', () => {
  it('lifts a menu that would open past the bottom of the window', () => {
    sizeEveryElement(200, 300);
    const el = render(100, window.innerHeight - 20);

    expect(parseFloat(el.style.top) + 300).toBe(window.innerHeight - MENU_VIEWPORT_MARGIN_PX);
  });

  it('pulls back a menu that would open past the right edge', () => {
    sizeEveryElement(200, 300);
    const el = render(window.innerWidth - 20, 50);

    expect(parseFloat(el.style.left) + 200).toBe(window.innerWidth - MENU_VIEWPORT_MARGIN_PX);
  });

  it('leaves a menu that already fits alone', () => {
    sizeEveryElement(200, 100);
    const el = render(100, 50);

    expect(el.style.top).toBe('50px');
    expect(el.style.left).toBe('100px');
  });
});
