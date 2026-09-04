// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { mount } from '@vue/test-utils';
import PhaseRing from '@renderer/components/deck/PhaseRing.vue';

const SIZE = 100;

const frameQueue: FrameRequestCallback[] = [];
let nextFrameHandle = 1;
const realRequest = window.requestAnimationFrame;
const realCancel = window.cancelAnimationFrame;
const realGetContext = HTMLCanvasElement.prototype.getContext;

// The ring only reports its phase through the arc it strokes, so the drawing calls
// are recorded and the last arc's end angle is read back as the phase.
const arcs: number[][] = [];

function fakeContext() {
  const noop = () => {};
  return {
    scale: noop,
    clearRect: noop,
    beginPath: noop,
    stroke: noop,
    fill: noop,
    arc: (...args: number[]) => {
      arcs.push(args);
    },
    strokeStyle: '',
    fillStyle: '',
    lineWidth: 0
  };
}

function installStubs() {
  arcs.length = 0;
  frameQueue.length = 0;
  window.requestAnimationFrame = (callback: FrameRequestCallback): number => {
    frameQueue.push(callback);
    return nextFrameHandle++;
  };
  window.cancelAnimationFrame = () => {
    frameQueue.length = 0;
  };
  Object.defineProperty(HTMLCanvasElement.prototype, 'getContext', {
    value: fakeContext,
    configurable: true,
    writable: true
  });
}

function flushFrame() {
  const callback = frameQueue.shift();
  if (callback) callback(performance.now());
}

// The progress arc is the second of the three arcs drawn per frame. Its end angle
// carries the phase.
function lastPhaseAngle(): number {
  const progress = arcs[arcs.length - 2];
  return progress[4];
}

function ring(options: { playing: boolean; cueing: boolean; beat: () => number | null }) {
  const wrapper = mount(PhaseRing, {
    props: {
      accent: '#f00',
      active: true,
      playing: options.playing,
      cueing: options.cueing,
      getBeat: options.beat,
      coverArt: null
    },
    attachTo: document.body
  });
  const canvas = wrapper.find('canvas').element;
  Object.defineProperty(canvas, 'clientWidth', { value: SIZE, configurable: true });
  Object.defineProperty(canvas, 'clientHeight', { value: SIZE, configurable: true });
  return wrapper;
}

describe('PhaseRing', () => {
  beforeEach(() => {
    installStubs();
  });

  afterEach(() => {
    window.requestAnimationFrame = realRequest;
    window.cancelAnimationFrame = realCancel;
    Object.defineProperty(HTMLCanvasElement.prototype, 'getContext', {
      value: realGetContext,
      configurable: true,
      writable: true
    });
  });

  it('advances while the deck is cue-previewing', () => {
    let beat = 0;
    const wrapper = ring({ playing: false, cueing: true, beat: () => beat });
    flushFrame();
    const before = lastPhaseAngle();
    beat = 1;
    flushFrame();

    expect(lastPhaseAngle()).not.toBe(before);
    wrapper.unmount();
  });

  it('holds where it stopped while the deck is paused', () => {
    let beat = 0;
    const wrapper = ring({ playing: false, cueing: false, beat: () => beat });
    flushFrame();
    const before = lastPhaseAngle();
    beat = 1;
    flushFrame();

    expect(lastPhaseAngle()).toBe(before);
    wrapper.unmount();
  });

  it('advances while the deck is playing', () => {
    let beat = 0;
    const wrapper = ring({ playing: true, cueing: false, beat: () => beat });
    flushFrame();
    const before = lastPhaseAngle();
    beat = 1;
    flushFrame();

    expect(lastPhaseAngle()).not.toBe(before);
    wrapper.unmount();
  });

  it('follows the playhead one last time when playback stops', async () => {
    let beat = 2.5;
    const wrapper = ring({ playing: true, cueing: false, beat: () => beat });
    flushFrame();
    const whilePlaying = lastPhaseAngle();

    // Stopping returns the playhead to the cue point in the same update.
    await wrapper.setProps({ playing: false });
    beat = 0;
    flushFrame();

    expect(lastPhaseAngle()).not.toBe(whilePlaying);
    expect(lastPhaseAngle()).toBe(-Math.PI / 2);
    wrapper.unmount();
  });

  it('follows the playhead one last time when a cue preview ends', async () => {
    let beat = 1.75;
    const wrapper = ring({ playing: false, cueing: true, beat: () => beat });
    flushFrame();
    const whileCueing = lastPhaseAngle();

    await wrapper.setProps({ cueing: false });
    beat = 0;
    flushFrame();

    expect(lastPhaseAngle()).not.toBe(whileCueing);
    expect(lastPhaseAngle()).toBe(-Math.PI / 2);
    wrapper.unmount();
  });

  it('holds from the frame after it stopped, so a scrub cannot whip it round', async () => {
    let beat = 2.5;
    const wrapper = ring({ playing: true, cueing: false, beat: () => beat });
    flushFrame();

    await wrapper.setProps({ playing: false });
    beat = 0;
    flushFrame();
    const settled = lastPhaseAngle();

    for (const scrubbed of [1, 2, 3]) {
      beat = scrubbed;
      flushFrame();
      expect(lastPhaseAngle()).toBe(settled);
    }
    wrapper.unmount();
  });
});
