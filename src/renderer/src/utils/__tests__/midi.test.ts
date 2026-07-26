import { describe, it, expect } from 'vitest';
import { describeMidiMessage } from '../midi';

describe('describeMidiMessage', () => {
  it('reads the channel out of the low nibble, counting from 1', () => {
    expect(describeMidiMessage([0xb0, 7, 64])).toBe('Ch 1  CC 7  64');
    expect(describeMidiMessage([0xb9, 7, 64])).toBe('Ch 10  CC 7  64');
    expect(describeMidiMessage([0x9f, 60, 100])).toBe('Ch 16  Note On 60  100');
  });

  it('joins pitch bend into one 14-bit value, LSB first', () => {
    expect(describeMidiMessage([0xe0, 0, 64])).toBe('Ch 1  Pitch Bend  8192');
    expect(describeMidiMessage([0xe0, 0, 0])).toBe('Ch 1  Pitch Bend  0');
    expect(describeMidiMessage([0xe0, 127, 127])).toBe('Ch 1  Pitch Bend  16383');
  });

  it('names system messages, which carry no channel', () => {
    expect(describeMidiMessage([0xf8])).toBe('Clock');
    expect(describeMidiMessage([0xfe])).toBe('Active Sensing');
    expect(describeMidiMessage([0xf0, 0x7e, 0x00])).toBe('SysEx 7E 00');
  });

  it('does not invent bytes a short message did not carry', () => {
    expect(describeMidiMessage([0xc0, 5])).toBe('Ch 1  Program 5');
    expect(describeMidiMessage([0xb0])).toBe('Ch 1  CC');
    expect(describeMidiMessage([])).toBe('empty');
  });

  it('falls back to raw bytes rather than mislabelling an unknown status', () => {
    expect(describeMidiMessage([0x40, 0x01])).toBe('data 40 01');
    expect(describeMidiMessage([0xf4, 0x01])).toBe('System F4 01');
  });
});
