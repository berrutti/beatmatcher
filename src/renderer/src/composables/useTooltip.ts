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

// One shared state across every v-tooltip, so an element leaving could otherwise
// hide a tooltip belonging to a different, still-visible one.
function hide(target?: HTMLElement) {
  if (target && target !== owner) return;
  clearShowTimer();
  state.visible = false;
  owner = null;
}

export function useTooltip() {
  return { state, scheduleShow, hide };
}
