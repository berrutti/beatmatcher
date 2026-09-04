// @vitest-environment happy-dom
import { describe, it, expect } from 'vitest';
import { mount } from '@vue/test-utils';
import { nextTick, defineComponent, ref } from 'vue';
import { i18n } from '@renderer/i18n';
import Modal from '../Modal.vue';

describe('Modal', () => {
  it('focuses the confirm button by default when it opens', async () => {
    // The auto-focus watcher only fires on a change, matching real usage
    // where a modal starts closed and toggles open later - mounting
    // directly with open:true would never trigger it.
    const wrapper = mount(Modal, {
      global: { plugins: [i18n] },
      props: { open: false, title: 'Title' },
      attachTo: document.body
    });
    await wrapper.setProps({ open: true });
    await nextTick();
    await nextTick();

    const confirmBtn = wrapper.find('.modal__btn--confirm').element;
    expect(document.activeElement).toBe(confirmBtn);
    wrapper.unmount();
  });

  it('focuses autoFocusEl instead of the confirm button when given', async () => {
    // Mirrors how BpmModal passes its input's template ref down, so the
    // regression (confirm button stealing focus from the input) is caught
    // at the level it actually happened: two independent open-watchers
    // racing for focus.
    const Host = defineComponent({
      components: { Modal },
      props: { open: Boolean },
      setup() {
        const inputEl = ref<HTMLElement | null>(null);
        return { inputEl };
      },
      template: `
        <Modal :open="open" title="Title" :auto-focus-el="inputEl">
          <input ref="inputEl" />
        </Modal>
      `
    });

    const wrapper = mount(Host, {
      global: { plugins: [i18n] },
      props: { open: false },
      attachTo: document.body
    });
    await wrapper.setProps({ open: true });
    await nextTick();
    await nextTick();

    const input = wrapper.find('input').element;
    expect(document.activeElement).toBe(input);
    wrapper.unmount();
  });

  // Attached, because Escape is handled on the document: a listener on the panel stops
  // seeing it the moment a backdrop click moves focus to the body.
  it('emits cancel when Escape is pressed', async () => {
    const wrapper = mount(Modal, {
      global: { plugins: [i18n] },
      props: { open: true, title: 'Title' },
      attachTo: document.body
    });

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    await nextTick();

    expect(wrapper.emitted('cancel')).toHaveLength(1);
    wrapper.unmount();
  });

  it('still emits cancel on Escape after the backdrop was clicked', async () => {
    const wrapper = mount(Modal, {
      global: { plugins: [i18n] },
      props: { open: true, title: 'Title' },
      attachTo: document.body
    });

    await wrapper.find('.modal__backdrop').trigger('mousedown');
    document.body.focus();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    await nextTick();

    expect(wrapper.emitted('cancel')).toBeTruthy();
    wrapper.unmount();
  });

  it('emits cancel when clicking the backdrop', async () => {
    const wrapper = mount(Modal, {
      global: { plugins: [i18n] },
      props: { open: true, title: 'Title' }
    });

    await wrapper.find('.modal__backdrop').trigger('click');

    expect(wrapper.emitted('cancel')).toHaveLength(1);
  });

  it('does not emit cancel when clicking inside the modal body', async () => {
    const wrapper = mount(Modal, {
      global: { plugins: [i18n] },
      props: { open: true, title: 'Title' }
    });

    await wrapper.find('.modal').trigger('click');

    expect(wrapper.emitted('cancel')).toBeUndefined();
  });

  it('closes off every exit when not dismissable', async () => {
    const wrapper = mount(Modal, {
      global: { plugins: [i18n] },
      props: { open: true, title: 'Title', dismissable: false },
      attachTo: document.body
    });

    expect(wrapper.findAll('button')).toHaveLength(0);

    await wrapper.find('.modal__backdrop').trigger('click');
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    await nextTick();

    expect(wrapper.emitted('cancel')).toBeUndefined();
    wrapper.unmount();
  });
});

describe('focus on mount', () => {
  it('focuses even when it mounts with open already true', async () => {
    const wrapper = mount(Modal, {
      props: { open: true, title: 'T' },
      global: { plugins: [i18n] },
      attachTo: document.body
    });
    await nextTick();
    await nextTick();
    expect(document.activeElement).toBe(wrapper.find('.modal__btn--confirm').element);
    wrapper.unmount();
  });

  it('falls back to the panel when it opens with no controls', async () => {
    const wrapper = mount(Modal, {
      props: { open: true, title: 'T', dismissable: false },
      global: { plugins: [i18n] },
      attachTo: document.body
    });
    await nextTick();
    await nextTick();
    expect(document.activeElement).toBe(wrapper.find('.modal').element);
    wrapper.unmount();
  });
});

describe('focus trap', () => {
  it('wraps Tab from the last control back to the first', async () => {
    const wrapper = mount(Modal, {
      props: { open: true, title: 'T' },
      global: { plugins: [i18n] },
      attachTo: document.body
    });
    await nextTick();
    const buttons = wrapper.findAll('button');
    buttons[buttons.length - 1].element.focus();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab' }));
    await nextTick();
    expect(document.activeElement).toBe(buttons[0].element);
    wrapper.unmount();
  });

  it('wraps Shift+Tab from the first control back to the last', async () => {
    const wrapper = mount(Modal, {
      props: { open: true, title: 'T' },
      global: { plugins: [i18n] },
      attachTo: document.body
    });
    await nextTick();
    const buttons = wrapper.findAll('button');
    buttons[0].element.focus();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true }));
    await nextTick();
    expect(document.activeElement).toBe(buttons[buttons.length - 1].element);
    wrapper.unmount();
  });

  it('still moves focus on Tab after a backdrop click sent focus to the body', async () => {
    const wrapper = mount(Modal, {
      props: { open: true, title: 'T' },
      global: { plugins: [i18n] },
      attachTo: document.body
    });
    await nextTick();
    await wrapper.find('.modal__backdrop').trigger('click');
    document.body.focus();
    expect(wrapper.find('.modal').element.contains(document.activeElement)).toBe(false);

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab' }));
    await nextTick();

    const buttons = wrapper.findAll('button');
    expect(document.activeElement).toBe(buttons[0].element);
    wrapper.unmount();
  });

  it('keeps focus on the panel when the modal has no controls at all', async () => {
    const wrapper = mount(Modal, {
      props: { open: true, title: 'T', dismissable: false },
      global: { plugins: [i18n] },
      attachTo: document.body
    });
    await nextTick();
    expect(wrapper.findAll('button')).toHaveLength(0);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab' }));
    await nextTick();
    expect(document.activeElement).toBe(wrapper.find('.modal').element);
    wrapper.unmount();
  });
});
