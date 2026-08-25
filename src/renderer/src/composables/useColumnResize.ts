import { computed, ref } from 'vue';
import {
  useCollectionStore,
  isMetadataField,
  isColumnField,
  type ColumnField,
  type MetadataField,
  type CollectionEntry
} from '@renderer/stores/collection';
import { shareFractions, resizeShareDelta } from '@renderer/utils/columnShares';
import { displayName, formatAddedDate } from '@renderer/utils/trackDisplay';
import { useColumnLabels } from '@renderer/composables/useColumnLabels';

const COLUMN_DRAG_THRESHOLD = 5;
const MIN_COLUMN_PIXELS = 40;
const AUTO_FIT_PADDING = 24;
// String length is a cheap proxy for rendered width. Only the longest
// handful of candidates are worth an actual canvas measurement, so a
// library of hundreds of tracks doesn't run measureText() on all of them
// every time a resizer is double-clicked.
const AUTO_FIT_CANDIDATE_LIMIT = 30;

// bpm/added aren't editable metadata at all, and trackNumber is editable
// metadata that still never needs more room than this - all three are
// pinned rather than joining the resizable text-field system.
const PINNED_FIELDS = ['bpm', 'added', 'trackNumber'] as const;
type PinnedField = (typeof PINNED_FIELDS)[number];

const PINNED_COLUMN_WIDTH: Record<PinnedField, number> = {
  bpm: 55,
  added: 55,
  trackNumber: 60
};

function isPinnedField(field: ColumnField): field is PinnedField {
  return PINNED_FIELDS.some((f) => f === field);
}

function isFieldResizable(field: ColumnField): field is MetadataField {
  return isMetadataField(field) && !isPinnedField(field);
}

// Chrome shared by every table: never resizable, never part of the
// order/visibility column system. The playlist-detail table has two extra
// leading columns (grip/index) on top of this.
export const TABLE_CHROME_WIDTH = {
  status: 60,
  actions: 180,
  remove: 32
};

// Split out from useColumnResize below (rather than returned alongside it) so
// a table can compute its own available width - which needs this value - and
// pass it in as useColumnResize's own `availableWidth` argument, without a
// circular dependency on the composable it's feeding into.
export function usePinnedColumnsWidth() {
  const store = useCollectionStore();
  // Pinned fields can be reordered anywhere among the resizable ones and
  // hidden independently, so how much pinned width is actually in play (on
  // top of each table's own always-present chrome) depends on which of them
  // are currently visible.
  return computed(() =>
    store.orderedVisibleColumns
      .filter(isPinnedField)
      .reduce((sum, field) => sum + PINNED_COLUMN_WIDTH[field], 0)
  );
}

export function metaCellValue(track: CollectionEntry, field: MetadataField): string {
  if (field === 'title') return track.title ?? displayName(track.name);
  return track[field] ?? '-';
}

// bpm/added share the resizable/reorderable column loop with the metadata
// fields but render entirely different cell content, so each still needs its
// own modifier class for column-specific styling (alignment etc).
export function columnCellClass(field: ColumnField): string {
  return isMetadataField(field) ? 'collection__td--meta' : `collection__td--${field}`;
}

