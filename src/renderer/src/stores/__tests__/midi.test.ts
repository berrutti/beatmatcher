import { describe, it, expect, vi, beforeEach } from 'vitest';
import { setActivePinia, createPinia } from 'pinia';

const listed: unknown[] = [];

// The store enumerates devices as soon as it is created, so the mock has to
// answer that with a list rather than null.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (command: string) => (command === 'list_midi_devices' ? listed : null))
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn()
}));

const stored: Record<string, unknown> = {};

vi.mock('@renderer/utils/storage', () => ({
  storageGet: vi.fn((key: string, fallback: unknown) => stored[key] ?? fallback),
  storageSet: vi.fn((key: string, value: unknown) => {
    stored[key] = value;
  }),
  STORAGE_KEYS: { midiDeckAssignments: 'midiDeckAssignments' }
}));

import { useMidiStore } from '../midi';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';

const mockedListen = vi.mocked(listen);
const mockedInvoke = vi.mocked(invoke);

const player = { port: 'XDJ-1000', mapping: 'XDJ-1000', assignable: true, deck: null };
const controller = { port: 'DDJ-FLX6', mapping: 'DDJ-FLX6', assignable: false, deck: null };

describe('the MIDI devices', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    listed.length = 0;
    for (const key of Object.keys(stored)) delete stored[key];
  });

  // Settings mounting twice before the first registration resolves used to leave a
  // leaked listener behind, and every message was counted twice.
  it('registers one listener when two starts overlap', async () => {
    const unlisten = vi.fn();
    mockedListen.mockImplementation(
      () => new Promise((resolve) => setTimeout(() => resolve(unlisten), 0))
    );
    const store = useMidiStore();

    await Promise.all([store.startMonitor(), store.startMonitor()]);

    expect(mockedListen).toHaveBeenCalledTimes(1);
  });

  it('writes every message to the console so a capture can be copied out', () => {
    const store = useMidiStore();
    const logged: string[] = [];
    const spy = vi.spyOn(console, 'log').mockImplementation((line: string) => {
      logged.push(line);
    });

    store.receive([
      { port: 'FLX6', timestampUs: 0, data: [0xb1, 0x21, 0x3f] },
      { port: 'FLX6', timestampUs: 1, data: [0xb1, 0x21, 0x40] }
    ]);
    spy.mockRestore();

    expect(logged).toEqual([
      '[midi] B1 21 3F\tCh 2  CC 33  63',
      '[midi] B1 21 40\tCh 2  CC 33  64'
    ]);
  });

  it('lists every connected device at once', async () => {
    listed.push(controller, player);
    const store = useMidiStore();

    await store.refresh();

    expect(store.devices.map((device) => device.port)).toEqual(['DDJ-FLX6', 'XDJ-1000']);
  });

  it('assigns a deck to a single-deck device and remembers it', async () => {
    listed.push(player);
    const store = useMidiStore();
    await store.refresh();

    await store.assignDeck('XDJ-1000', 'D');

    expect(mockedInvoke).toHaveBeenCalledWith('set_midi_device_deck', {
      port: 'XDJ-1000',
      deck: 'D'
    });
    expect(store.devices[0].deck).toBe('D');
    expect(stored.midiDeckAssignments).toEqual({ 'XDJ-1000': 'D' });
  });

  // Unplugging a player mid-set must not be a reconfiguration.
  it('re-pushes a remembered assignment when the same device comes back', async () => {
    stored.midiDeckAssignments = { 'XDJ-1000': 'C' };
    listed.push(player);
    const store = useMidiStore();

    await store.refresh();

    expect(mockedInvoke).toHaveBeenCalledWith('set_midi_device_deck', {
      port: 'XDJ-1000',
      deck: 'C'
    });
    expect(store.devices[0].deck).toBe('C');
  });

  it('clears an assignment when the same deck is chosen again', async () => {
    listed.push(player);
    const store = useMidiStore();
    await store.refresh();
    await store.assignDeck('XDJ-1000', 'D');

    await store.assignDeck('XDJ-1000', null);

    expect(store.devices[0].deck).toBeNull();
    expect(stored.midiDeckAssignments).toEqual({});
  });
});
