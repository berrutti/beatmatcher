import type { ActionSpec, ParamAddress } from '@renderer/utils/midiMapping';

export type ControlShape =
  | 'button'
  | 'knob'
  | 'faderVertical'
  | 'faderHorizontal'
  | 'wheel'
  | 'encoder'
  | 'stepBack'
  | 'stepForward'
  | 'play'
  | 'cue'
  | 'headphones'
  | 'loopIn'
  | 'loopOut'
  | 'reloop'
  | 'quantize'
  | 'shift'
  | 'load'
  | 'eject'
  | 'enter'
  | 'back'
  | 'view';

// The engine says whether an action reads a button, a position or a movement. Which
// physical control that looks like is this side's decision, which is why no widget
// kind crosses the boundary. Most of these are buttons, so the kind separates none of
// them and only what the action does is left to draw.
const ACTION_SHAPES: Record<string, ControlShape> = {
  tempo_fader: 'faderVertical',
  xfader_position: 'faderHorizontal',
  jog: 'wheel',
  browse: 'encoder',
  play_toggle: 'play',
  transport_cue: 'cue',
  cue_toggle: 'headphones',
  loop_in: 'loopIn',
  loop_out: 'loopOut',
  loop_exit_or_reloop: 'reloop',
  quantize_toggle: 'quantize',
  shift: 'shift',
  load: 'load',
  eject: 'eject',
  enter: 'enter',
  back: 'back',
  toggle_view: 'view'
};

const PARAM_SHAPES: Record<string, ControlShape> = {
  'eq/low': 'knob',
  'eq/mid': 'knob',
  'eq/high': 'knob',
  'filter/value': 'knob',
  'fader/gain': 'faderVertical'
};

const KIND_SHAPES: Record<ActionSpec['kind'], ControlShape> = {
  button: 'button',
  positional: 'knob',
  relative: 'encoder'
};

export type MappingSection = 'global' | 'deck' | 'mixer';

// The headphone cue lives on the mixer's channel strip, next to that channel's EQ and
// fader. A player has no mixer section at all, so it is never the deck's.
const MIXER_ACTIONS = new Set(['cue_toggle']);

export function sectionOf(action: ActionSpec): MappingSection {
  if (!action.needsDeck) return 'global';
  if (action.addressed || MIXER_ACTIONS.has(action.id)) return 'mixer';
  return 'deck';
}

export function paramShape(address: ParamAddress): ControlShape {
  return PARAM_SHAPES[`${address.slot}/${address.param}`] ?? 'knob';
}

// A stepped action is a press that moves the cursor, so it points the way it moves.
export function actionShape(action: ActionSpec): ControlShape {
  if (action.steps !== null) return action.steps < 0 ? 'stepBack' : 'stepForward';
  return ACTION_SHAPES[action.id] ?? KIND_SHAPES[action.kind];
}

// Left to right the way the controls sit on a surface: step back, the encoder between
// them, step forward. Anything unlisted keeps the order the vocabulary arrived in.
const ACTION_ORDER: string[] = [
  'play_toggle',
  'transport_cue',
  'cue_toggle',
  'loop_in',
  'loop_out',
  'loop_exit_or_reloop',
  'quantize_toggle',
  'shift',
  'jog',
  'tempo_fader',
  'load',
  'eject',
  'xfader_position',
  'back',
  'enter',
  'toggle_view'
];

function rank(action: ActionSpec): number {
  const named = ACTION_ORDER.indexOf(action.id);
  return named === -1 ? ACTION_ORDER.length : named;
}

// `browse` is three rows that belong together and in this order, which its id alone
// cannot express.
function browseRank(action: ActionSpec): number {
  if (action.steps === null) return 1;
  return action.steps < 0 ? 0 : 2;
}

export function laidOut(actions: ActionSpec[]): ActionSpec[] {
  return [...actions].sort((left, right) => {
    if (left.id === right.id) return browseRank(left) - browseRank(right);
    return rank(left) - rank(right);
  });
}
