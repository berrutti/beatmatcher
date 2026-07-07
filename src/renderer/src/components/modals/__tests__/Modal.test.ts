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

  it('emits cancel when Escape is pressed', async () => {
    const wrapper = mount(Modal, {
      global: { plugins: [i18n] },
      props: { open: true, title: 'Title' }
    });

    await wrapper.find('.modal__backdrop').trigger('keydown', { key: 'Escape' });

    expect(wrapper.emitted('cancel')).toHaveLength(1);
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
});
