import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import vue from 'eslint-plugin-vue';
import vueParser from 'vue-eslint-parser';
import globals from 'globals';
import prettier from 'eslint-config-prettier';

export default [
  {
    ignores: [
      '**/dist/**',
      '**/dist-web/**',
      '**/dist-tauri/**',
      '**/node_modules/**',
      'src-tauri/**',
      'session-core/**'
    ]
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  ...vue.configs['flat/essential'],
  {
    files: ['**/*.ts', '**/*.vue'],
    languageOptions: {
      parser: vueParser,
      parserOptions: {
        parser: '@typescript-eslint/parser',
        sourceType: 'module'
      },
      globals: {
        ...globals.browser,
        ...globals.node
      }
    }
  },
  {
    files: ['**/*.ts', '**/*.vue'],
    ignores: ['**/stores/**'],
    rules: {
      'no-restricted-imports': [
        'error',
        {
          paths: [
            {
              name: '@tauri-apps/api/core',
              importNames: ['invoke'],
              message: 'invoke must only be called from Pinia stores (src/renderer/src/stores/).'
            }
          ]
        }
      ]
    }
  },
  {
    rules: {
      eqeqeq: 'error',
      'no-unused-vars': 'off',
      '@typescript-eslint/no-unused-vars': 'warn',
      'vue/multi-word-component-names': 'off',
      'no-restricted-syntax': [
        'warn',
        {
          selector: 'CallExpression[callee.property.name="then"]',
          message: 'Prefer async/await over .then(). Make the function async and await the Promise.'
        },
        {
          selector:
            'CallExpression[callee.type="ArrowFunctionExpression"], CallExpression[callee.type="FunctionExpression"]',
          message: 'Avoid IIFEs. Extract to a named function instead.'
        }
      ]
    }
  },
  prettier
];
