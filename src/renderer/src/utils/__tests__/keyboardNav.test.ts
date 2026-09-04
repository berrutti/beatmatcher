// @vitest-environment happy-dom
import { describe, it, expect, afterEach } from 'vitest';
import { installKeyboardNav } from '../keyboardNav';

let stop: (() => void) | null = null;

afterEach(() => {
  stop?.();
  stop = null;
});

function marked(): boolean {
  return document.documentElement.hasAttribute('data-keyboard-nav');
}

describe('keyboard navigation modality', () => {
  it('is unmarked until something is actually tabbed', () => {
    stop = installKeyboardNav();
    expect(marked()).toBe(false);
  });

  it('marks on Tab and clears on the next pointer press', () => {
    stop = installKeyboardNav();

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab' }));
    expect(marked()).toBe(true);

    document.dispatchEvent(new Event('pointerdown'));
    expect(marked()).toBe(false);
  });

  it('ignores keys that are not Tab, so typing does not raise rings', () => {
    stop = installKeyboardNav();
    for (const key of ['a', 'Enter', 'Escape', 'ArrowDown', ' ']) {
      document.dispatchEvent(new KeyboardEvent('keydown', { key }));
      expect(marked(), key).toBe(false);
    }
  });

  it('leaves nothing marked once uninstalled', () => {
    const release = installKeyboardNav();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab' }));
    expect(marked()).toBe(true);

    release();
    expect(marked()).toBe(false);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab' }));
    expect(marked()).toBe(false);
  });
});