export function useColumnResize(availableWidth: () => number) {
  const store = useCollectionStore();
  const { COLUMN_LABELS, getColumnLabel } = useColumnLabels();

  function resizableFieldsInOrder(): MetadataField[] {
    return store.orderedVisibleColumns.filter(isFieldResizable);
  }

  // Only pairs with a field that's genuinely its neighbor on screen right now
  // - skipping over a pinned field to reach the next resizable one further
  // down the (unfiltered) order would resize a pair that isn't actually
  // adjacent, moving a boundary nowhere near the handle being dragged.
  function nextResizableField(field: MetadataField): MetadataField | null {
    const order = store.orderedVisibleColumns;
    const next = order[order.indexOf(field) + 1];
    return next !== undefined && isFieldResizable(next) ? next : null;
  }

  // The last resizable column has nothing to its right to trade width with,
  // so it gets no resize handle at all - it just takes whatever share of the
  // leftover space it's been given, same as every other resizable column,
  // but isn't draggable.
  function isResizableField(field: ColumnField): boolean {
    return isFieldResizable(field) && nextResizableField(field) !== null;
  }

  // `table-layout: fixed` updates a <col>'s width attribute from a `calc()` but
  // does not re-lay-out the table, so shares are resolved to pixels here.
  const resizableShareFractions = computed(() =>
    shareFractions(resizableFieldsInOrder(), store.getColumnShare)
  );

  const metadataWidthsPx = computed((): Record<MetadataField, number> => {
    const result = {} as Record<MetadataField, number>;
    for (const field of resizableFieldsInOrder()) {
      result[field] = (resizableShareFractions.value[field] ?? 0) * availableWidth();
    }
    return result;
  });

  function columnWidth(field: ColumnField): string {
    if (isPinnedField(field)) return `${PINNED_COLUMN_WIDTH[field]}px`;
    return `${metadataWidthsPx.value[field] ?? 0}px`;
  }

  // The header row is found via `closest` at drag start, so this one pair of
  // refs and handlers works for the whole table without a container-specific
  // element ref.
  const draggingColumn = ref<ColumnField | null>(null);
  const dropTargetColumn = ref<ColumnField | null>(null);

  function onColumnHeaderPointerDown(e: PointerEvent, field: ColumnField) {
    if (e.button !== 0) return;
    const headerRow = (e.currentTarget as HTMLElement).closest('tr');
    if (!headerRow) return;
    const startX = e.clientX;
    let active = false;

    // Measured once: a live re-measure sees the just-swapped neighbour back under
    // the motionless cursor and swaps it straight back.
    const headerCells = Array.from(headerRow.querySelectorAll<HTMLElement>('[data-column-field]'));
    const fields = headerCells.map((el) => el.dataset.columnField).filter(isColumnField);
    const slotRects = headerCells.map((el) => el.getBoundingClientRect());

    function slotIndexAt(clientX: number): number {
      for (let i = 0; i < slotRects.length; i++) {
        if (clientX < slotRects[i].left + slotRects[i].width / 2) return i;
      }
      return slotRects.length - 1;
    }

    // The swap itself only happens on drop. While dragging this just tracks
    // what would happen so the drop target can be highlighted live.
    let pendingBefore: ColumnField | null = null;
    let hasPendingSwap = false;

    function onMove(event: PointerEvent) {
      if (!active) {
        if (Math.abs(event.clientX - startX) < COLUMN_DRAG_THRESHOLD) return;
        active = true;
        draggingColumn.value = field;
      }
      const draggedIndex = fields.indexOf(field);
      const slot = slotIndexAt(event.clientX);
      const target = fields[slot];
      if (draggedIndex === -1 || target === undefined || target === field) {
        dropTargetColumn.value = null;
        hasPendingSwap = false;
        return;
      }
      dropTargetColumn.value = target;
      pendingBefore = slot < draggedIndex ? target : (fields[slot + 1] ?? null);
      hasPendingSwap = true;
    }

    function stopListeners() {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
      window.removeEventListener('pointercancel', onCancel);
    }

    function onUp() {
      stopListeners();
      if (active && hasPendingSwap) store.reorderColumn(field, pendingBefore);
      draggingColumn.value = null;
      dropTargetColumn.value = null;
    }

    function onCancel() {
      stopListeners();
      draggingColumn.value = null;
      dropTargetColumn.value = null;
    }

    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    window.addEventListener('pointercancel', onCancel);
  }

  function onResizerPointerDown(e: PointerEvent, field: ColumnField) {
    if (e.button !== 0 || !isFieldResizable(field)) return;
    // TypeScript doesn't carry a control-flow-narrowed parameter's narrowing
    // into a nested function declaration referenced later (as an event
    // listener here) - rebinding to fresh consts right where the narrowing
    // happens gives onMove variables whose declared type is already narrow,
    // instead of relying on narrowing that won't survive the closure.
    const resizedField = field;
    const candidateNeighbor = nextResizableField(resizedField);
    if (!candidateNeighbor) return;
    // Same as resizedField above: the narrowing from `if (!candidateNeighbor)`
    // only holds textually after that check, not inside a closure defined
    // later - rebinding once more, right here, captures a variable whose
    // declared type is already non-null from the start.
    const neighborField = candidateNeighbor;
    let lastX = e.clientX;

    function onMove(event: PointerEvent) {
      const deltaPx = event.clientX - lastX;
      lastX = event.clientX;
      const { field: newFieldShare, neighbor: newNeighborShare } = resizeShareDelta({
        fields: resizableFieldsInOrder(),
        getShare: store.getColumnShare,
        field: resizedField,
        neighbor: neighborField,
        deltaPx,
        availableWidth: availableWidth(),
        minPx: MIN_COLUMN_PIXELS
      });
      store.setColumnShare(resizedField, newFieldShare);
      store.setColumnShare(neighborField, newNeighborShare);
    }

    function stop() {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', stop);
      window.removeEventListener('pointercancel', stop);
    }

    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', stop);
    window.addEventListener('pointercancel', stop);
  }

  // Auto-fit sizes a column to whatever it actually needs to show across the
  // whole collection, not just whichever table's row happened to trigger it -
  // the two tables' widths are shared, and the full collection is the only
  // dataset that's stable regardless of which one you double-clicked in.
  function columnCellValues(field: ColumnField): string[] {
    if (isMetadataField(field)) return store.tracks.map((track) => metaCellValue(track, field));
    if (field === 'bpm') {
      return store.tracks.map((track) => {
        const bpm = store.getBpm(track);
        return bpm !== null ? `${bpm.toFixed(1)} BPM` : '';
      });
    }
    return store.tracks.map((track) => formatAddedDate(track.addedAt));
  }

  let autoFitCanvas: HTMLCanvasElement | null = null;

  // Every other column absorbs the difference proportionally: an auto-fit has no
  // one neighbour it is dragging against.
  function autoFitColumn(field: ColumnField, e: MouseEvent) {
    if (!isFieldResizable(field)) return;
    const th = (e.currentTarget as HTMLElement).closest('th');
    if (!autoFitCanvas) autoFitCanvas = document.createElement('canvas');
    const ctx = autoFitCanvas.getContext('2d');
    if (!ctx) return;
    ctx.font = th ? getComputedStyle(th).font : getComputedStyle(document.body).font;
    // Fit to the widest of the header label and every row's own value, not
    // just the header label - a short header like "Title" fitting the column
    // to itself would shrink it below what most track titles need.
    const candidates = [COLUMN_LABELS.value[field], ...columnCellValues(field)]
      .sort((a, b) => b.length - a.length)
      .slice(0, AUTO_FIT_CANDIDATE_LIMIT);
    const desiredPx = Math.ceil(
      candidates.reduce((max, text) => Math.max(max, ctx.measureText(text).width), 0) +
        AUTO_FIT_PADDING
    );

    const otherShareTotal = resizableFieldsInOrder()
      .filter((f) => f !== field)
      .reduce((sum, f) => sum + store.getColumnShare(f), 0);
    // With no other resizable column to renormalize against, this one already
    // fills 100% of the space regardless of its own share value.
    if (otherShareTotal <= 0) return;
    const width = availableWidth();
    const clampedPx = Math.min(desiredPx, Math.max(MIN_COLUMN_PIXELS, width - MIN_COLUMN_PIXELS));
    const newShare = (clampedPx * otherShareTotal) / Math.max(1, width - clampedPx);
    store.setColumnShare(field, newShare);
  }

  return {
    PINNED_COLUMN_WIDTH,
    isPinnedField,
    isFieldResizable,
    isResizableField,
    resizableFieldsInOrder,
    columnWidth,
    getColumnLabel,
    COLUMN_LABELS,
    draggingColumn,
    dropTargetColumn,
    onColumnHeaderPointerDown,
    onResizerPointerDown,
    autoFitColumn
  };
}
