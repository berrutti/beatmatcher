import {
  ghostLanding,
  grabOffset,
  DROP_LANDING_MS,
  type LandingSpot
} from '@renderer/utils/dragGhostLanding';
import { dropLandingWithin } from '@renderer/utils/dropLanding';
import { offerToDeck } from '@renderer/utils/deckDrop';

// Enough of the collection store for a drag, so a caller passes what it has
// rather than the whole store.
export type DragStore = {
  startDrag: (path: string) => void;
  endDrag: () => void;
};

const DRAG_THRESHOLD = 5;

const DRAG_GHOST_SCALE = 0.6;

type DragGhost = { element: HTMLElement; offsetX: number; offsetY: number };

// Tracked at module level so a ghost orphaned by an earlier drag (e.g. the
// pointerup was lost because the window lost focus) can never pile up: each
// new drag removes any leftover ghost before creating its own.
let currentDragGhost: DragGhost | null = null;

function clearDragGhost() {
  currentDragGhost?.element.remove();
  currentDragGhost = null;
}

// Released from `currentDragGhost` first, so a new drag starting mid-flight is
// not blocked by one still animating.
function landDragGhost(ghost: DragGhost, target: HTMLElement | null, origin: LandingSpot) {
  const { element } = ghost;
  if (currentDragGhost?.element === element) currentDragGhost = null;
  const landing = ghostLanding({
    anchor: { x: ghost.offsetX, y: ghost.offsetY },
    origin,
    target: target ? (dropLandingWithin(target)?.getBoundingClientRect() ?? null) : null
  });
  element.style.transition = `left ${DROP_LANDING_MS}ms ease-in, top ${DROP_LANDING_MS}ms ease-in, transform ${DROP_LANDING_MS}ms ease-in, opacity ${DROP_LANDING_MS}ms ease-in`;
  element.style.left = `${landing.left}px`;
  element.style.top = `${landing.top}px`;
  element.style.transform = `scale(${landing.scale})`;
  element.style.opacity = `${landing.opacity}`;
  window.setTimeout(() => element.remove(), DROP_LANDING_MS);
}

// A detached <tr> loses the table's column model and renders squished, so the
// clone is wrapped in a table carrying the same colgroup. That colgroup comes
// from the original row, since `closest` on a detached clone finds nothing.
function wrapRowClone(
  row: HTMLTableRowElement,
  originalRow: HTMLTableRowElement
): HTMLTableElement {
  const table = document.createElement('table');
  table.style.borderCollapse = 'collapse';
  table.style.tableLayout = 'fixed';
  const colgroup = originalRow.closest('table')?.querySelector('colgroup');
  if (colgroup) table.appendChild(colgroup.cloneNode(true));
  const tbody = document.createElement('tbody');
  tbody.appendChild(row);
  table.appendChild(tbody);
  return table;
}

// transform-origin defaults to the element's own center, so scaling never
// shifts that center: left/top only need the unscaled half-size offset, and
// that offset never changes again for the rest of the drag.
function createDragGhost(source: HTMLElement, clientX: number, clientY: number): DragGhost {
  clearDragGhost();
  const rect = source.getBoundingClientRect();
  const clone = source.cloneNode(true) as HTMLElement;
  clone.querySelectorAll('button').forEach((button) => button.remove());
  const element =
    clone instanceof HTMLTableRowElement && source instanceof HTMLTableRowElement
      ? wrapRowClone(clone, source)
      : clone;
  element.classList.add('collection__drag-ghost');
  element.style.width = `${rect.width}px`;
  element.style.height = `${rect.height}px`;
  const { x: offsetX, y: offsetY } = grabOffset(rect, clientX, clientY);
  // Scale around the held point, or the shrink walks the row out from under
  // the pointer that is holding it.
  element.style.transformOrigin = `${offsetX}px ${offsetY}px`;
  element.style.left = `${clientX - offsetX}px`;
  element.style.top = `${clientY - offsetY}px`;
  document.body.appendChild(element);
  // Only `transform` animates here, never left/top, so the brief shrink-in
  // never delays cursor tracking.
  requestAnimationFrame(() => {
    element.style.transition = 'transform 100ms ease';
    element.style.transform = `scale(${DRAG_GHOST_SCALE})`;
  });
  const ghost: DragGhost = { element, offsetX, offsetY };
  currentDragGhost = ghost;
  return ghost;
}

