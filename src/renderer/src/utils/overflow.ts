export function overflows(el: HTMLElement): boolean {
  // Both are rounded to whole pixels, so a sub-pixel layout difference reads as an overflow.
  return el.scrollWidth > el.clientWidth + 1 || el.scrollHeight > el.clientHeight + 1;
}
