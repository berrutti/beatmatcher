import { describe, it, expect } from 'vitest';
import { actionShape, laidOut, paramShape, sectionOf } from '../midiControlShape';
import type { ActionSpec } from '../midiMapping';

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

// A player has no mixer section at all, so the headphone cue, the EQ and the channel
// fader are the mixer's and never the deck's.
describe('a row sits in the section that owns the control', () => {
  it('puts the headphone cue with the mixer, not with the transport', () => {
    expect(sectionOf(action({ id: 'cue_toggle' }))).toBe('mixer');
    expect(sectionOf(action({ id: 'transport_cue' }))).toBe('deck');
  });

  it('puts an addressed mixer param with the mixer', () => {
    expect(sectionOf(action({ id: 'deck_param', addressed: true, kind: 'positional' }))).toBe(
      'mixer'
    );
  });

  it('puts anything naming no deck in the global section', () => {
    expect(sectionOf(action({ id: 'enter', needsDeck: false }))).toBe('global');
    expect(sectionOf(action({ id: 'xfader_position', needsDeck: false }))).toBe('global');
  });

  it('leaves the rest with the deck', () => {
    for (const id of ['play_toggle', 'loop_in', 'jog', 'tempo_fader', 'shift', 'eject']) {
      expect(sectionOf(action({ id }))).toBe('deck');
    }
  });
});

describe('a row is shaped like the control it maps', () => {
  it('gives the crossfader a horizontal fader and the tempo fader a vertical one', () => {
    expect(actionShape(action({ id: 'xfader_position', kind: 'positional' }))).toBe(
      'faderHorizontal'
    );
    expect(actionShape(action({ id: 'tempo_fader', kind: 'positional' }))).toBe('faderVertical');
  });

  it('gives the jog a wheel and the browse encoder its detents', () => {
    expect(actionShape(action({ id: 'jog', kind: 'relative' }))).toBe('wheel');
    expect(actionShape(action({ id: 'browse', kind: 'relative' }))).toBe('encoder');
  });

  it('points a stepped action the way it moves the cursor', () => {
    expect(actionShape(action({ id: 'browse', kind: 'button', steps: -1 }))).toBe('stepBack');
    expect(actionShape(action({ id: 'browse', kind: 'button', steps: 1 }))).toBe('stepForward');
  });

  // Every one of these is a button, so the kind tells them apart from nothing. What
  // the action does is the only thing left to draw.
  it('draws each button for what it does, not for being a button', () => {
    const shapes = [
      ['play_toggle', 'play'],
      ['transport_cue', 'cue'],
      ['cue_toggle', 'headphones'],
      ['loop_in', 'loopIn'],
      ['loop_out', 'loopOut'],
      ['loop_exit_or_reloop', 'reloop'],
      ['quantize_toggle', 'quantize'],
      ['shift', 'shift'],
      ['load', 'load'],
      ['eject', 'eject'],
      ['enter', 'enter'],
      ['back', 'back'],
      ['toggle_view', 'view']
    ] as const;
    for (const [id, shape] of shapes) {
      expect(actionShape(action({ id, kind: 'button' }))).toBe(shape);
    }
  });

  it('gives no two button actions the same shape', () => {
    const ids = [
      'play_toggle',
      'transport_cue',
      'cue_toggle',
      'loop_in',
      'loop_out',
      'loop_exit_or_reloop',
      'quantize_toggle',
      'shift',
      'load',
      'eject',
      'enter',
      'back',
      'toggle_view'
    ];
    const drawn = ids.map((id) => actionShape(action({ id, kind: 'button' })));
    expect(new Set(drawn).size).toBe(ids.length);
  });

  it('falls back to the kind for an action it has no shape for', () => {
    expect(actionShape(action({ id: 'brand_new', kind: 'button' }))).toBe('button');
    expect(actionShape(action({ id: 'brand_new', kind: 'positional' }))).toBe('knob');
    expect(actionShape(action({ id: 'brand_new', kind: 'relative' }))).toBe('encoder');
  });

  it('shapes a mixer address from what the control physically is', () => {
    expect(paramShape({ slot: 'eq', param: 'low' })).toBe('knob');
    expect(paramShape({ slot: 'filter', param: 'value' })).toBe('knob');
    expect(paramShape({ slot: 'fader', param: 'gain' })).toBe('faderVertical');
    expect(paramShape({ slot: 'unknown', param: 'thing' })).toBe('knob');
  });
});

describe('the screen orders rows for itself', () => {
  it('puts the browse steps either side of the encoder', () => {
    const rows = laidOut([
      action({ id: 'browse', needsDeck: false, kind: 'relative', steps: null }),
      action({ id: 'browse', needsDeck: false, kind: 'button', steps: -1 }),
      action({ id: 'browse', needsDeck: false, kind: 'button', steps: 1 })
    ]);
    expect(rows.map((row) => row.steps)).toEqual([-1, null, 1]);
  });

  it('orders by its own table, not by the order the vocabulary arrived in', () => {
    const rows = laidOut([
      action({ id: 'shift' }),
      action({ id: 'play_toggle' }),
      action({ id: 'loop_in' })
    ]);
    expect(rows.map((row) => row.id)).toEqual(['play_toggle', 'loop_in', 'shift']);
  });

  it('keeps an action it does not name, after the ones it does', () => {
    const rows = laidOut([action({ id: 'brand_new' }), action({ id: 'play_toggle' })]);
    expect(rows.map((row) => row.id)).toEqual(['play_toggle', 'brand_new']);
  });

  it('leaves the caller its own array', () => {
    const given = [action({ id: 'shift' }), action({ id: 'play_toggle' })];
    laidOut(given);
    expect(given.map((row) => row.id)).toEqual(['shift', 'play_toggle']);
  });
});
