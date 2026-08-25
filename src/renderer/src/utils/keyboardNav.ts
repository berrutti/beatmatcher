const KEYBOARD_NAV = 'data-keyboard-nav';

// WKWebView stops matching :focus-visible once the last interaction was a click, which
// left a trapped dialog moving focus with nothing on screen to show for it. The modality
// is tracked here instead, so a ring appears on Tab and never on a click or on the
// programmatic focus a dialog does when it opens.
export function installKeyboardNav(): () => void {
  const root = document.documentElement;

  function onKeyDown(nativeEvent: KeyboardEvent): void {
    if (nativeEvent.key === 'Tab') root.setAttribute(KEYBOARD_NAV, '');
  }

  function onPointerDown(): void {
    root.removeAttribute(KEYBOARD_NAV);
  }

  document.addEventListener('keydown', onKeyDown, true);
  document.addEventListener('pointerdown', onPointerDown, true);
  return () => {
    document.removeEventListener('keydown', onKeyDown, true);
    document.removeEventListener('pointerdown', onPointerDown, true);
    root.removeAttribute(KEYBOARD_NAV);
  };
}
