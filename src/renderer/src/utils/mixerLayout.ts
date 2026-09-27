import type { MixerParamSpec } from '@renderer/utils/sessionCore';

// Highs on top, as a channel strip reads. A band this omits still renders, after these.
const EQ_BAND_ORDER = ['high', 'highMid', 'lowMid', 'low'];

// Time first, because it is the one a performance reaches for.
const ECHO_PARAM_ORDER = ['time', 'feedback', 'lowCut', 'return'];

function specsInOrder(specs: MixerParamSpec[], order: string[]): MixerParamSpec[] {
  const rank = (spec: MixerParamSpec) => {
    const index = order.indexOf(spec.param);
    return index === -1 ? order.length : index;
  };
  return [...specs].sort((left, right) => rank(left) - rank(right));
}

export function eqBandsInLayoutOrder(specs: MixerParamSpec[]): MixerParamSpec[] {
  return specsInOrder(specs, EQ_BAND_ORDER);
}

export function echoControlsInLayoutOrder(specs: MixerParamSpec[]): MixerParamSpec[] {
  return specsInOrder(specs, ECHO_PARAM_ORDER);
}
