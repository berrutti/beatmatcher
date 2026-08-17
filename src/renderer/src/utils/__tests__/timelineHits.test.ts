import { describe, it, expect } from 'vitest';
import { hitPriority } from '@renderer/utils/timelineHits';
import type { Hit } from '@renderer/utils/timelineEngine';

const at = (target: string, part?: string): Hit => (part ? { target, part } : { target });
const beats = (winner: Hit, loser: Hit) => hitPriority(winner) > hitPriority(loser);

describe('hitPriority', () => {
  it('ranks the overview above every other target', () => {
    const others = [
      'laneDropdown',
      'filterRegion',
      'clip',
      'nudgeSpan',
      'waveformSeparator',
      'laneSeparator',
      'lane',
      'clipBand',
      'tickRow'
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
    expect(beats(at('nudgeSpan'), at('laneSeparator'))).toBe(true);
    expect(beats(at('clip', 'body'), at('laneSeparator'))).toBe(true);
  });

  it('keeps the waveform separator above the clip body it sits on', () => {
    expect(beats(at('waveformSeparator'), at('clip', 'body'))).toBe(true);
  });

  it('ranks a nudge above the clip body and the lane beneath it', () => {
    expect(beats(at('nudgeSpan'), at('clip', 'body'))).toBe(true);
    expect(beats(at('nudgeSpan'), at('lane'))).toBe(true);
  });

  it('ranks a clip trim edge above a nudge but its body below one', () => {
    expect(beats(at('clip', 'start'), at('nudgeSpan'))).toBe(true);
    expect(beats(at('clip', 'end'), at('nudgeSpan'))).toBe(true);
    expect(beats(at('nudgeSpan'), at('clip', 'body'))).toBe(true);
  });

  it('ranks a filter region above a nudge by its edges and its body alike', () => {
    expect(beats(at('filterRegion', 'start'), at('nudgeSpan'))).toBe(true);
    expect(beats(at('filterRegion', 'end'), at('nudgeSpan'))).toBe(true);
    expect(beats(at('filterRegion', 'body'), at('nudgeSpan'))).toBe(true);
  });

  it('leaves the ruler at the back', () => {
    for (const target of ['clipBand', 'lane', 'laneSeparator']) {
      expect(beats(at(target), at('tickRow')), target).toBe(true);
    }
  });

  it('falls back to the target rank when the part is not listed separately', () => {
    expect(hitPriority(at('clipBand', 'anything'))).toBe(hitPriority(at('clipBand')));
    expect(hitPriority(at('overview', 'move'))).toBe(hitPriority(at('overview')));
  });

  it('scores an unknown target zero so it loses to everything ranked', () => {
    expect(hitPriority(at('nonsense'))).toBe(0);
    expect(hitPriority(at('nonsense', 'body'))).toBe(0);
    expect(beats(at('tickRow'), at('nonsense'))).toBe(true);
  });

  it('gives every ranked entry a distinct priority', () => {
    const ranked = [
      at('overview'),
      at('laneDropdown'),
      at('filterRegion', 'start'),
      at('filterRegion', 'end'),
      at('clip', 'start'),
      at('clip', 'end'),
      at('filterRegion', 'body'),
      at('nudgeSpan'),
      at('waveformSeparator'),
      at('clip', 'body'),
      at('laneSeparator'),
      at('lane'),
      at('clipBand'),
      at('tickRow')
    ];
    const scores = ranked.map(hitPriority);
    expect(new Set(scores).size).toBe(ranked.length);
    expect([...scores].sort((a, b) => b - a)).toEqual(scores);
  });
});
