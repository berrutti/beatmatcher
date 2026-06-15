import { createApp } from 'vue';
import { createPinia } from 'pinia';
import App from './App.vue';
import './assets/main.css';
import { i18n } from './i18n';
import { initSessionCore } from '@renderer/utils/sessionCore';

async function init() {
  await initSessionCore();

  const pinia = createPinia();
  const app = createApp(App);
  app.use(pinia);
  app.use(i18n);
  app.mount('#app');

  if (import.meta.hot) {
    import.meta.hot.dispose(() => {
      app.unmount();
    });
  }
}

init();
