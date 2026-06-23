<template>
  <Modal :open="open" :title="t('update.title')" :confirm-label="confirmLabel" @confirm="onConfirm" @cancel="updater.dismiss()">
    <p class="update__body">{{ bodyText }}</p>
  </Modal>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { useUpdaterStore } from '@renderer/stores/updater';
import Modal from '@renderer/components/modals/Modal.vue';

const { t } = useI18n();
const updater = useUpdaterStore();

const open = computed(
  () =>
    updater.status === 'available' ||
    updater.status === 'downloading' ||
    updater.status === 'error'
);

const bodyText = computed(() => {
  if (updater.status === 'downloading') {
    if (updater.totalBytes && updater.totalBytes > 0) {
      const pct = Math.round((updater.downloadedBytes / updater.totalBytes) * 100);
      return t('update.downloading', { pct });
    }
    return t('update.downloadingNoSize');
  }
  if (updater.status === 'error') return t('update.error');
  if (updater.notes) return updater.notes;
  return t('update.available', { version: updater.newVersion ?? '' });
});

const confirmLabel = computed(() => {
  if (updater.status === 'downloading') return t('update.installing');
  if (updater.status === 'error') return t('update.retry');
  return t('update.install');
});

async function onConfirm(): Promise<void> {
  if (updater.status === 'downloading') return;
  await updater.downloadAndInstall();
}
</script>

<style scoped>
.update__body {
  font-size: 0.75rem;
  color: var(--color-muted);
  line-height: 1.5;
  margin: 0;
}
</style>
