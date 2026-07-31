import { describe, it, expect } from 'vitest';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';

const RENDERER = join(__dirname, '..', 'src', 'renderer', 'src');
const MAIN_CSS = readFileSync(join(RENDERER, 'assets', 'main.css'), 'utf8');
const TO_LIGHT = /color-scheme:\s*(light|only\s+light)/;

function vueFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) return vueFiles(full);
    return full.endsWith('.vue') ? [full] : [];
  });
}

function rootBlock(): string {
  const start = MAIN_CSS.indexOf(':root {');
  expect(start).toBeGreaterThanOrEqual(0);
  return MAIN_CSS.slice(start, MAIN_CSS.indexOf('}', start));
}

// The UA paints number-input spinners and scrollbars itself, and under its default light
// scheme they come out near-black here, which reads as the arrows being missing.
describe('native control colour scheme', () => {
  it('declares a dark scheme at the root', () => {
    expect(rootBlock()).toMatch(/color-scheme:\s*dark/);
  });

  it('is not overridden back to light anywhere', () => {
    expect(MAIN_CSS).not.toMatch(TO_LIGHT);
    for (const file of vueFiles(RENDERER)) {
      expect(readFileSync(file, 'utf8'), file).not.toMatch(TO_LIGHT);
    }
  });

  it('reaches every component that renders a number input', () => {
    const withNumberInputs = vueFiles(RENDERER).filter((file) =>
      readFileSync(file, 'utf8').includes('type="number"')
    );
    expect(withNumberInputs.length).toBeGreaterThan(0);
    for (const file of withNumberInputs) {
      expect(readFileSync(file, 'utf8'), file).not.toMatch(/color-scheme:/);
    }
  });
});
