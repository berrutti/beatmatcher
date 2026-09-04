// @vitest-environment happy-dom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { mount } from '@vue/test-utils';
import { defineComponent, h, withDirectives } from 'vue';
import { vSliderReset } from '@renderer/directives/sliderReset';
import { thumbCentre, pressIsOnThumb } from '@renderer/utils/sliderThumb';

function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

function pointer(type: string, x: number, y: number): PointerEvent {
  const event = new Event(type, { bubbles: true });
  Object.defineProperties(event, {
    clientX: { value: x },
    clientY: { value: y },
    button: { value: 0 }
  });
  return event as PointerEvent;
}

type Slider = {
  vertical: boolean;
  trackLength: number;
  thumbLength: number;
  grace: number;
  min: number;
  max: number;
};

// happy-dom lays nothing out and resolves no custom properties, so the whole
// geometric path is dark unless both are stubbed. Without this the directive
// falls back to reading the value, which is the branch the plain fuzz covers.
const originalComputed = window.getComputedStyle;
const originalRect = HTMLElement.prototype.getBoundingClientRect;

function stubLayout(slider: Slider) {
  const cross = 30;
  HTMLElement.prototype.getBoundingClientRect = () =>
    ({
      left: 0,
      top: 0,
      width: slider.vertical ? cross : slider.trackLength,
      height: slider.vertical ? slider.trackLength : cross
    }) as unknown as ReturnType<Element['getBoundingClientRect']>;

  window.getComputedStyle = ((el: Element) => {
    if (!(el instanceof HTMLInputElement)) return originalComputed(el);
    return {
      writingMode: slider.vertical ? 'vertical-lr' : 'horizontal-tb',
      getPropertyValue: (name: string) => {
        if (name === '--slider-thumb-length') return `${slider.thumbLength}px`;
        if (name === '--slider-thumb-grace') return slider.grace > 0 ? `${slider.grace}px` : '';
        return '';
      }
    } as unknown as CSSStyleDeclaration;
  }) as typeof window.getComputedStyle;
}

function render(slider: Slider) {
  const reset = vi.fn();
  const component = defineComponent({
    render: () =>
      withDirectives(
        h('input', {
          type: 'range',
          min: String(slider.min),
          max: String(slider.max),
          step: 'any'
        }),
        [[vSliderReset, { enabled: true, reset }]]
      )
  });
  const wrapper = mount(component, { attachTo: document.body });
  const el = wrapper.element;
  if (!(el instanceof HTMLInputElement)) throw new Error('the directive needs a real input');
  Object.defineProperty(el, 'clientWidth', {
    configurable: true,
    get: () => (slider.vertical ? 30 : slider.trackLength)
  });
  Object.defineProperty(el, 'clientHeight', {
    configurable: true,
    get: () => (slider.vertical ? slider.trackLength : 30)
  });
  return { el, reset, wrapper };
}

function fuzzSlider(random: () => number): Slider {
  const vertical = random() < 0.5;
  const thumbLength = 8 + Math.round(random() * 24);
  return {
    vertical,
    trackLength: thumbLength + 20 + Math.round(random() * 300),
    thumbLength,
    grace: random() < 0.4 ? Math.round(random() * 6) : 0,
    min: 0,
    max: 1
  };
}

function geometryFor(slider: Slider, value: number) {
  return {
    min: slider.min,
    max: slider.max,
    value,
    trackLength: slider.trackLength,
    thumbLength: slider.thumbLength,
    maxAtStart: slider.vertical
  };
}

