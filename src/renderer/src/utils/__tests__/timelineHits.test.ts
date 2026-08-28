import { describe, it, expect } from 'vitest';
import { hitPriority, HIT_TARGETS, HIT_PRECEDENCE } from '@renderer/utils/timelineHits';
import type { Hit } from '@renderer/utils/timelineEngine';

const at = (target: string, part?: string): Hit => (part ? { target, part } : { target });
const beats = (winner: Hit, loser: Hit) => hitPriority(winner) > hitPriority(loser);

describe('hitPriority', () => {
  it('ranks the overview above every other target', () => {
    const others = [
      'laneDropdown',
      'filterRegion',
      'clip',
      'waveformSeparator',
      'laneSeparator',
      'lane',
      'clipBand'
    ];
    for (const target of others) {
      expect(beats(at('overview'), at(target)), target).toBe(true);
    }
  });

  it('ranks an element edge above the body of that same element', () => {
    expect(beats(at('filterRegion', 'start'), at('filterRegion', 'body'))).toBe(true);
    expect(beats(at('filterRegion', 'end'), at('filterRegion', 'body'))).toBe(true);
    expect(beats(at('clip', 'start'), at('clip', 'body'))).toBe(true);
    expect(beats(at('clip', 'end'), at('clip', 'body'))).toBe(true);
  });

  it('keeps the lane separator below the elements that overlap it', () => {
    expect(beats(at('filterRegion', 'body'), at('laneSeparator'))).toBe(true);
    expect(beats(at('clip', 'body'), at('laneSeparator'))).toBe(true);
  });

  it('keeps the waveform separator above the clip body it sits on', () => {
    expect(beats(at('waveformSeparator'), at('clip', 'body'))).toBe(true);
  });

  it('leaves the empty clip band at the back', () => {
    const above = [at('lane'), at('laneSeparator'), at('clip', 'body'), at('filterRegion', 'body')];
    for (const hit of above) {
      expect(beats(hit, at('clipBand')), hit.target).toBe(true);
    }
  });

  it('falls back to the target rank when the part is not listed separately', () => {
    expect(hitPriority(at('clipBand', 'anything'))).toBe(hitPriority(at('clipBand')));
    expect(hitPriority(at('overview', 'move'))).toBe(hitPriority(at('overview')));
  });

  it('scores an unknown target zero so it loses to everything ranked', () => {
    expect(hitPriority(at('nonsense'))).toBe(0);
    expect(hitPriority(at('nonsense', 'body'))).toBe(0);
    expect(beats(at('clipBand'), at('nonsense'))).toBe(true);
  });

  it('ranks a separator above the lane dropdown it crosses in the label column', () => {
    expect(beats(at('laneSeparator'), at('laneDropdown'))).toBe(true);
    expect(beats(at('waveformSeparator'), at('laneDropdown'))).toBe(true);
  });

  it('ranks every target the timeline emits', () => {
    for (const target of HIT_TARGETS) {
      const ranked = HIT_PRECEDENCE.some(
        (entry) => entry === target || entry.startsWith(`${target}:`)
      );
      expect(ranked, target).toBe(true);
    }
  });

  it('gives every ranked entry a distinct priority', () => {
    const ranked = [
      at('overview'),
      at('filterRegion', 'start'),
      at('filterRegion', 'end'),
      at('clip', 'start'),
      at('clip', 'end'),
      at('filterRegion', 'body'),
      at('waveformSeparator'),
      at('clip', 'body'),
      at('laneSeparator'),
      at('laneDropdown'),
      at('lane'),
      at('clipBand')
    ];
    const scores = ranked.map(hitPriority);
    expect(new Set(scores).size).toBe(ranked.length);
    expect([...scores].sort((a, b) => b - a)).toEqual(scores);
  });
});
