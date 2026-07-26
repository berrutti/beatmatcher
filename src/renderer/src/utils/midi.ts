export type MidiMessage = { port: string; timestampUs: number; data: number[] };

const CHANNEL_NAMES: Record<number, string> = {
  0x80: 'Note Off',
  0x90: 'Note On',
  0xa0: 'Aftertouch',
  0xb0: 'CC',
  0xc0: 'Program',
  0xd0: 'Pressure',
  0xe0: 'Pitch Bend'
};

const SYSTEM_NAMES: Record<number, string> = {
  0xf0: 'SysEx',
  0xf1: 'MTC Quarter Frame',
  0xf2: 'Song Position',
  0xf3: 'Song Select',
  0xf6: 'Tune Request',
  0xf7: 'SysEx End',
  0xf8: 'Clock',
  0xfa: 'Start',
  0xfb: 'Continue',
  0xfc: 'Stop',
  0xfe: 'Active Sensing',
  0xff: 'Reset'
};

export function hex(byte: number): string {
  return byte.toString(16).toUpperCase().padStart(2, '0');
}

export function describeMidiMessage(data: number[]): string {
  const status = data[0];
  if (status === undefined) return 'empty';
  if (status < 0x80) return `data ${data.map(hex).join(' ')}`;

  if (status >= 0xf0) {
    const name = SYSTEM_NAMES[status] ?? `System ${hex(status)}`;
    const rest = data.slice(1);
    return rest.length > 0 ? `${name} ${rest.map(hex).join(' ')}` : name;
  }

  const channel = (status & 0x0f) + 1;
  const name = CHANNEL_NAMES[status & 0xf0] ?? `Status ${hex(status)}`;
  const first = data[1];
  const second = data[2];

  // Pitch bend is one 14-bit value split across two bytes, so showing the bytes
  // separately would read as two unrelated controls.
  if ((status & 0xf0) === 0xe0 && first !== undefined && second !== undefined) {
    return `Ch ${channel}  ${name}  ${(second << 7) | first}`;
  }
  if (first !== undefined && second !== undefined) {
    return `Ch ${channel}  ${name} ${first}  ${second}`;
  }
  if (first !== undefined) {
    return `Ch ${channel}  ${name} ${first}`;
  }
  return `Ch ${channel}  ${name}`;
}
