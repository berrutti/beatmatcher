export const commands = {
  CUE: 'CUE',
  PLAY: 'PLAY',
  NUDGE_BACK: 'NUDGE_BACK',
  NUDGE_FORWARD: 'NUDGE_FORWARD',
  LOOP_IN: 'LOOP_IN',
  LOOP_OUT_EXIT: 'LOOP_OUT_EXIT'
} as const;

export type Command = (typeof commands)[keyof typeof commands];
export type DeckBindings = Record<Command, string>;
export type Keybindings = Record<'A' | 'B' | 'C' | 'D', DeckBindings>;

export const CODE_TO_CHAR: Record<string, string> = {
  Period: '.',
  Comma: ',',
  Slash: '/',
  Semicolon: ';',
  Quote: "'",
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  Minus: '-',
  Equal: '=',
  Backquote: '`'
};

export function resolveKey(e: KeyboardEvent): string {
  if (e.code.startsWith('Key')) return e.code.slice(3).toLowerCase();
  return CODE_TO_CHAR[e.code] ?? e.key.toLowerCase();
}

export const DEFAULT_KEYS: Keybindings = {
  A: {
    [commands.CUE]: 'd',
    [commands.PLAY]: 'f',
    [commands.NUDGE_BACK]: 'e',
    [commands.NUDGE_FORWARD]: 'r',
    [commands.LOOP_IN]: 'c',
    [commands.LOOP_OUT_EXIT]: 'v'
  },
  B: {
    [commands.CUE]: 'h',
    [commands.PLAY]: 'j',
    [commands.NUDGE_BACK]: 'y',
    [commands.NUDGE_FORWARD]: 'u',
    [commands.LOOP_IN]: 'n',
    [commands.LOOP_OUT_EXIT]: 'm'
  },
  C: {
    [commands.CUE]: 'a',
    [commands.PLAY]: 's',
    [commands.NUDGE_BACK]: 'q',
    [commands.NUDGE_FORWARD]: 'w',
    [commands.LOOP_IN]: 'z',
    [commands.LOOP_OUT_EXIT]: 'x'
  },
  D: {
    [commands.CUE]: 'k',
    [commands.PLAY]: 'l',
    [commands.NUDGE_BACK]: 'i',
    [commands.NUDGE_FORWARD]: 'o',
    [commands.LOOP_IN]: ',',
    [commands.LOOP_OUT_EXIT]: '.'
  }
};
