<template>
  <Modal
    :open="open"
    :title="title"
    :confirm-label="props.confirmLabel ?? t('confirmModal.load')"
    @confirm="emit('confirm')"
    @cancel="emit('cancel')"
  >
    <p class="body">{{ body }}</p>
  </Modal>
</template>

<script setup lang="ts">
import Modal from './Modal.vue';
import { useI18n } from 'vue-i18n';

const { t } = useI18n();

const props = withDefaults(
  defineProps<{ open: boolean; title: string; body: string; confirmLabel?: string }>(),
  { confirmLabel: undefined }
);
const emit = defineEmits<{ confirm: []; cancel: [] }>();
</script>

<style scoped>
.body {
  font-size: 0.75rem;
  color: var(--color-muted);
  line-height: 1.5;
  margin: 0;
}
</style>
