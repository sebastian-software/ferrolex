'use strict';

const CONFIGURATION_SECTION = 'ferrolex';
const DEFAULT_COMMAND = 'ferrolex-lsp';

let client;

function stringList(value) {
  return Array.isArray(value) ? value.filter((item) => typeof item === 'string') : [];
}

function readSettings(vscode) {
  const configuration = vscode.workspace.getConfiguration(CONFIGURATION_SECTION);
  const userDictionaryPath = configuration.get('userDictionaryPath', '');

  return {
    command: configuration.get('lsp.command', DEFAULT_COMMAND),
    words: stringList(configuration.get('dictionary.words', ['ferrolex'])),
    ignoredWords: stringList(configuration.get('ignoredWords', [])),
    commentPrefix: configuration.get('commentPrefix', '//'),
    userDictionaryPath:
      typeof userDictionaryPath === 'string' && userDictionaryPath.length > 0
        ? userDictionaryPath
        : undefined
  };
}

function createServerOptions(vscode, transport) {
  const { command } = readSettings(vscode);
  return { command, args: [], transport };
}

function createClientOptions(vscode) {
  const settings = readSettings(vscode);
  return {
    documentSelector: [{ scheme: 'file' }, { scheme: 'untitled' }],
    initializationOptions: {
      ferrolex: {
        words: settings.words,
        ignoredWords: settings.ignoredWords,
        commentPrefix: settings.commentPrefix,
        ...(settings.userDictionaryPath === undefined
          ? {}
          : { userDictionaryPath: settings.userDictionaryPath })
      }
    },
    synchronize: {
      configurationSection: CONFIGURATION_SECTION
    }
  };
}

async function startClient(context, vscode, LanguageClient, TransportKind) {
  client = new LanguageClient(
    'ferrolex',
    'Ferrolex Language Server',
    createServerOptions(vscode, TransportKind.stdio),
    createClientOptions(vscode)
  );

  await client.start();
  context.subscriptions.push({ dispose: () => void client?.stop() });
  context.subscriptions.push(
    vscode.commands.registerCommand('ferrolex.restartLanguageServer', async () => {
      await client.stop();
      await client.start();
    })
  );

  return client;
}

async function activate(context) {
  const vscode = require('vscode');
  const { LanguageClient, TransportKind } = require('vscode-languageclient/node');
  return startClient(context, vscode, LanguageClient, TransportKind);
}

async function deactivate() {
  await client?.stop();
  client = undefined;
}

module.exports = {
  DEFAULT_COMMAND,
  activate,
  createClientOptions,
  createServerOptions,
  deactivate,
  readSettings,
  startClient
};
