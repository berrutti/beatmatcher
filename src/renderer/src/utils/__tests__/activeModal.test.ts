import { describe, it, expect, vi, afterEach } from 'vitest';
import { confirmModal, cancelModal } from '../activeModal';

function renderModal(): { confirm: () => void; cancel: () => void } {
  const confirm = vi.fn();
  const cancel = vi.fn();
  for (const [action, handler] of [
    ['confirm', confirm],
    ['cancel', cancel]
  ] as const) {
    const button = document.createElement('button');
    button.className = `modal__btn modal__btn--${action}`;
    button.addEventListener('click', handler);
    document.body.appendChild(button);
  }
  return { confirm, cancel };
}

describe('answering the open modal', () => {
  afterEach(() => {
    document.body.innerHTML = '';
  });

  it('presses whichever button the action means', () => {
    const { confirm, cancel } = renderModal();

    expect(confirmModal()).toBe(true);
    expect(confirm).toHaveBeenCalledTimes(1);
    expect(cancel).not.toHaveBeenCalled();

    expect(cancelModal()).toBe(true);
    expect(cancel).toHaveBeenCalledTimes(1);
  });

  it('lets the press through when no modal is on screen', () => {
    expect(confirmModal()).toBe(false);
    expect(cancelModal()).toBe(false);
  });
});
