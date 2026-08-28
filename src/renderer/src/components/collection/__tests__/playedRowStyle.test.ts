import { describe, it, expect } from 'vitest';
import browserSource from '../Browser.vue?raw';

// jsdom does not apply a component's <style>, so a rule that selects nothing is
// invisible to a mounted test. The stylesheet itself is the subject here.
function browserStyles(): string {
  return browserSource.slice(
    browserSource.indexOf('<style'),
    browserSource.lastIndexOf('</style>')
  );
}

function selectorsFor(marker: string): string[] {
  return browserStyles()
    .split('}')
    .map((block) => block.slice(0, block.indexOf('{')).trim())
    .filter((selector) => selector.includes(marker));
}

describe('a played track reads as used in every list that marks one', () => {
  it('styles the row itself, not only a name element a table row lacks', () => {
    const selectors = selectorsFor('.collection__item--played');
    expect(selectors.length).toBeGreaterThan(0);

    const stylesTheRow = selectors.some((selector) =>
      selector
        .split(',')
        .map((part) => part.trim())
        .some((part) => part.endsWith('.collection__item--played'))
    );
    expect(stylesTheRow, `only descendant rules found: ${selectors.join(' | ')}`).toBe(true);
  });

  it('is not overridden by a rule that colours a cell inside the row', () => {
    const styles = browserStyles();
    // Inheritance loses to any direct rule, so every element that sets its own
    // colour has to say what it does on a played row.
    const colouring = styles
      .replace(/\/\*[\s\S]*?\*\//g, '')
      .split('}')
      .map((block) => ({
        selector: block.slice(0, block.indexOf('{')).trim(),
        body: block.slice(block.indexOf('{'))
      }))
      .filter(
        ({ selector, body }) =>
          // Only what a row actually contains: a cell value or a track name.
          /^\.collection__(meta-value|item-name|td|playlist-num)/.test(selector) &&
          !selector.includes('--played') &&
          /color:\s*var\(--color-text\)/.test(body)
      )
      .map(({ selector }) => selector);

    for (const selector of colouring) {
      const guarded = styles.includes(`.collection__item--played ${selector}`);
      expect(guarded, `${selector} sets its own colour and overrides the played row`).toBe(true);
    }
  });

  it('keeps the name rule for the lists that do render a name element', () => {
    const selectors = selectorsFor('.collection__item--played');
    const stylesTheName = selectors.some((selector) =>
      selector.includes('.collection__item--played .collection__item-name')
    );
    expect(stylesTheName).toBe(true);
  });
});

describe('a track title is read, not scanned', () => {
  it('gives the title column full contrast and leaves the rest muted', () => {
    const styles = browserStyles();
    expect(selectorsFor('.collection__meta-value--title').length).toBeGreaterThan(0);
    const titleRule = styles.slice(styles.indexOf('.collection__meta-value--title'));
    expect(titleRule.slice(0, titleRule.indexOf('}'))).toContain('var(--color-text)');
  });
});

describe('renaming a set does not move its name', () => {
  it('right-aligns the rename input, matching the title it replaces', () => {
    const styles = browserStyles();
    const rule = styles.slice(styles.indexOf('.collection__playlist-rename {'));
    expect(rule.slice(0, rule.indexOf('}'))).toContain('text-align: right');
  });
});

describe('a row that drags to a deck says so', () => {
  it('gives playlist rows the same hand as the ALL view rows', () => {
    const styles = browserStyles();
    const rule = styles.slice(styles.indexOf('.collection__playlist-track {'));
    expect(rule.slice(0, rule.indexOf('}'))).toContain('cursor: grab');
  });

  it('makes the reorder handle a whole cell, not one glyph', () => {
    const styles = browserStyles();
    const rule = styles.slice(styles.indexOf('.collection__playlist-handle {'));
    expect(rule.slice(0, rule.indexOf('}'))).toContain('cursor: grab');
  });

  it('holds one cursor for the whole reorder', () => {
    // Otherwise the hand reopens each time the pointer crosses a row that is
    // not itself a handle, mid-gesture.
    expect(browserStyles()).toContain('.collection__table--reordering *');
  });
});

describe('a standing transform transition is not left on every row', () => {
  it('scopes the reorder slide to a reorder that is actually running', () => {
    const styles = browserStyles();
    const plain = styles.slice(styles.indexOf('\n.collection__playlist-track {'));
    // A standing transform transition keeps every row on its own compositor
    // layer, which shows up as flicker when the list is merely scrolled.
    expect(plain.slice(0, plain.indexOf('}'))).not.toContain('transform');
    expect(styles).toContain('.collection__table--reordering .collection__playlist-track');
  });
});

describe('the reorder handle reads as a column of its own', () => {
  it('centres the handle cells, grip and number alike', () => {
    const styles = browserStyles();
    const rule = styles.slice(styles.indexOf('.collection__playlist-handle {'));
    const body = rule.slice(0, rule.indexOf('}'));
    expect(body).toContain('text-align: center');
  });
});

describe('picking a row up and putting it down is animated', () => {
  it('eases the dragged row into and out of its held state', () => {
    const styles = browserStyles();
    // On the row, not the modifier: declared on the modifier the ease only
    // plays on the way in, so releasing snaps.
    const dragging = styles.slice(styles.indexOf('.collection__playlist-track--dragging {'));
    expect(dragging.slice(0, dragging.indexOf('}'))).not.toContain('transition');
    const plain = styles.slice(styles.indexOf('\n.collection__playlist-track {'));
    expect(plain.slice(0, plain.indexOf('}'))).toContain('transition: opacity');
  });
});
