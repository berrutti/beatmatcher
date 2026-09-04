// A trackpad pinch arrives as a wheel event with ctrlKey set, and that flag is the only
// thing telling it apart from a two-finger swipe: both carry deltas on both axes.
export function wheelIntent(event: { ctrlKey: boolean; metaKey: boolean }): 'zoom' | 'pan' {
  return event.ctrlKey || event.metaKey ? 'zoom' : 'pan';
}
