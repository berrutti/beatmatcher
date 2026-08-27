// What the user did, kept apart from what the app does about it: items and
// gestures emit these and never touch a store.

import type { ResetExtent } from '@renderer/utils/sessionCore';
import type { ViewWindow } from '@renderer/utils/timelineView';
import type { ClipSelectionRef } from '@renderer/utils/timelineLayout';
import type {
  LanePoint,
  FilterActiveSpan,
  EditableLaneKey,
  TransportBlock
} from '@renderer/utils/types';

// `trackBpm` converts an entered BPM to a rate; `currentBpm` prefills the dialog.
export type BpmContext = {
  ms: number;
  clipStartMs: number;
  clipEndMs: number;
  trackBpm: number;
  currentBpm: number;
};

export type Intent =
  | { type: 'seek'; ms: number }
  | { type: 'view.set'; view: ViewWindow }
  | {
      type: 'lane.openDropdown';
      deck: string;
      lane: EditableLaneKey | null;
      clientX: number;
      clientY: number;
    }
  | { type: 'lane.resize'; deck: string; lane: EditableLaneKey; height: number }
  | { type: 'lane.resizeReset'; deck: string; lane: EditableLaneKey }
  | { type: 'waveform.resize'; deck: string; height: number }
  | { type: 'waveform.resizeReset'; deck: string }
  | {
      type: 'lane.draw';
      deck: string;
      lane: EditableLaneKey;
      samples: LanePoint[];
      t0: number;
      t1: number;
      rateMin: number;
      rateMax: number;
    }
  | { type: 'filter.toggle'; deck: string; t0: number; t1: number }
  | { type: 'clip.move'; block: TransportBlock; deltaMs: number }
  | { type: 'clip.trim'; block: TransportBlock; edge: 'start' | 'end'; newMs: number }
  // A click: the controller resolves it to a span (the BPM region under ms,
  // the iteration of an unlocked loop block, or the whole block).
  | { type: 'clip.select'; block: TransportBlock; ms: number; additive: boolean }
  | { type: 'clip.selectRange'; targets: ClipSelectionRef[]; additive: boolean }
  | { type: 'clip.clearSelection' }
  | { type: 'clip.delete'; ranges: ClipSelectionRef[] }
  | { type: 'clip.split'; block: TransportBlock; ms: number }
  | { type: 'loopBlock.toggleUnlock'; block: TransportBlock; ms: number }
  | { type: 'filterRegion.select'; deck: string; span: FilterActiveSpan }
  | { type: 'filterRegion.clearSelection' }
  | {
      type: 'filterRegion.resize';
      deck: string;
      span: FilterActiveSpan;
      edge: 'start' | 'end';
      newMs: number;
    }
  | { type: 'filterRegion.delete'; deck: string; span: FilterActiveSpan }
  | { type: 'filterRegion.move'; deck: string; span: FilterActiveSpan; deltaMs: number }
  | {
      type: 'menu.deck';
      deck: string;
      clientX: number;
      clientY: number;
      bpm: BpmContext | null;
      split: { block: TransportBlock; ms: number } | null;
      lane: { key: EditableLaneKey; ms: number } | null;
    }
  | {
      type: 'lane.reset';
      deck: string;
      lane: EditableLaneKey;
      ms: number;
      extent: ResetExtent;
      rateMin: number | undefined;
      rateMax: number | undefined;
    }
  | {
      type: 'menu.filterRegion';
      deck: string;
      span: FilterActiveSpan;
      clientX: number;
      clientY: number;
    };

export type IntentHandler = (intent: Intent) => void;
