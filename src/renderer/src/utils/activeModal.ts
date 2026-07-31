// Only ever one modal at a time, and it is already on screen with its own
// buttons, so the open one is read off the DOM rather than tracked in state.
const BACKDROP = '.modal__backdrop';

function openBackdrop(): Element | null {
  return document.querySelector(BACKDROP);
}

// Returns whether a modal took the press, so a caller falls through to its own
// behaviour only when none is open. A gate the user must wait out renders no
// buttons: it still takes the press rather than letting it reach what is behind.
function answerModal(action: 'confirm' | 'cancel'): boolean {
  const backdrop = openBackdrop();
  if (!backdrop) return false;
  const button = backdrop.querySelector(`.modal__btn--${action}`);
  if (button instanceof HTMLElement) button.click();
  return true;
}

export function confirmModal(): boolean {
  return answerModal('confirm');
}

export function cancelModal(): boolean {
  return answerModal('cancel');
}
