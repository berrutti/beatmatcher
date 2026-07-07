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

function clearShowTimer() {
  if (showTimer !== null) {
    clearTimeout(showTimer);
    showTimer = null;
  }
}

function scheduleShow(text: string, target: HTMLElement) {
  clearShowTimer();
  showTimer = setTimeout(() => {
    state.text = text;
    state.targetRect = target.getBoundingClientRect();
    state.visible = true;
  }, TOOLTIP_SHOW_DELAY_MS);
}

function hide() {
  clearShowTimer();
  state.visible = false;
}

export function useTooltip() {
  return { state, scheduleShow, hide };
}
