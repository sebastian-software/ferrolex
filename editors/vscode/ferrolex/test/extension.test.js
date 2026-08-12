'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');

const {
  DEFAULT_COMMAND,
  createClientOptions,
  createServerOptions,
  readSettings,
  startClient
} = require('../extension');

function vscodeWith(values) {
  return {
    workspace: {
      getConfiguration(section) {
        assert.equal(section, 'ferrolex');
        return {
          get(key, fallback) {
            return Object.hasOwn(values, key) ? values[key] : fallback;
          }
        };
      }
    }
  };
}

test('uses the PATH server and small default dictionary', () => {
  const settings = readSettings(vscodeWith({}));

  assert.deepEqual(settings, {
    command: DEFAULT_COMMAND,
    words: ['ferrolex'],
    ignoredWords: [],
    commentPrefix: '//',
    userDictionaryPath: undefined
  });
});

test('maps VS Code settings into LSP initialization and stdio options', () => {
  const vscode = vscodeWith({
    'lsp.command': '/opt/ferrolex/bin/ferrolex-lsp',
    'dictionary.words': ['ferrolex', 'Project'],
    ignoredWords: ['generated'],
    commentPrefix: '#',
    userDictionaryPath: '/tmp/ferrolex-words.txt'
  });

  assert.deepEqual(createServerOptions(vscode, 'stdio'), {
    command: '/opt/ferrolex/bin/ferrolex-lsp',
    args: [],
    transport: 'stdio'
  });
  assert.deepEqual(createClientOptions(vscode), {
    documentSelector: [{ scheme: 'file' }, { scheme: 'untitled' }],
    initializationOptions: {
      ferrolex: {
        words: ['ferrolex', 'Project'],
        ignoredWords: ['generated'],
        commentPrefix: '#',
        userDictionaryPath: '/tmp/ferrolex-words.txt'
      }
    },
    synchronize: { configurationSection: 'ferrolex' }
  });
});

test('does not forward malformed word-list settings', () => {
  const options = createClientOptions(
    vscodeWith({ 'dictionary.words': 'ferrolex', ignoredWords: [42, 'kept'] })
  );

  assert.deepEqual(options.initializationOptions.ferrolex.words, []);
  assert.deepEqual(options.initializationOptions.ferrolex.ignoredWords, ['kept']);
});

test('starts an stdio language client and wires the restart command', async () => {
  const calls = [];
  class FakeLanguageClient {
    constructor(...arguments_) {
      this.arguments = arguments_;
      this.starts = 0;
      this.stops = 0;
    }

    async start() {
      this.starts += 1;
    }

    async stop() {
      this.stops += 1;
    }
  }

  let restart;
  const vscode = {
    ...vscodeWith({}),
    commands: {
      registerCommand(command, handler) {
        calls.push(command);
        restart = handler;
        return { dispose() {} };
      }
    }
  };
  const context = { subscriptions: [] };
  const languageClient = await startClient(
    context,
    vscode,
    FakeLanguageClient,
    { stdio: 'stdio' }
  );

  assert.equal(languageClient.starts, 1);
  assert.deepEqual(languageClient.arguments.slice(0, 2), ['ferrolex', 'Ferrolex Language Server']);
  assert.equal(languageClient.arguments[2].command, DEFAULT_COMMAND);
  assert.equal(languageClient.arguments[2].transport, 'stdio');
  assert.deepEqual(calls, ['ferrolex.restartLanguageServer']);
  assert.equal(context.subscriptions.length, 2);

  await restart();
  assert.equal(languageClient.stops, 1);
  assert.equal(languageClient.starts, 2);
});
