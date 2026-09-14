type ControlKind = 'button' | 'positional' | 'relative';

export type ButtonSpec = 'momentary' | 'trigger';

export type ActionSpec = {
  id: string;
  needsDeck: boolean;
  addressed: boolean;
  steps: number | null;
  kind: ControlKind;
  // The action undoes itself on the release, so a trigger cannot drive it.
  needsRelease: boolean;
};

export type ParamAddress = { slot: string; param: string };

/// One mapping a device can be given, as the picker lists it.
export type MappingChoice = { name: string; needsDeck: boolean };

export type MidiVocabulary = { actions: ActionSpec[]; deckParams: ParamAddress[] };

type ResolutionSpec = '7bit' | '14bit' | 'centre_delta' | 'signed_step';

export type MappingSlot = {
  action: string;
  deck: string | null;
  slot: string | null;
  param: string | null;
  steps: number | null;
};

export type Capture = {
  channel: number;
  note: number | null;
  cc: number | null;
  resolution: ResolutionSpec | null;
  button: ButtonSpec | null;
};

type DraftBinding = { slot: MappingSlot; captures: Capture[] };

export type MappingDraft = {
  port: string;
  name: string;
  matches: string[];
  bindings: DraftBinding[];
  // Slots sharing one address. A profile built from them is refused, so nothing
  // that collides is live on the controller.
  colliding: MappingSlot[];
  armed: MappingSlot | null;
  file: unknown;
};

/// What the armed control currently reads as, carried with the slot Rust read it
/// for, because only Rust knows which slot is armed at the moment a message lands.
export type PendingCapture = { slot: MappingSlot; capture: Capture };

export type LearnUpdate = {
  port: string;
  slot: MappingSlot;
  capture: Capture;
  landed: boolean;
};

export function actionSlot(action: ActionSpec, deck: string | null): MappingSlot {
  return {
    action: action.id,
    deck: action.needsDeck ? deck : null,
    slot: null,
    param: null,
    steps: action.steps
  };
}

export function paramSlot(deck: string | null, address: ParamAddress): MappingSlot {
  return { action: 'deck_param', deck, slot: address.slot, param: address.param, steps: null };
}

export function slotKey(slot: MappingSlot): string {
  return [slot.action, slot.deck, slot.slot, slot.param, slot.steps].join('/');
}

export function bindingsBySlot(draft: MappingDraft | null): Map<string, Capture[]> {
  const found = new Map<string, Capture[]>();
  for (const binding of draft?.bindings ?? []) found.set(slotKey(binding.slot), binding.captures);
  return found;
}

// Reads back the way the file writes it: the channel counted from 1, then the
// address, then how the value is read.
export function captureLabel(capture: Capture, resolutionLabel: (name: string) => string): string {
  const channel = `ch ${capture.channel}`;
  if (capture.note !== null) return `${channel} · note ${capture.note}`;
  const resolution = capture.resolution === null ? '' : ` · ${resolutionLabel(capture.resolution)}`;
  return `${channel} · cc ${capture.cc}${resolution}`;
}

// A modifier can give one action a second address, and both belong on its row.
export function capturesLabel(
  captures: Capture[],
  resolutionLabel: (name: string) => string
): string {
  return captures.map((capture) => captureLabel(capture, resolutionLabel)).join('  +  ');
}

// Two actions differing only by how far they step read as one row without it.
export function stepsSuffix(steps: number | null): string {
  if (steps === null) return '';
  return steps < 0 ? ` ${steps}` : ` +${steps}`;
}

export function actionFor(
  vocabulary: MidiVocabulary | null,
  slot: MappingSlot | null
): ActionSpec | null {
  if (slot === null) return null;
  const found = (vocabulary?.actions ?? []).find(
    (action) => action.id === slot.action && action.steps === slot.steps
  );
  return found ?? null;
}

export const BUTTON_SPECS: ButtonSpec[] = ['momentary', 'trigger'];

// A button whose release the action needs, bound to one that never sends it, leaves
// the deck holding the press.
export function unreleasable(action: ActionSpec | null, captures: Capture[] | undefined): boolean {
  if (action === null || !action.needsRelease) return false;
  return (captures ?? []).some((capture) => capture.button === 'trigger');
}

export function collidingKeys(draft: MappingDraft | null): Set<string> {
  return new Set((draft?.colliding ?? []).map(slotKey));
}

export function mappingFileText(draft: MappingDraft | null): string {
  return draft === null ? '' : `${JSON.stringify(draft.file, null, 2)}\n`;
}
