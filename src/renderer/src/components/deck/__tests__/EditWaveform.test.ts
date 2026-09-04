// @vitest-environment happy-dom
import { describe, it, expect } from 'vitest';
import { mount } from '@vue/test-utils';
import EditWaveform from '@renderer/components/deck/EditWaveform.vue';
import type { WaveformStyleOption } from '@renderer/utils/types';
import type { TrackData } from '@renderer/stores/decks';

const DENSE_RATE = 150;
const TRACK_SEC = 4;

function mountWaveform(trackData: TrackData | null, densePoints: number | null) {
  return mount(EditWaveform, {
    props: {
      accent: '#a855f7',
      trackData,
      loading: trackData === null,
      trackBpm: 128,
      beatOffset: 0,
      cuePoint: 0,
      loopRegion: null,
      loopActive: false,
      denseSpectralData: densePoints === null ? null : new Float32Array(densePoints * 4),
      denseSpectralRate: densePoints === null ? 0 : DENSE_RATE,
      densePointsReady: 0,
      bandsReady: false,
      bandBalance: [1, 1, 1] as [number, number, number],
      waveformStyle: 'blended' as WaveformStyleOption,
      getTrackPosition: () => 0,
      getPlayheadPosition: () => 0,
      getSpectralWaveformRegion: async () => new ArrayBuffer(0)
    },
    global: { mocks: { $t: (key: string) => key } }
  });
}

const zoomLabel = (wrapper: ReturnType<typeof mountWaveform>) =>
  wrapper.find('.waveform__zoom-label').text();

describe('the edit view before the decode returns', () => {
  it('frames the view from the points, which know the length a chunk in', () => {
    const wrapper = mountWaveform(null, TRACK_SEC * DENSE_RATE);

    // The whole track is shorter than the default zoom, so the label names the track.
    expect(zoomLabel(wrapper)).toBe(`${TRACK_SEC}s`);
  });

  it('shows the canvas rather than the empty state once points exist', () => {
    const wrapper = mountWaveform(null, TRACK_SEC * DENSE_RATE);
    const content = wrapper.find('.waveform__content');

    expect(content.attributes('style') ?? '').not.toContain('display: none');
  });

  it('says nothing about a length with neither a track nor points', () => {
    expect(zoomLabel(mountWaveform(null, null))).toBe('10s');
  });
});
