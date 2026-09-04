// @vitest-environment happy-dom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { mount } from '@vue/test-utils';
import { setActivePinia, createPinia } from 'pinia';
import Buttons from '@renderer/components/collection/Buttons.vue';
import { i18n } from '@renderer/i18n';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue({}) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock('@tauri-apps/plugin-store', () => ({ load: vi.fn().mockResolvedValue({}) }));

describe('the per-deck load buttons', () => {
  let seen: EventListener;
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => window.removeEventListener('bm:collection-drop', seen));

  it('offers a track the same way a drag does', async () => {
    const detail = vi.fn();
    seen = (event) => detail((event as CustomEvent).detail);
    window.addEventListener('bm:collection-drop', seen);

    const wrapper = mount(Buttons, {
      props: { path: '/music/a.mp3', disabled: false, unavailableTooltip: '' },
      global: { plugins: [i18n], directives: { tooltip: () => {} } }
    });
    const button = wrapper.findAll('button').at(0);
    await button?.trigger('click');

    expect(detail).toHaveBeenCalled();
    expect(typeof detail.mock.calls[0][0].accept).toBe('function');
    wrapper.unmount();
  });
});
