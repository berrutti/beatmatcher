import type { ObjectDirective } from 'vue';
import { clampMenuPosition } from '@renderer/utils/menuPlacement';

function place(el: HTMLElement): void {
  const rect = el.getBoundingClientRect();
  const { x, y } = clampMenuPosition(
    { x: parseFloat(el.style.left) || 0, y: parseFloat(el.style.top) || 0 },
    { width: rect.width, height: rect.height },
    { width: window.innerWidth, height: window.innerHeight }
  );
  el.style.left = `${x}px`;
  el.style.top = `${y}px`;
}

export const vMenuPlacement: ObjectDirective<HTMLElement> = {
  mounted: place,
  updated: place
};
