const KEYBOARD_NAV = 'data-keyboard-nav';

// WKWebView stops matching :focus-visible once the last interaction was a click,
// leaving a trapped dialog moving focus with nothing to show for it. Tracked here
// so a ring appears on Tab and never on a click or a dialog's own focus call.
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
