export const commands = {
  CUE: 'CUE',
  PLAY: 'PLAY',
  NUDGE_BACK: 'NUDGE_BACK',
  NUDGE_FORWARD: 'NUDGE_FORWARD',
  LOOP_IN: 'LOOP_IN',
  LOOP_OUT_EXIT: 'LOOP_OUT_EXIT'
} as const;

export const KEYS = {
  deckA: {
    [commands.CUE]: 'd',
    [commands.PLAY]: 'f',
    [commands.NUDGE_BACK]: 'e',
    [commands.NUDGE_FORWARD]: 'r',
    [commands.LOOP_IN]: 'c',
    [commands.LOOP_OUT_EXIT]: 'v'
  },
  deckB: {
    [commands.CUE]: 'h',
    [commands.PLAY]: 'j',
    [commands.NUDGE_BACK]: 'y',
    [commands.NUDGE_FORWARD]: 'u',
    [commands.LOOP_IN]: 'n',
    [commands.LOOP_OUT_EXIT]: 'm'
  },
  deckC: {
    [commands.CUE]: 'a',
    [commands.PLAY]: 's',
    [commands.NUDGE_BACK]: 'q',
    [commands.NUDGE_FORWARD]: 'w',
    [commands.LOOP_IN]: 'z',
    [commands.LOOP_OUT_EXIT]: 'x'
  },
  deckD: {
    [commands.CUE]: 'k',
    [commands.PLAY]: 'l',
    [commands.NUDGE_BACK]: 'i',
    [commands.NUDGE_FORWARD]: 'o',
    [commands.LOOP_IN]: ',',
    [commands.LOOP_OUT_EXIT]: '.'
  }
} as const;
