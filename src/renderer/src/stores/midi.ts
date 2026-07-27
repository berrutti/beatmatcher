import { defineStore } from 'pinia';
import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { type MidiMessage } from '@renderer/utils/midi';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';

const MONITOR_LIMIT = 200;

export const useMidiStore = defineStore('midi', () => {
  const inputs = ref<string[]>([]);
  const selectedInput = ref<string | null>(null);
  const messages = ref<MidiMessage[]>([]);
  const error = ref<string>('');

  // The pending promise, not the resolved handle: two overlapping calls would both
  // pass a `!unlisten` check before either finished registering, and the first
  // listener would leak and double every batch.
  let listening: Promise<UnlistenFn> | null = null;

  async function loadInputs(): Promise<void> {
    error.value = '';
    try {
      inputs.value = await invoke<string[]>('list_midi_inputs');
      selectedInput.value = await invoke<string | null>('get_midi_input');
    } catch (cause) {
      error.value = String(cause);
    }
  }

  async function selectInput(port: string | null): Promise<void> {
    error.value = '';
    try {
      await invoke('set_midi_input', { port });
      selectedInput.value = port;
      storageSet(STORAGE_KEYS.midiInput, port ?? '');
    } catch (cause) {
      error.value = String(cause);
      selectedInput.value = null;
    }
  }

  // An absent key means the user has never chosen, which is the only case where
  // guessing is welcome. An empty one means they chose None, and taking the
  // single controller anyway would override that on every launch.
  function preferredPort(): string | null {
    const remembered = storageGet<string | null>(STORAGE_KEYS.midiInput, null);
    if (remembered === null) {
      return inputs.value.length === 1 ? inputs.value[0] : null;
    }
    if (remembered === '') return null;
    return inputs.value.includes(remembered) ? remembered : null;
  }

  async function connectPreferred(): Promise<void> {
    await loadInputs();
    if (selectedInput.value) return;
    const port = preferredPort();
    if (port) await selectInput(port);
  }

  function receive(batch: MidiMessage[]): void {
    const newest = [...batch].reverse();
    messages.value = newest.concat(messages.value).slice(0, MONITOR_LIMIT);
  }

  connectPreferred();

  async function startMonitor(): Promise<void> {
    if (!listening) {
      listening = listen<MidiMessage[]>('midi-messages', (event) => {
        receive(event.payload);
      });
    }
    await listening;
    await invoke('set_midi_monitor', { enabled: true });
  }

  async function stopMonitor(): Promise<void> {
    await invoke('set_midi_monitor', { enabled: false });
    const pending = listening;
    listening = null;
    if (pending) (await pending)();
  }

  function clearMessages(): void {
    messages.value = [];
  }

  return {
    inputs,
    selectedInput,
    messages,
    error,
    loadInputs,
    connectPreferred,
    selectInput,
    receive,
    startMonitor,
    stopMonitor,
    clearMessages
  };
});
