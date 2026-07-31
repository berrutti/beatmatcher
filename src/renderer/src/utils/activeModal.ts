// The modal is already on screen with its own buttons, so answering it is
// pressing one. Returns whether there was a button, so a caller falls through
// to its own behaviour when there was not.
function answerModal(action: 'confirm' | 'cancel'): boolean {
  const button = document.querySelector(`.modal__btn--${action}`);
  if (!(button instanceof HTMLElement)) return false;
  button.click();
  return true;
}

export function confirmModal(): boolean {
  return answerModal('confirm');
}

export function cancelModal(): boolean {
  return answerModal('cancel');
}
