import { describe, it, expect, beforeEach, vi } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';
import { invoke } from '@tauri-apps/api/core';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock('@renderer/stores/settings', () => ({
  LIVE_MIXER_ID: 'classic-4band',
  useSettingsStore: () => ({ filtersEngagedAtStart: false, hydrated: true })
}));

import { useMixerStore, FILTER_ACTIVE, FILTER_VALUE, SEND_AMOUNT } from '../mixer';

describe('channel effects', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it('runs the filter and the send at the same time', () => {
    const store = useMixerStore();
    store.swarmToggleParam('A', FILTER_ACTIVE);
    store.swarmAdjust('A', FILTER_VALUE, 0.4);
    store.swarmAdjust('A', SEND_AMOUNT, 0.7);

    expect(store.paramActive('A', FILTER_ACTIVE)).toBe(true);
    expect(store.paramValue('A', FILTER_VALUE)).toBe(0.4);
    expect(store.paramValue('A', SEND_AMOUNT)).toBe(0.7);
  });

  it('finds the reverb tail the manifest declares', () => {
    const store = useMixerStore();
    expect(store.tailSpec?.slot).toBe('reverb');
  });

  it('starts the resonance at its declared default', () => {
    const store = useMixerStore();
    const spec = store.resonanceSpec;
    expect(store.masterEffectValue(spec)).toBe(spec.defaultValue);
  });

  it('clamps a master effect and sends it to the engine by address', () => {
    const store = useMixerStore();
    const tail = store.tailSpec;
    if (!tail) throw new Error('no reverb tail');

    store.setMasterEffect(tail, 1_000_000);

    expect(store.masterEffectValue(tail)).toBe(tail.max);
    expect(invoke).toHaveBeenCalledWith('set_master_effect', {
      slot: 'reverb',
      param: 'tail',
      value: tail.max
    });
  });
});
