import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { mount } from '@vue/test-utils';
import EditWaveform from '@renderer/components/deck/EditWaveform.vue';
import type { TrackData } from '@renderer/stores/decks';

const DURATION_SEC = 240;

function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

const trackData: TrackData = {
  duration: DURATION_SEC,
  sampleRate: 44100,
  bpm: 128,
  silenceEnd: 0,
  coverArt: null
};

// The component batches drags to its own rAF loop, so a gesture commits nothing until a
// frame runs. Frames are driven by hand, or the assertions pass on an empty emit list.
const frameQueue: FrameRequestCallback[] = [];
let nextFrameHandle = 1;
const realRequest = window.requestAnimationFrame;
const realCancel = window.cancelAnimationFrame;

function installFrameControl() {
  frameQueue.length = 0;
  window.requestAnimationFrame = (callback: FrameRequestCallback): number => {
    frameQueue.push(callback);
    return nextFrameHandle++;
  };
  window.cancelAnimationFrame = () => {
    frameQueue.length = 0;
  };
}

function flushFrame() {
  const callback = frameQueue.shift();
  if (callback) callback(performance.now());
}

function rect(width: number): DOMRect {
  return {
    left: 0,
    top: 0,
    right: width,
    bottom: 100,
    width,
    height: 100,
    x: 0,
    y: 0,
    toJSON: () => ''
  };
}

// happy-dom reports every element as zero-sized, and a zero-width canvas is also real
// (the edit view lives behind `v-show`), so both widths are fuzzed: `pxToSec` divides by this.
function stubSize(element: HTMLCanvasElement, width: number) {
  element.getBoundingClientRect = () => rect(width);
  Object.defineProperty(element, 'clientWidth', { value: width, configurable: true });
  Object.defineProperty(element, 'clientHeight', { value: 100, configurable: true });
}

// happy-dom's WheelEvent drops the MouseEvent init fields, so `clientX` has to be
// put back or every wheel gesture reads as NaN and tests nothing.
function wheelAt(clientX: number, deltaY: number, deltaX: number): WheelEvent {
  const event = new WheelEvent('wheel', { deltaY, deltaX, bubbles: true });
  Object.defineProperty(event, 'clientX', { value: clientX, configurable: true });
  return event;
}

function mountWaveform() {
  return mount(EditWaveform, {
    props: {
      accent: '#a855f7',
      trackData,
      loading: false,
      trackBpm: 128,
      beatOffset: 0,
      cuePoint: 0,
      loopRegion: null,
      loopActive: false,
      denseSpectralData: null,
      denseSpectralRate: 0,
      getTrackPosition: () => 12,
      getPlayheadPosition: () => 12,
      getSpectralWaveformRegion: async () => new ArrayBuffer(0)
    },
    global: { mocks: { $t: (key: string) => key } }
  });
}

describe('the edit waveform under fuzzed gestures', () => {
  let wrapper: ReturnType<typeof mountWaveform> | null = null;

  beforeEach(() => {
    installFrameControl();
    wrapper = mountWaveform();
  });

  afterEach(() => {
    wrapper?.unmount();
    wrapper = null;
    window.requestAnimationFrame = realRequest;
    window.cancelAnimationFrame = realCancel;
  });

  function canvas(): HTMLCanvasElement {
    const element = wrapper?.find('canvas').element;
    if (!(element instanceof HTMLCanvasElement)) throw new Error('no canvas');
    return element;
  }

  function seeks(): number[] {
    const emitted = wrapper?.emitted('seek') ?? [];
    return emitted.map((entry) => {
      const value = Array.isArray(entry) ? entry[0] : entry;
      if (typeof value !== 'number') throw new Error(`seek payload was ${typeof value}`);
      return value;
    });
  }

  function gesture(random: () => number, element: HTMLCanvasElement) {
    const x = random() * 900 - 100;
    const roll = random();
    if (roll < 0.4) {
      element.dispatchEvent(
        new MouseEvent('mousedown', { clientX: x, button: random() < 0.5 ? 0 : 2, bubbles: true })
      );
      window.dispatchEvent(new MouseEvent('mousemove', { clientX: random() * 900 - 100 }));
      flushFrame();
      window.dispatchEvent(new MouseEvent('mouseup'));
    } else if (roll < 0.7) {
      element.dispatchEvent(wheelAt(x, (random() * 2 - 1) * 200, (random() * 2 - 1) * 200));
      flushFrame();
    } else {
      element.dispatchEvent(new MouseEvent('mousedown', { clientX: x, button: 0, bubbles: true }));
      flushFrame();
      window.dispatchEvent(new MouseEvent('mouseup'));
    }
  }

  it('commits seeks once frames run', () => {
    const element = canvas();
    stubSize(element, 800);
    const random = makeRandom(3);

    for (let step = 0; step < 400; step++) gesture(random, element);

    expect(seeks().length).toBeGreaterThan(100);
  });

  for (const width of [800, 0]) {
    it(`emits only in-range seeks with a ${width}px canvas`, () => {
      const element = canvas();
      stubSize(element, width);
      const random = makeRandom(width + 1);

      for (let step = 0; step < 1200; step++) gesture(random, element);

      for (const sec of seeks()) {
        expect(Number.isFinite(sec), `width ${width}`).toBe(true);
        expect(sec, `width ${width}`).toBeGreaterThanOrEqual(0);
        expect(sec, `width ${width}`).toBeLessThanOrEqual(DURATION_SEC);
      }
    });
  }

  it('seeks to the pointer, not to the edge of the view', () => {
    const element = canvas();
    stubSize(element, 800);

    element.dispatchEvent(new MouseEvent('mousedown', { clientX: 400, button: 0, bubbles: true }));
    flushFrame();
    window.dispatchEvent(new MouseEvent('mouseup'));

    const committed = seeks();
    expect(committed.length).toBe(1);
    expect(committed[0]).toBeGreaterThan(0);
    expect(committed[0]).toBeLessThan(DURATION_SEC);
  });

  it('holds position instead of emitting NaN when the canvas has collapsed', () => {
    const element = canvas();
    stubSize(element, 0);

    element.dispatchEvent(new MouseEvent('mousedown', { clientX: 0, button: 0, bubbles: true }));
    flushFrame();
    window.dispatchEvent(new MouseEvent('mouseup'));

    for (const sec of seeks()) expect(Number.isNaN(sec)).toBe(false);
  });

  it('never emits a beat offset from a pointer gesture', () => {
    const element = canvas();
    stubSize(element, 800);
    const random = makeRandom(99);

    for (let step = 0; step < 400; step++) gesture(random, element);

    expect(wrapper?.emitted('setBeatOffset')).toBeUndefined();
  });
});
