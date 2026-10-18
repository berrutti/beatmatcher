import type { MixerParamSpec } from '@renderer/utils/sessionCore';

// Highs on top, as a channel strip reads.
const EQ_BAND_ORDER = ['high', 'highMid', 'lowMid', 'low'];

export function eqBandsInLayoutOrder(specs: MixerParamSpec[]): MixerParamSpec[] {
  const rank = (spec: MixerParamSpec) => {
    const index = EQ_BAND_ORDER.indexOf(spec.param);
    return index === -1 ? EQ_BAND_ORDER.length : index;
  };
  return [...specs].sort((left, right) => rank(left) - rank(right));
}
