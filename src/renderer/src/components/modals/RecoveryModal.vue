<template>
  <Modal
    v-if="item"
    :open="true"
    :title="confirming ? $t('recovery.discardTitle') : $t('recovery.title')"
    :body="confirming ? $t('recovery.discardBody') : $t('recovery.body')"
    :dismissable="false"
    :auto-focus-el="confirming ? cancelBtn : firstSaveBtn"
  >
    <template v-if="!confirming">
      <div class="recovery__files">
        <div v-if="item.audioBytes > 0" class="recovery__file">
          <span class="recovery__label">
            {{ $t(item.kind === 'render' ? 'recovery.renderFile' : 'recovery.audioFile') }}
          </span>
          <span class="recovery__size">{{ sizeLabel }}</span>
          <Button ref="firstSaveBtn" variant="primary" :disabled="busy" @click="onSave('audio')">
            {{ $t('modal.save') }}
          </Button>
        </div>
        <div v-if="item.logPath" class="recovery__file">
          <span class="recovery__label">{{ $t('recovery.sessionLog') }}</span>
          <span class="recovery__size"></span>
          <Button ref="logSaveBtn" variant="primary" :disabled="busy" @click="onSave('log')">
            {{ $t('modal.save') }}
          </Button>
        </div>
      </div>
      <div v-if="remaining > 0" class="recovery__note">
        {{ $t('recovery.remaining', { count: remaining }) }}
      </div>
      <div class="recovery__actions">
        <Button variant="danger" :disabled="busy" @click="onDiscardClick">
          {{ $t('modal.discard') }}
        </Button>
      </div>
    </template>

    <template v-else>
      <label class="recovery__ask">
        <input v-model="dontAskAgain" type="checkbox" />
        <span>{{ $t('recovery.dontAskAgain') }}</span>
      </label>
      <div class="recovery__actions">
        <Button ref="cancelBtn" :disabled="busy" @click="confirming = false">
          {{ $t('modal.cancel') }}
        </Button>
        <Button variant="danger" :disabled="busy" @click="onDiscard">
          {{ $t('modal.discard') }}
        </Button>
      </div>
    </template>
  </Modal>
</template>

<script setup lang="ts">
import { computed, nextTick, ref } from 'vue';
import Modal from '@renderer/components/modals/Modal.vue';
import Button from '@renderer/components/Button.vue';
import { useRecoveryStore, type RecoverableFile } from '@renderer/stores/recovery';

const recovery = useRecoveryStore();
const busy = ref(false);
const confirming = ref(false);
const dontAskAgain = ref(false);

type Focusable = { focus: () => void };

const firstSaveBtn = ref<Focusable | null>(null);
const cancelBtn = ref<Focusable | null>(null);
const logSaveBtn = ref<Focusable | null>(null);

const item = computed(() => recovery.pending[0] ?? null);
const remaining = computed(() => Math.max(0, recovery.pending.length - 1));

const sizeLabel = computed(() => {
  const bytes = item.value?.audioBytes ?? 0;
  const megabytes = bytes / (1024 * 1024);
  return megabytes >= 1 ? `${megabytes.toFixed(1)} MB` : `${Math.round(bytes / 1024)} KB`;
});

// Saving a file removes its row, so Enter would otherwise land on a button that is gone.
async function refocus(): Promise<void> {
  await nextTick();
  (firstSaveBtn.value ?? logSaveBtn.value)?.focus();
}

async function onSave(file: RecoverableFile): Promise<void> {
  const target = item.value;
  if (!target || busy.value) return;
  busy.value = true;
  try {
    await recovery.saveFile(target, file);
  } finally {
    busy.value = false;
  }
  await refocus();
}

async function onDiscardClick(): Promise<void> {
  if (recovery.skipDiscardConfirm) {
    await onDiscard();
    return;
  }
  confirming.value = true;
  await nextTick();
  cancelBtn.value?.focus();
}

async function onDiscard(): Promise<void> {
  const target = item.value;
  if (!target || busy.value) return;
  busy.value = true;
  try {
    if (dontAskAgain.value) recovery.alwaysSkipDiscardConfirm();
    await recovery.discard(target);
  } finally {
    busy.value = false;
  }
  confirming.value = false;
  dontAskAgain.value = false;
  await refocus();
}
</script>

<style scoped>
.recovery__files {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 340px;
}

.recovery__file {
  display: grid;
  grid-template-columns: 1fr auto auto;
  align-items: center;
  gap: 12px;
  font-size: 0.75rem;
  color: var(--color-text);
}

.recovery__size {
  color: var(--color-muted);
  font-variant-numeric: tabular-nums;
}

.recovery__note {
  font-size: 0.65rem;
  letter-spacing: 0.04em;
  color: var(--color-muted);
}

.recovery__ask {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 340px;
  font-size: 0.7rem;
  color: var(--color-muted);
  cursor: pointer;
}

/* Gated like the buttons: see `utils/keyboardNav.ts`. */
:root[data-keyboard-nav] .recovery__ask input:focus {
  outline: 2px solid var(--color-text);
  outline-offset: 2px;
}

.recovery__actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
}
</style>
