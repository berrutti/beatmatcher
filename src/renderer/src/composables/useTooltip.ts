import { reactive } from 'vue';

const TOOLTIP_SHOW_DELAY_MS = 350;

type TooltipState = {
  visible: boolean;
  text: string;
  targetRect: DOMRect | null;
};

const state = reactive<TooltipState>({
  visible: false,
  text: '',
  targetRect: null
});

let showTimer: ReturnType<typeof setTimeout> | null = null;
let owner: HTMLElement | null = null;

function clearShowTimer() {
  if (showTimer !== null) {
    clearTimeout(showTimer);
    showTimer = null;
  }
}

function scheduleShow(text: string, target: HTMLElement) {
  clearShowTimer();
  owner = target;
  showTimer = setTimeout(() => {
    state.text = text;
    state.targetRect = target.getBoundingClientRect();
    state.visible = true;
  }, TOOLTIP_SHOW_DELAY_MS);
}

// `target` identifies which element is asking to hide the tooltip. The
// state is a single shared singleton across every v-tooltip instance, so
// without this check one element unmounting (or its mouseleave firing)
// could hide a tooltip that actually belongs to a different, still-visible
// element.
function hide(target?: HTMLElement) {
  if (target && target !== owner) return;
  clearShowTimer();
  state.visible = false;
  owner = null;
}

export function useTooltip() {
  return { state, scheduleShow, hide };
}
