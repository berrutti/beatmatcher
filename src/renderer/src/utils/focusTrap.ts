const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function focusableWithin(root: HTMLElement | null): HTMLElement[] {
  if (!root) return [];
  return Array.from(root.querySelectorAll(FOCUSABLE)).filter(
    (node): node is HTMLElement => node instanceof HTMLElement
  );
}

// Tab must not reach the app behind a modal backdrop: a dialog with no exit would
// otherwise hand focus to the controls the user is being kept away from.
export function trapTabWithin(nativeEvent: KeyboardEvent, root: HTMLElement | null): void {
  const focusable = focusableWithin(root);
  nativeEvent.preventDefault();
  if (focusable.length === 0) {
    root?.focus();
    return;
  }
  // Focus may be outside the trap entirely, e.g. on the body after a backdrop click.
  if (root && document.activeElement instanceof Node && !root.contains(document.activeElement)) {
    focusable[nativeEvent.shiftKey ? focusable.length - 1 : 0].focus();
    return;
  }
  const active = document.activeElement;
  const at = active instanceof HTMLElement ? focusable.indexOf(active) : -1;
  const step = nativeEvent.shiftKey ? -1 : 1;
  // From the container itself, forward means the first control and back means the last.
  const next =
    at === -1
      ? step === 1
        ? 0
        : focusable.length - 1
      : (at + step + focusable.length) % focusable.length;
  focusable[next].focus();
}
