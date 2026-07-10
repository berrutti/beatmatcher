import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import type { ColumnField } from '@renderer/stores/collection';

export function useColumnLabels() {
  const { t } = useI18n();

  const COLUMN_LABELS = computed<Record<ColumnField, string>>(() => ({
    title: t('browser.colTitle'),
    artist: t('browser.colArtist'),
    album: t('browser.colAlbum'),
    albumArtist: t('browser.colAlbumArtist'),
    genre: t('browser.colGenre'),
    composer: t('browser.colComposer'),
    remixer: t('browser.colRemixer'),
    label: t('browser.colLabel'),
    comment: t('browser.colComment'),
    trackNumber: t('browser.colTrackNumber'),
    year: t('browser.colYear'),
    rating: t('browser.colRating'),
    bpm: t('browser.colBpm'),
    added: t('browser.colAdded')
  }));

  function getColumnLabel(field: ColumnField): string {
    return COLUMN_LABELS.value[field];
  }

  return { COLUMN_LABELS, getColumnLabel };
}
