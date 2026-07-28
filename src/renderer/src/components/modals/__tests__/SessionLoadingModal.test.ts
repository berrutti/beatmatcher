import { describe, it, expect } from 'vitest';
import { mount } from '@vue/test-utils';
import { createI18n } from 'vue-i18n';
import SessionLoadingModal from '@renderer/components/modals/SessionLoadingModal.vue';
import type { SessionLoadPhase } from '@renderer/stores/session';
import en from '@renderer/locales/en.json';

const i18n = createI18n({ legacy: false, locale: 'en', messages: { en } });

function mountModal(props: {
  open: boolean;
  fraction: number;
  loadedTracks: number;
  totalTracks: number;
  phase?: SessionLoadPhase;
}) {
  return mount(SessionLoadingModal, {
    props: { phase: 'decoding', ...props },
    global: { plugins: [i18n] }
  });
}

describe('SessionLoadingModal', () => {
  it('renders nothing while closed', () => {
    const wrapper = mountModal({ open: false, fraction: 0, loadedTracks: 0, totalTracks: 0 });
    expect(wrapper.find('.loading-modal').exists()).toBe(false);
  });

  it('offers no way to dismiss itself', () => {
    const wrapper = mountModal({ open: true, fraction: 0.5, loadedTracks: 1, totalTracks: 2 });
    expect(wrapper.findAll('button')).toHaveLength(0);
  });

  it('fills the bar to the loaded fraction', () => {
    const wrapper = mountModal({ open: true, fraction: 0.42, loadedTracks: 2, totalTracks: 5 });
    expect(wrapper.find('.loading-modal__fill').attributes('style')).toContain('width: 42%');
  });

  it('floors the percentage so it never reads 100 while a track is still decoding', () => {
    const wrapper = mountModal({ open: true, fraction: 0.999, loadedTracks: 3, totalTracks: 4 });
    expect(wrapper.find('.loading-modal__percent').text()).toBe('99%');
  });

  it('clamps a fraction outside 0..1 rather than overflowing the bar', () => {
    const over = mountModal({ open: true, fraction: 1.4, loadedTracks: 4, totalTracks: 4 });
    expect(over.find('.loading-modal__percent').text()).toBe('100%');
    const under = mountModal({ open: true, fraction: -0.2, loadedTracks: 0, totalTracks: 4 });
    expect(under.find('.loading-modal__percent').text()).toBe('0%');
  });

  it('reports the track counts it was given', () => {
    const wrapper = mountModal({ open: true, fraction: 0.5, loadedTracks: 2, totalTracks: 4 });
    expect(wrapper.find('.loading-modal__counts').text()).toBe('2 of 4 tracks');
  });

  it('shows no counts before the backend has reported a total', () => {
    const wrapper = mountModal({ open: true, fraction: 0, loadedTracks: 0, totalTracks: 0 });
    expect(wrapper.find('.loading-modal__counts').exists()).toBe(false);
  });

  it('exposes progress to assistive tech', () => {
    const wrapper = mountModal({ open: true, fraction: 0.25, loadedTracks: 1, totalTracks: 4 });
    const bar = wrapper.find('[role="progressbar"]');
    expect(bar.attributes('aria-valuenow')).toBe('25');
    expect(bar.attributes('aria-valuemin')).toBe('0');
    expect(bar.attributes('aria-valuemax')).toBe('100');
  });
  it('names the phase it is in', () => {
    const reading = mountModal({
      open: true,
      phase: 'reading',
      fraction: 0,
      loadedTracks: 0,
      totalTracks: 0
    });
    expect(reading.find('.loading-modal__phase').text()).toBe('Reading session file');
    const parsing = mountModal({
      open: true,
      phase: 'parsing',
      fraction: 0,
      loadedTracks: 0,
      totalTracks: 0
    });
    expect(parsing.find('.loading-modal__phase').text()).toBe('Parsing events');
    const indexing = mountModal({
      open: true,
      phase: 'indexing',
      fraction: 0,
      loadedTracks: 0,
      totalTracks: 0
    });
    expect(indexing.find('.loading-modal__phase').text()).toBe('Building scrub index');
  });

  it('shows an indeterminate bar for the phases that report no increments', () => {
    for (const phase of ['reading', 'parsing', 'indexing'] as const) {
      const wrapper = mountModal({
        open: true,
        phase,
        fraction: 0,
        loadedTracks: 0,
        totalTracks: 4
      });
      expect(wrapper.find('.loading-modal__fill--indeterminate').exists(), phase).toBe(true);
      expect(wrapper.find('.loading-modal__percent').exists(), phase).toBe(false);
      expect(wrapper.find('.loading-modal__counts').exists(), phase).toBe(false);
    }
  });

  it('switches to a measured bar once decoding starts', () => {
    const wrapper = mountModal({
      open: true,
      phase: 'decoding',
      fraction: 0.5,
      loadedTracks: 2,
      totalTracks: 4
    });
    expect(wrapper.find('.loading-modal__fill--indeterminate').exists()).toBe(false);
    expect(wrapper.find('.loading-modal__percent').text()).toBe('50%');
  });
});
