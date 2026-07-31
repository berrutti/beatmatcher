import { describe, it, expect } from 'vitest';
import { formatMs, dateStamp } from '@renderer/utils/time';

describe('dateStamp', () => {
  it('is day-month-year, zero padded', () => {
    expect(dateStamp(new Date(2026, 6, 5))).toBe('05-07-2026');
  });

  it('pads neither the year nor a two-digit day', () => {
    expect(dateStamp(new Date(2026, 11, 25))).toBe('25-12-2026');
  });
});

describe('formatMs', () => {
  it('formats under an hour as m:ss', () => {
    expect(formatMs(0)).toBe('0:00');
    expect(formatMs(1_000)).toBe('0:01');
    expect(formatMs(61_000)).toBe('1:01');
    expect(formatMs(599_000)).toBe('9:59');
  });

  it('formats an hour and over as h:mm:ss', () => {
    expect(formatMs(3_600_000)).toBe('1:00:00');
    expect(formatMs(3_661_000)).toBe('1:01:01');
    expect(formatMs(36_000_000)).toBe('10:00:00');
  });

  it('truncates rather than rounds, so a label never shows a second early', () => {
    expect(formatMs(1_999)).toBe('0:01');
    expect(formatMs(3_599_999)).toBe('59:59');
  });

  it('pads minutes only once an hour is shown', () => {
    expect(formatMs(300_000)).toBe('5:00');
    expect(formatMs(3_900_000)).toBe('1:05:00');
  });
});