describe('the slider reset directive against fuzzed thumb geometry', () => {
  beforeEach(() => {
    document.body.className = '';
  });

  afterEach(() => {
    document.body.innerHTML = '';
    document.body.className = '';
    window.getComputedStyle = originalComputed;
    HTMLElement.prototype.getBoundingClientRect = originalRect;
  });

  it('resets a press on the thumb even when the browser nudged the value', () => {
    const random = makeRandom(61);
    let cases = 0;
    for (let step = 0; step < 1500; step++) {
      const slider = fuzzSlider(random);
      stubLayout(slider);
      const value = random();
      const { el, reset, wrapper } = render(slider);
      el.value = String(value);

      const along = thumbCentre(geometryFor(slider, value));
      const at = slider.vertical ? { x: 15, y: along } : { x: along, y: 15 };
      el.dispatchEvent(pointer('pointerdown', at.x, at.y));
      // A nudge the press may have caused, which must not veto the reset.
      el.value = String(Math.min(1, value + 0.01));
      window.dispatchEvent(pointer('pointerup', at.x, at.y));

      expect(reset, `${step}`).toHaveBeenCalledTimes(1);
      cases++;
      wrapper.unmount();
    }
    expect(cases).toBeGreaterThan(0);
  });

  it('refuses a press on the track, however close to the thumb it landed', () => {
    const random = makeRandom(67);
    let cases = 0;
    for (let step = 0; step < 1500; step++) {
      const slider = fuzzSlider(random);
      stubLayout(slider);
      const value = random();
      const geometry = geometryFor(slider, value);
      const centre = thumbCentre(geometry);
      const outside = centre + (slider.thumbLength / 2 + slider.grace) * (random() < 0.5 ? 1 : -1);
      const along = outside + (outside > centre ? 1 : -1);
      if (along < 0 || along > slider.trackLength) continue;
      if (pressIsOnThumb(geometry, along, slider.grace)) continue;

      const { el, reset, wrapper } = render(slider);
      el.value = String(value);
      const at = slider.vertical ? { x: 15, y: along } : { x: along, y: 15 };
      el.dispatchEvent(pointer('pointerdown', at.x, at.y));
      el.value = String(random());
      window.dispatchEvent(pointer('pointerup', at.x, at.y));

      expect(reset, `${step}`).not.toHaveBeenCalled();
      cases++;
      wrapper.unmount();
    }
    expect(cases).toBeGreaterThan(0);
  });

  it('gives a bare thumb no reach beyond its own box', () => {
    const random = makeRandom(71);
    let cases = 0;
    for (let step = 0; step < 1500; step++) {
      const slider = { ...fuzzSlider(random), grace: 0 };
      stubLayout(slider);
      const value = random();
      const centre = thumbCentre(geometryFor(slider, value));
      const along = centre + slider.thumbLength / 2 + 1;
      if (along > slider.trackLength) continue;

      const { el, reset, wrapper } = render(slider);
      el.value = String(value);
      const at = slider.vertical ? { x: 15, y: along } : { x: along, y: 15 };
      el.dispatchEvent(pointer('pointerdown', at.x, at.y));
      el.value = String(random());
      window.dispatchEvent(pointer('pointerup', at.x, at.y));

      expect(reset, `${step}`).not.toHaveBeenCalled();
      cases++;
      wrapper.unmount();
    }
    expect(cases).toBeGreaterThan(0);
  });

  it('never resets a drag that started on the thumb', () => {
    const random = makeRandom(73);
    let cases = 0;
    for (let step = 0; step < 1500; step++) {
      const slider = fuzzSlider(random);
      stubLayout(slider);
      const value = random();
      const { el, reset, wrapper } = render(slider);
      el.value = String(value);

      const along = thumbCentre(geometryFor(slider, value));
      const at = slider.vertical ? { x: 15, y: along } : { x: along, y: 15 };
      el.dispatchEvent(pointer('pointerdown', at.x, at.y));
      const away = slider.vertical ? { x: 15, y: along + 40 } : { x: along + 40, y: 15 };
      window.dispatchEvent(pointer('pointermove', away.x, away.y));
      el.value = String(random());
      // Back to where it started, which a release-point check would call a click.
      window.dispatchEvent(pointer('pointerup', at.x, at.y));

      expect(reset, `${step}`).not.toHaveBeenCalled();
      cases++;
      wrapper.unmount();
    }
    expect(cases).toBeGreaterThan(0);
  });
});
