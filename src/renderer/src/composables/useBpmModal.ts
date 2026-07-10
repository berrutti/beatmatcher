import { ref, computed } from 'vue';
import { useCollectionStore } from '@renderer/stores/collection';

// Each table (ALL, playlist-detail) owns its own instance rather than
// sharing one - only one modal is ever open at a time regardless, but there's
// no benefit to coupling the two tables' UI state together for it.
export function useBpmModal() {
  const store = useCollectionStore();

  const bpmModalTrackId = ref<string | null>(null);
  const bpmModalCurrentBpm = computed(() => {
    const track = store.tracks.find((t) => t.id === bpmModalTrackId.value);
    return track ? store.getBpm(track) : null;
  });

  function openBpmModal(id: string) {
    bpmModalTrackId.value = id;
  }

  function onBpmSubmit(bpm: number) {
    if (bpmModalTrackId.value) store.setBpm(bpmModalTrackId.value, bpm);
    bpmModalTrackId.value = null;
  }

  return { bpmModalTrackId, bpmModalCurrentBpm, openBpmModal, onBpmSubmit };
}
