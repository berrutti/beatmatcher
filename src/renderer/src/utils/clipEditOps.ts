import type { SessionEvent } from '@renderer/stores/session';
import type { Clip } from '@renderer/composables/useSessionTimeline';
import {
  blocksForDeck as coreBlocksForDeck,
  blockBounds as coreBlockBounds,
  moveTransportBlock as coreMoveTransportBlock,
  trimTransportBlock as coreTrimTransportBlock
} from '@renderer/utils/sessionCore';
import { TransportBlock } from './types';

export const MIN_BLOCK_MS = 100;

export function blocksForDeck(clips: Clip[], deck: string): TransportBlock[] {
  return coreBlocksForDeck(clips, deck);
}

// The range the block's boundaries may occupy, for live drag previews that
// want to show the same clamping the commit will apply.
export function blockBounds(
  events: SessionEvent[],
  clips: Clip[],
  block: TransportBlock
): { minStartMs: number; maxEndMs: number } | null {
  return coreBlockBounds(events, clips, block);
}

export function moveTransportBlock(
  events: SessionEvent[],
  clips: Clip[],
  block: TransportBlock,
  deltaMs: number
): { events: SessionEvent[]; appliedDeltaMs: number } {
  return coreMoveTransportBlock(events, clips, block, deltaMs);
}

export function trimTransportBlock(
  events: SessionEvent[],
  clips: Clip[],
  block: TransportBlock,
  edge: 'start' | 'end',
  newMs: number
): { events: SessionEvent[]; appliedMs: number } {
  return coreTrimTransportBlock(events, clips, block, edge, newMs);
}