function moveDragGhost(ghost: DragGhost, clientX: number, clientY: number) {
  ghost.element.style.left = `${clientX - ghost.offsetX}px`;
  ghost.element.style.top = `${clientY - ghost.offsetY}px`;
}

function deckElementAtPoint(clientX: number, clientY: number): HTMLElement | null {
  const under = document.elementFromPoint(clientX, clientY);
  const deckEl = under?.closest('[data-deck-id]');
  return deckEl instanceof HTMLElement ? deckEl : null;
}

export function startTrackDrag(store: DragStore, event: PointerEvent, path: string) {
  if (event.button !== 0) return;
  if (!(event.target instanceof HTMLElement) || event.target.closest('button')) return;
  // A sustained press-and-move is WebKit's own text-selection gesture, which
  // auto-scrolls whatever container the pointer nears. It fires no wheel or
  // scroll event, so stopping the selection before it starts is the only way.
  event.preventDefault();

  const startX = event.clientX;
  const startY = event.clientY;
  const itemEl =
    event.currentTarget instanceof HTMLElement
      ? event.currentTarget
      : event.target instanceof HTMLElement
        ? event.target.closest('tr')
        : null;
  if (!(itemEl instanceof HTMLElement)) return;
  const sourceEl = itemEl;
  let active = false;
  let dragGhost: DragGhost | null = null;
  const originRect = sourceEl.getBoundingClientRect();
  const origin: LandingSpot = { left: originRect.left, top: originRect.top };

  function onMove(event: PointerEvent) {
    if (!active) {
      if (
        Math.abs(event.clientX - startX) < DRAG_THRESHOLD &&
        Math.abs(event.clientY - startY) < DRAG_THRESHOLD
      )
        return;
      active = true;
      store.startDrag(path);
      document.body.style.cursor = 'grabbing';
      // The search input can be left focused from an earlier click. Without
      // blurring it here, keyboard shortcuts typed during/after the drag go
      // into the search box instead of controlling decks.
      const focused = document.activeElement;
      if (focused instanceof HTMLInputElement) focused.blur();
      dragGhost = createDragGhost(sourceEl, event.clientX, event.clientY);
      return;
    }
    if (dragGhost) moveDragGhost(dragGhost, event.clientX, event.clientY);
  }

  function finishDrag(landOn: HTMLElement | null): boolean {
    const wasActive = active;
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
    window.removeEventListener('pointercancel', onCancel);
    window.removeEventListener('blur', onCancel);
    if (dragGhost) landDragGhost(dragGhost, landOn, origin);
    else clearDragGhost();
    dragGhost = null;
    if (wasActive) {
      document.body.style.cursor = '';
      store.endDrag();
    }
    return wasActive;
  }

  function onUp(event: PointerEvent) {
    const deckEl = deckElementAtPoint(event.clientX, event.clientY);
    const deckId = deckEl?.dataset.deckId;
    // Offered before the ghost is released, because whether a deck took it is
    // what decides where the ghost goes.
    const accepted = active && deckId !== undefined && offerToDeck(path, deckId);
    finishDrag(accepted ? deckEl : null);
  }

  function onCancel() {
    finishDrag(null);
  }

  window.addEventListener('pointermove', onMove);
  window.addEventListener('pointerup', onUp);
  window.addEventListener('pointercancel', onCancel);
  // If the window loses focus mid-drag (alt-tab, native dialog), no further
  // pointer events arrive at all, so this is the only way to clean up.
  window.addEventListener('blur', onCancel);
}
