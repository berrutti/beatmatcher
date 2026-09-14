import { describe, it, expect } from 'vitest';
import {
  actionFor,
  actionSlot,
  bindingsBySlot,
  captureLabel,
  collidingKeys,
  mappingFileText,
  paramSlot,
  slotKey,
  stepsSuffix,
  type ActionSpec,
  type Capture,
  type MappingDraft,
  type MappingSlot
} from '../midiMapping';

function action(over: Partial<ActionSpec> = {}): ActionSpec {
  return {
    id: 'play_toggle',
    needsDeck: true,
    addressed: false,
    steps: null,
    kind: 'button',
    needsRelease: false,
    ...over
  };
}

function note(over: Partial<Capture> = {}): Capture {
  return { channel: 1, note: 11, cc: null, resolution: null, button: 'momentary', ...over };
}

function draft(
  bindings: { slot: MappingSlot; captures: Capture[] }[],
  colliding: MappingSlot[] = []
) {
  const built: MappingDraft = {
    port: 'port',
    name: 'Device',
    matches: ['Device'],
    bindings,
    colliding,
    armed: null,
    file: { version: 2 }
  };
  return built;
}

const identity = (name: string): string => name;

describe('a slot addresses one binding', () => {
  it('carries the deck only for an action that needs one', () => {
    expect(actionSlot(action(), 'A').deck).toBe('A');
    expect(actionSlot(action({ id: 'enter', needsDeck: false }), 'A').deck).toBeNull();
  });

  it('keeps the action steps, so browse either way is two slots', () => {
    const forward = actionSlot(action({ id: 'browse', needsDeck: false, steps: 1 }), null);
    const back = actionSlot(action({ id: 'browse', needsDeck: false, steps: -1 }), null);
    expect(slotKey(forward)).not.toBe(slotKey(back));
  });

  it('separates two decks of one action and two params of one slot', () => {
    expect(slotKey(actionSlot(action(), 'A'))).not.toBe(slotKey(actionSlot(action(), 'B')));
    expect(slotKey(paramSlot('A', { slot: 'eq', param: 'low' }))).not.toBe(
      slotKey(paramSlot('A', { slot: 'eq', param: 'mid' }))
    );
  });
});

describe('a draft reads back by slot', () => {
  it('finds a binding under the slot it was written for', () => {
    const slot = actionSlot(action(), 'A');
    const found = bindingsBySlot(draft([{ slot, captures: [note()] }]));
    expect(found.get(slotKey(slot))?.[0]?.note).toBe(11);
    expect(found.has(slotKey(actionSlot(action(), 'B')))).toBe(false);
  });

  it('is empty before a draft exists', () => {
    expect(bindingsBySlot(null).size).toBe(0);
    expect(collidingKeys(null).size).toBe(0);
    expect(mappingFileText(null)).toBe('');
  });

  it('reports every colliding slot by the same key the grid draws with', () => {
    const play = actionSlot(action(), 'A');
    const cue = actionSlot(action({ id: 'cue_toggle' }), 'A');
    const keys = collidingKeys(
      draft(
        [
          { slot: play, captures: [note()] },
          { slot: cue, captures: [note()] }
        ],
        [play, cue]
      )
    );
    expect(keys).toEqual(new Set([slotKey(play), slotKey(cue)]));
  });
});

describe('an armed slot finds the action it was armed for', () => {
  const browse = action({ id: 'browse', needsDeck: false, steps: 1, kind: 'button' });
  const encoder = action({ id: 'browse', needsDeck: false, steps: null, kind: 'relative' });
  const vocabulary = { actions: [browse, encoder], deckParams: [] };

  it('tells two addressings of one action apart', () => {
    expect(actionFor(vocabulary, actionSlot(browse, null))?.kind).toBe('button');
    expect(actionFor(vocabulary, actionSlot(encoder, null))?.kind).toBe('relative');
  });

  it('is null with nothing armed and with an action the vocabulary lacks', () => {
    expect(actionFor(vocabulary, null)).toBeNull();
    expect(actionFor(vocabulary, actionSlot(action({ id: 'jog' }), 'A'))).toBeNull();
    expect(actionFor(null, actionSlot(browse, null))).toBeNull();
  });
});

describe('a capture reads as the file spells it', () => {
  it('names a note without a resolution', () => {
    expect(captureLabel(note({ channel: 7, note: 65 }), identity)).toBe('ch 7 · note 65');
  });

  it('names a control change with how its value is read', () => {
    expect(
      captureLabel({ channel: 1, note: null, cc: 7, resolution: '14bit', button: null }, identity)
    ).toBe('ch 1 · cc 7 · 14bit');
  });

  it('signs the step distance so two browse rows differ on screen', () => {
    expect(stepsSuffix(null)).toBe('');
    expect(stepsSuffix(1)).toBe(' +1');
    expect(stepsSuffix(-1)).toBe(' -1');
  });
});
