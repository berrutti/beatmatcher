// The semantic vocabulary the timeline emits. Items + gestures never touch the
// stores; they produce one of these intents and the controller reacts (calls an
// edit op, moves the camera, updates a selection). This is the seam between "what
// the user did" and "what the app does about it". The parent-reacts model.

import type { ViewWindow } from '@renderer/utils/timelineView';
import type {
  LanePoint,
  FilterActiveSpan,
  NudgeSpan
} from '@renderer/composables/useSessionTimeline';
import type { EditableLaneKey } from '@renderer/utils/sessionEditOps';
import type { TransportBlock } from '@renderer/utils/types';

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
  | { type: 'clip.select'; block: TransportBlock; ms: number }
  | { type: 'clip.clearSelection' }
  | { type: 'clip.delete'; block: TransportBlock }
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
