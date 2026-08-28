import { createApp } from 'vue';
import { createPinia } from 'pinia';
import App from './App.vue';
import '@fontsource/jost/400.css';
import '@fontsource/jost/600.css';
import '@fontsource/jost/700.css';
import './assets/main.css';
import { i18n } from './i18n';
import { initSessionCore } from '@renderer/utils/sessionCore';
import { vTooltip } from '@renderer/directives/tooltip';
import { vSliderReset } from '@renderer/directives/sliderReset';
import { vMenuPlacement } from '@renderer/directives/menuPlacement';

async function init() {
  await initSessionCore();

  const pinia = createPinia();
  const app = createApp(App);
  app.use(pinia);
  app.use(i18n);
  app.directive('tooltip', vTooltip);
  app.directive('slider-reset', vSliderReset);
  app.directive('menu-placement', vMenuPlacement);
  app.mount('#app');

  if (import.meta.hot) {
    import.meta.hot.dispose(() => {
      app.unmount();
    });
  }
}

init();
