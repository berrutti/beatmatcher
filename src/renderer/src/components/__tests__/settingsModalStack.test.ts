// @vitest-environment happy-dom
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { mount } from '@vue/test-utils';
import { setActivePinia, createPinia } from 'pinia';
import { nextTick } from 'vue';

// `refresh()` assigns the result straight to `devices`, so a null here re-renders as a crash.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (command: string) => (command === 'list_midi_devices' ? [] : null))
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock('@tauri-apps/plugin-store', () => ({ load: vi.fn().mockResolvedValue({}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));

import { i18n } from '@renderer/i18n';
import Settings from '@renderer/components/Settings.vue';
import { anyModalOpen } from '@renderer/utils/modalStack';

describe('Settings and the global keyboard', () => {
  beforeEach(() => setActivePinia(createPinia()));

  it('holds the keyboard while it is open and releases it when closed', async () => {
    expect(anyModalOpen.value).toBe(false);
    const wrapper = mount(Settings, {
      global: { plugins: [i18n], directives: { tooltip: {} } }
    });
    await nextTick();
    expect(anyModalOpen.value).toBe(true);
    wrapper.unmount();
    await nextTick();
    expect(anyModalOpen.value).toBe(false);
  });
});
