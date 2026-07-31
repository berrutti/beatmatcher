import { defineStore } from 'pinia';
import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { midiConsoleLine, type MidiMessage } from '@renderer/utils/midi';
import type { DeckId } from '@renderer/utils/types';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';

export type MidiDevice = {
  port: string;
  mapping: string | null;
  assignable: boolean;
  deck: string | null;
};

export const useMidiStore = defineStore('midi', () => {
  const devices = ref<MidiDevice[]>([]);
  const error = ref<string>('');

  // Keyed by port name, which is all a device tells us about itself.
  const assignments = ref<Record<string, DeckId>>(
    storageGet<Record<string, DeckId>>(STORAGE_KEYS.midiDeckAssignments, {})
  );

  // The pending promise rather than the resolved handle, or two overlapping calls both pass
  // a `!unlisten` check before either registers and the first listener doubles every batch.
  let listening: Promise<UnlistenFn> | null = null;

  async function refresh(): Promise<void> {
    error.value = '';
    try {
      devices.value = await invoke<MidiDevice[]>('list_midi_devices');
      await restoreAssignments();
    } catch (cause) {
      error.value = String(cause);
    }
  }

  // A device that was assigned before comes back to the same deck, so unplugging
  // it mid-set is not a reconfiguration.
  async function restoreAssignments(): Promise<void> {
    for (const device of devices.value) {
      if (!device.assignable || device.deck !== null) continue;
      const remembered = assignments.value[device.port];
      if (remembered) await assignDeck(device.port, remembered);
    }
  }

  async function assignDeck(port: string, deck: DeckId | null): Promise<void> {
    error.value = '';
    try {
      await invoke('set_midi_device_deck', { port, deck });
      const next = { ...assignments.value };
      if (deck === null) delete next[port];
      else next[port] = deck;
      assignments.value = next;
      storageSet(STORAGE_KEYS.midiDeckAssignments, assignments.value);
      devices.value = devices.value.map((device) =>
        device.port === port ? { ...device, deck } : device
      );
    } catch (cause) {
      error.value = String(cause);
    }
  }

  function receive(batch: MidiMessage[]): void {
    for (const message of batch) console.log(midiConsoleLine(message.data));
  }

  // Runs for the whole session rather than while the panel is open, so a capture is already
  // in the console when anyone looks. A release build has no devtools, so it buffers nothing.
  async function startMonitor(): Promise<void> {
    if (!import.meta.env.DEV) return;
    if (!listening) {
      listening = listen<MidiMessage[]>('midi-messages', (event) => {
        receive(event.payload);
      });
    }
    await listening;
    await invoke('set_midi_monitor', { enabled: true });
  }

  refresh();
  startMonitor().catch((cause) => {
    error.value = String(cause);
  });

  return {
    devices,
    error,
    assignments,
    refresh,
    assignDeck,
    receive,
    startMonitor
  };
});
