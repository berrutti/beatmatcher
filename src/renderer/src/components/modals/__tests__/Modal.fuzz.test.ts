import { describe, it, expect, afterEach } from 'vitest';
import { mount, type VueWrapper } from '@vue/test-utils';
import { nextTick } from 'vue';
import { i18n } from '@renderer/i18n';
import Modal from '../Modal.vue';
import { anyModalOpen } from '@renderer/utils/modalStack';

function makeRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

type Wrapper = VueWrapper<InstanceType<typeof Modal>>;

function open(props: { open: boolean; dismissable: boolean }): Wrapper {
  return mount(Modal, {
    props: { title: 'T', ...props },
    global: { plugins: [i18n] },
    attachTo: document.body
  });
}

// Somewhere for focus to be stolen to, the way a background control would.
let outside: HTMLButtonElement | null = null;

function outsideButton(): HTMLButtonElement {
  if (!outside) {
    outside = document.createElement('button');
    document.body.appendChild(outside);
  }
  return outside;
}

afterEach(() => {
  outside?.remove();
  outside = null;
});

function key(name: string, shiftKey = false): void {
  document.dispatchEvent(new KeyboardEvent('keydown', { key: name, shiftKey }));
}

describe('Modal state under random operation sequences', () => {
  it('never leaves the keyboard held after every modal is gone', async () => {
    const random = makeRandom(7);
    const live: { wrapper: Wrapper; open: boolean }[] = [];
    let mounts = 0;

    for (let step = 0; step < 600; step++) {
      const roll = random();
      if (roll < 0.4 || live.length === 0) {
        live.push({
          wrapper: open({ open: random() < 0.8, dismissable: random() < 0.5 }),
          open: true
        });
        const entry = live[live.length - 1];
        entry.open = entry.wrapper.props('open') === true;
        mounts++;
      } else if (roll < 0.7) {
        const at = Math.floor(random() * live.length);
        const next = !live[at].open;
        await live[at].wrapper.setProps({ open: next });
        live[at].open = next;
      } else {
        const at = Math.floor(random() * live.length);
        live[at].wrapper.unmount();
        live.splice(at, 1);
      }
      await nextTick();

      const expected = live.some((entry) => entry.open);
      expect(anyModalOpen.value, `step ${step}`).toBe(expected);
    }

    for (const entry of live) entry.wrapper.unmount();
    await nextTick();
    expect(anyModalOpen.value).toBe(false);
    expect(mounts).toBeGreaterThan(10);
  });

  it('keeps focus inside an open modal no matter where it is pushed', async () => {
    const random = makeRandom(23);
    const wrapper = open({ open: true, dismissable: true });
    await nextTick();
    const panel = wrapper.find('.modal').element;
    if (!(panel instanceof HTMLElement)) throw new Error('the panel should be an element');
    let trapped = 0;

    for (let step = 0; step < 800; step++) {
      const roll = random();
      if (roll < 0.25) {
        outsideButton().focus();
      } else if (roll < 0.4) {
        await wrapper.find('.modal__backdrop').trigger('click');
        document.body.focus();
      } else if (roll < 0.5) {
        panel.focus();
      }

      key('Tab', random() < 0.5);
      await nextTick();

      expect(panel.contains(document.activeElement), `step ${step}`).toBe(true);
      trapped++;
    }

    expect(trapped).toBe(800);
    wrapper.unmount();
  });

  it('offers a modal that is not dismissable no way out, however it is prodded', async () => {
    const random = makeRandom(101);
    const wrapper = open({ open: true, dismissable: false });
    await nextTick();

    for (let step = 0; step < 800; step++) {
      const roll = random();
      if (roll < 0.3) key('Escape');
      else if (roll < 0.5) await wrapper.find('.modal__backdrop').trigger('click');
      else if (roll < 0.7) await wrapper.find('.modal').trigger('click');
      else key('Tab', random() < 0.5);
      await nextTick();

      expect(wrapper.emitted('cancel'), `step ${step}`).toBeUndefined();
      expect(wrapper.emitted('confirm'), `step ${step}`).toBeUndefined();
    }

    expect(anyModalOpen.value).toBe(true);
    wrapper.unmount();
    await nextTick();
    expect(anyModalOpen.value).toBe(false);
  });

  it('answers the keyboard only on the topmost of stacked modals', async () => {
    const random = makeRandom(77);
    const under = open({ open: true, dismissable: true });
    const over = open({ open: true, dismissable: false });
    await nextTick();
    const overPanel = over.find('.modal').element;
    let tabs = 0;

    for (let step = 0; step < 600; step++) {
      if (random() < 0.5) {
        key('Escape');
      } else {
        key('Tab', random() < 0.5);
        tabs++;
        await nextTick();
        expect(overPanel.contains(document.activeElement), `step ${step}`).toBe(true);
      }
      await nextTick();
      expect(under.emitted('cancel'), `step ${step}`).toBeUndefined();
    }

    expect(tabs).toBeGreaterThan(100);
    over.unmount();
    under.unmount();
  });

  it('only ever exits a dismissable modal through a real exit', async () => {
    const random = makeRandom(55);
    const wrapper = open({ open: true, dismissable: true });
    await nextTick();
    let expectedCancels = 0;

    for (let step = 0; step < 800; step++) {
      const roll = random();
      if (roll < 0.25) {
        key('Escape');
        expectedCancels++;
      } else if (roll < 0.5) {
        await wrapper.find('.modal__backdrop').trigger('click');
        expectedCancels++;
      } else if (roll < 0.7) {
        // Inside the panel: never an exit, however many times it is clicked.
        await wrapper.find('.modal').trigger('click');
      } else {
        key('Tab', random() < 0.5);
      }
      await nextTick();

      expect(wrapper.emitted('cancel')?.length ?? 0, `step ${step}`).toBe(expectedCancels);
    }

    expect(expectedCancels).toBeGreaterThan(50);
    wrapper.unmount();
  });
});
