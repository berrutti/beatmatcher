// The semantic vocabulary the timeline emits. Items + gestures never touch the
// stores; they produce one of these intents and the controller reacts (calls an
// edit op, moves the camera, updates a selection). This is the seam between "what
// the user did" and "what the app does about it". The parent-reacts model.

import type { ViewWindow } from '@renderer/utils/timelineView';
import type { ClipSelectionRef } from '@renderer/utils/timelineLayout';
import type {
  LanePoint,
  FilterActiveSpan,
  NudgeSpan,
  EditableLaneKey,
  TransportBlock
} from '@renderer/utils/types';

// Context for the "Set BPM" menu items: the clicked point, the clip span it
// falls in, the track's grid bpm (to convert an entered BPM to a rate), and the
// tempo currently playing there (to prefill the dialog).
export type BpmContext = {
  atMs: number;
  clipStartMs: number;
  clipEndMs: number;
  trackBpm: number;
  currentBpm: number;
};

export type Intent =
  // transport / camera
  | { type: 'seek'; ms: number }
  | { type: 'view.set'; view: ViewWindow }
  // lane chrome
  | { type: 'lane.openDropdown'; deck: string; clientX: number; clientY: number }
  | { type: 'lane.resize'; height: number }
  | { type: 'lane.resizeReset' }
  | { type: 'waveform.resize'; height: number }
  | { type: 'waveform.resizeReset' }
  // automation edits (committed on gesture end)
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
  | { type: 'nudge.paint'; deck: string; t0: number; t1: number; direction: 1 | -1 }
  | { type: 'filter.toggle'; deck: string; t0: number; t1: number }
  // clip block edits
  | { type: 'clip.move'; block: TransportBlock; deltaMs: number }
  | { type: 'clip.trim'; block: TransportBlock; edge: 'start' | 'end'; newMs: number }
  // A click: the controller resolves it to a span (the BPM region under ms,
  // the iteration of an unlocked loop block, or the whole block).
  | { type: 'clip.select'; block: TransportBlock; ms: number; additive: boolean }
  // Explicit spans (marquee, whole-block double-click).
  | { type: 'clip.selectRange'; targets: ClipSelectionRef[]; additive: boolean }
  | { type: 'clip.clearSelection' }
  | { type: 'clip.delete'; ranges: ClipSelectionRef[] }
  | { type: 'loopBlock.toggleUnlock'; block: TransportBlock; ms: number }
  // filter-region edits
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
  // context menu
  | {
      type: 'menu.deck';
      deck: string;
      clientX: number;
      clientY: number;
      nudge: NudgeSpan | null;
      bpm: BpmContext | null;
    }
  | {
      type: 'menu.filterRegion';
      deck: string;
      span: FilterActiveSpan;
      clientX: number;
      clientY: number;
    };

export type IntentHandler = (intent: Intent) => void;
