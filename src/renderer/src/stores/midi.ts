import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import { call } from '@renderer/tauriCommands';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { midiConsoleLine, type MidiDevice, type MidiMessage } from '@renderer/utils/midi';
import type {
  ButtonSpec,
  LearnUpdate,
  MappingChoice,
  MappingDraft,
  MappingSlot,
  MidiVocabulary,
  PendingCapture
} from '@renderer/utils/midiMapping';
import type { DeckId } from '@renderer/utils/types';
import { storageGet, storageSet, STORAGE_KEYS } from '@renderer/utils/storage';

export const useMidiStore = defineStore('midi', () => {
  const devices = ref<MidiDevice[]>([]);
  const error = ref<string>('');

  // Keyed by port name, which is all a device tells us about itself.
  const assignments = ref<Record<string, DeckId>>(
    storageGet<Record<string, DeckId>>(STORAGE_KEYS.midiDeckAssignments, {})
  );

  // Which mapping the user gave a device, when the one its port name claimed was not
  // the one they wanted. Keyed by port for the same reason.
  const chosenMappings = ref<Record<string, string>>(
    storageGet<Record<string, string>>(STORAGE_KEYS.midiDeviceMappings, {})
  );

  const mappings = ref<MappingChoice[]>([]);

  // The pending promise rather than the resolved handle, or two overlapping calls both pass
  // a `!unlisten` check before either registers and the first listener doubles every batch.
  let listening: Promise<UnlistenFn> | null = null;

  async function refresh(): Promise<void> {
    error.value = '';
    try {
      mappings.value = await call('midi_mappings');
      devices.value = await call('list_midi_devices');
      await restoreMappings();
      await restoreAssignments();
    } catch (cause) {
      error.value = String(cause);
    }
  }

  // A device claims a mapping by its port name on connect, so a choice that disagrees
  // with the guess has to be reapplied every time it comes back.
  async function restoreMappings(): Promise<void> {
    for (const device of devices.value) {
      const chosen = chosenMappings.value[device.port];
      if (chosen === undefined || chosen === device.mapping) continue;
      await setDeviceMapping(device.port, chosen);
    }
  }

  // Unmapped and with no draft left over, so the panel opens on a blank sheet rather
  // than resuming whatever was last learned for this port.
  async function createMapping(port: string): Promise<void> {
    await setDeviceMapping(port, null);
    await call('discard_midi_draft', { port });
  }

  async function setDeviceMapping(port: string, mapping: string | null): Promise<void> {
    error.value = '';
    try {
      await call('set_midi_device_mapping', { port, mapping });
      const next = { ...chosenMappings.value };
      if (mapping === null) delete next[port];
      else next[port] = mapping;
      chosenMappings.value = next;
      storageSet(STORAGE_KEYS.midiDeviceMappings, chosenMappings.value);
      // The list carries whether the new mapping needs a deck, which nothing here knows.
      devices.value = await call('list_midi_devices');
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
      await call('set_midi_device_deck', { port, deck });
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

  const vocabulary = ref<MidiVocabulary | null>(null);
  const draft = ref<MappingDraft | null>(null);
  // Rust routes an incoming message by the armed slot, so it is read from the draft
  // rather than tracked here, where a failed arm would leave the two disagreeing.
  const armed = computed<MappingSlot | null>(() => draft.value?.armed ?? null);
  // What the armed control reads as while it is still moving. It becomes the
  // draft's own value once the control settles.
  const pending = ref<PendingCapture | null>(null);

  let learnListening: Promise<UnlistenFn> | null = null;

  async function loadVocabulary(): Promise<void> {
    if (vocabulary.value === null) {
      vocabulary.value = await call('midi_vocabulary');
    }
  }

  // Rust emits only for the slot it has armed, and it arms before this side hears
  // about it, so the update names its own slot rather than being matched against one.
  function applyLearn(update: LearnUpdate): void {
    if (draft.value?.port !== update.port) return;
    if (!update.landed) {
      pending.value = { slot: update.slot, capture: update.capture };
      return;
    }
    pending.value = null;
    refreshDraft(update.port).catch((cause) => {
      error.value = String(cause);
    });
  }

  async function startLearn(port: string): Promise<void> {
    error.value = '';
    try {
      await loadVocabulary();
      if (!learnListening) {
        learnListening = listen<LearnUpdate>('midi-learn', (event) => {
          applyLearn(event.payload);
        });
      }
      await learnListening;
      pending.value = null;
      draft.value = await call('start_midi_learn', { port });
    } catch (cause) {
      error.value = String(cause);
    }
  }

  async function refreshDraft(port: string): Promise<void> {
    draft.value = await call('midi_mapping_draft', { port });
  }

  async function stopLearn(): Promise<void> {
    pending.value = null;
    draft.value = null;
    savedPath.value = null;
    await call('stop_midi_learn');
  }

  async function armSlot(port: string, slot: MappingSlot): Promise<void> {
    error.value = '';
    pending.value = null;
    try {
      draft.value = await call('arm_midi_slot', { port, slot });
    } catch (cause) {
      error.value = String(cause);
    }
  }

  async function disarmSlot(port: string): Promise<void> {
    pending.value = null;
    try {
      draft.value = await call('disarm_midi_slot', { port });
    } catch (cause) {
      error.value = String(cause);
    }
  }

  // Where the file last landed, so the panel can say the mapping is on disk.
  const savedPath = ref<string | null>(null);

  // Back to whatever mapping claims the port, or to nothing when none does. The
  // draft outlives the panel, so this is the only way to abandon one.
  async function resetDraft(port: string): Promise<void> {
    error.value = '';
    try {
      await call('discard_midi_draft', { port });
      pending.value = null;
      savedPath.value = null;
      draft.value = await call('start_midi_learn', { port });
    } catch (cause) {
      error.value = String(cause);
    }
  }

  async function saveMapping(port: string): Promise<void> {
    error.value = '';
    try {
      savedPath.value = await call('save_midi_mapping', { port });
    } catch (cause) {
      error.value = String(cause);
    }
  }

  async function setSlotButton(port: string, slot: MappingSlot, button: ButtonSpec): Promise<void> {
    error.value = '';
    try {
      draft.value = await call('set_midi_slot_button', { port, slot, button });
    } catch (cause) {
      error.value = String(cause);
    }
  }

  async function clearSlot(port: string, slot: MappingSlot): Promise<void> {
    error.value = '';
    try {
      draft.value = await call('clear_midi_slot', { port, slot });
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
    await call('set_midi_monitor', { enabled: true });
  }

  refresh();
  startMonitor().catch((cause) => {
    error.value = String(cause);
  });

  return {
    devices,
    error,
    assignments,
    mappings,
    setDeviceMapping,
    createMapping,
    vocabulary,
    draft,
    armed,
    pending,
    savedPath,
    refresh,
    assignDeck,
    receive,
    startMonitor,
    startLearn,
    stopLearn,
    refreshDraft,
    armSlot,
    disarmSlot,
    clearSlot,
    setSlotButton,
    resetDraft,
    saveMapping
  };
});
