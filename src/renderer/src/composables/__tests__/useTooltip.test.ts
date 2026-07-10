import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { useTooltip } from '../useTooltip';

describe('useTooltip', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('does not hide a visible tooltip when an unrelated element asks to hide', () => {
    const { scheduleShow, hide, state } = useTooltip();
    const elA = document.createElement('div');
    const elB = document.createElement('div');

    scheduleShow('Tooltip A', elA);
    vi.runAllTimers();
    expect(state.visible).toBe(true);

    // elB is a different element unrelated to the currently shown tooltip
    // (e.g. it just unmounted for an unrelated reason).
    hide(elB);
    expect(state.visible).toBe(true);

    hide(elA);
    expect(state.visible).toBe(false);
  });
});
