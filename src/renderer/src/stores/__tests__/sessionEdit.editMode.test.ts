import { describe, it, expect, vi, beforeEach } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(null) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock('@tauri-apps/plugin-store', () => ({ load: vi.fn().mockResolvedValue({}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

import { useSessionEditStore } from '../sessionEdit';
import { STORAGE_KEYS, storageGet } from '@renderer/utils/storage';

describe('edit mode is remembered', () => {
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
  });

  it('starts off the first time, since nothing has been stored yet', () => {
    expect(useSessionEditStore().editMode).toBe(false);
  });

  it('stores the choice, so the next session opens the way the last one was left', () => {
    const store = useSessionEditStore();
    store.toggleEditMode();
    expect(storageGet(STORAGE_KEYS.sessionEditMode, false)).toBe(true);

    store.toggleEditMode();
    expect(storageGet(STORAGE_KEYS.sessionEditMode, false)).toBe(false);
  });

  it('opens in edit mode when that is how it was left', () => {
    localStorage.setItem(STORAGE_KEYS.sessionEditMode, 'true');
    expect(useSessionEditStore().editMode).toBe(true);
  });

  it('keeps it on when a session loads, rather than dropping out of edit mode', () => {
    localStorage.setItem(STORAGE_KEYS.sessionEditMode, 'true');
    const store = useSessionEditStore();
    store.reset(null);
    expect(store.editMode).toBe(true);
  });
});
