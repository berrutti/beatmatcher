import { describe, it, expect, vi, afterEach } from 'vitest';
import { confirmModal, cancelModal } from '../activeModal';

type Spies = { confirm: () => void; cancel: () => void };

// Mirrors Modal.vue: the buttons live inside the backdrop, and a modal that is not
// dismissable renders no actions at all.
function openModal({ dismissable = true } = {}): Spies {
  const confirm = vi.fn();
  const cancel = vi.fn();
  const backdrop = document.createElement('div');
  backdrop.className = 'modal__backdrop';
  if (dismissable) {
    for (const [action, handler] of [
      ['confirm', confirm],
      ['cancel', cancel]
    ] as const) {
      const button = document.createElement('button');
      button.className = `modal__btn modal__btn--${action}`;
      button.addEventListener('click', handler);
      backdrop.appendChild(button);
    }
  }
  document.body.appendChild(backdrop);
  return { confirm, cancel };
}

describe('answering the open modal', () => {
  afterEach(() => {
    document.body.innerHTML = '';
  });

  it('presses whichever button the action means', () => {
    const { confirm, cancel } = openModal();

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

  it('answers the modal on top, not the one behind it', () => {
    const behind = openModal();
    const front = openModal();

    expect(confirmModal()).toBe(true);
    expect(front.confirm).toHaveBeenCalledTimes(1);
    expect(behind.confirm).not.toHaveBeenCalled();
  });

  it('swallows the press when the modal on top has no buttons', () => {
    const behind = openModal();
    openModal({ dismissable: false });

    expect(confirmModal()).toBe(true);
    expect(cancelModal()).toBe(true);
    expect(behind.confirm).not.toHaveBeenCalled();
    expect(behind.cancel).not.toHaveBeenCalled();
  });
});
