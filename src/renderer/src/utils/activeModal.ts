// The last backdrop is the one on screen, since modals share a z-index and are not
// teleported. Returns whether a modal took the press, so a caller only acts when none did.
function answerModal(action: 'confirm' | 'cancel'): boolean {
  const backdrops = document.querySelectorAll('.modal__backdrop');
  const topmost = backdrops[backdrops.length - 1];
  if (!topmost) return false;
  const button = topmost.querySelector(`.modal__btn--${action}`);
  if (button instanceof HTMLElement) button.click();
  return true;
}

export function confirmModal(): boolean {
  return answerModal('confirm');
}

export function cancelModal(): boolean {
  return answerModal('cancel');
}
