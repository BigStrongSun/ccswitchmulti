// Execute the shipped renderer snippets against synthetic state only. No CDP or user data.
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const assert = require("node:assert/strict");

const source = fs.readFileSync(
  path.join(__dirname, "../src-tauri/src/codex_desktop.rs"),
  "utf8",
);
function core(name) {
  const match = source.match(
    new RegExp(`fn ${name}\\(\\) -> &'static str \\{\\s*r#"([\\s\\S]*?)"#`),
  );
  assert.ok(match, `missing real renderer snippet ${name}`);
  return match[1];
}
function template(name) {
  const match = source.match(
    new RegExp(`fn ${name}\\([\\s\\S]*?r#"([\\s\\S]*?)"#`),
  );
  assert.ok(match, `missing real renderer template ${name}`);
  return match[1].replaceAll("{{", "{").replaceAll("}}", "}");
}
const patchKey = "__ccSwitchCodexAppCompatibilityV10";
const releaseScript = template("build_model_picker_release_script").replaceAll(
  "{MODEL_PICKER_PATCH_KEY}",
  patchKey,
);
const substitutions = {
  payload: JSON.stringify({
    defaultModel: "routed-model",
    modelProvider: "custom",
    modelNames: ["routed-model"],
    models: [],
  }),
  MODEL_PICKER_PATCH_KEY: patchKey,
  release_script: releaseScript,
  activation_guard: core("renderer_activation_guard_core_script"),
  lifecycle_core: core("model_picker_lifecycle_core_script"),
  model_patch_core: core("model_picker_patch_core_script"),
  guardian_v2_patch_core: core("guardian_v2_compatibility_patch_core_script"),
  request_normalization_core: core(
    "app_server_request_normalization_core_script",
  ),
  history_lineage_hydration_core: core("history_lineage_hydration_core_script"),
  module_client_discovery_core: core(
    "app_server_module_client_discovery_core_script",
  ),
};
let rendererScript = template("build_model_picker_unlock_script");
for (const [name, value] of Object.entries(substitutions))
  rendererScript = rendererScript.replaceAll(`{${name}}`, value);
new vm.Script(rendererScript);

const setup = `
const window = globalThis;
const patchKey = '__ccSwitchCodexAppCompatibilityV10';
const clearInterval = () => {};
window.removeEventListener = () => {};
const state = { payload: {}, historyManagerCandidates: new Set() };
window[patchKey] = state;
${core("model_picker_lifecycle_core_script")}
${core("history_lineage_hydration_core_script")}
let conversation = { id: 'synthetic-thread', rolloutPath: 'old.jsonl', turns: [{ id: 'synthetic-turn' }], resumeState: 'resumed' };
let mutations = 0;
let completeLoads = 0;
let finishResume;
const manager = {
  getCachedConversations: () => [conversation],
  getConversation: () => conversation,
  isConversationStreaming: () => false,
  threadStore: { threadsById: new Map([['synthetic-thread', { path: 'active.jsonl' }]]) },
  updateConversationState: (_id, update) => { mutations += 1; const next = { ...conversation }; update(next); conversation = next; },
  setConversation: value => { mutations += 1; conversation = value; },
  resumeConversationForUnavailableOwner: () => new Promise(resolve => { finishResume = () => resolve({ status: 'ready' }); }),
  getCompleteConversationTurns: async () => { completeLoads += 1; return []; },
};
`;

async function main() {
  const fenced = (script, epoch) =>
    template("build_epoch_fenced_document_script")
      .replaceAll("{epoch}", String(epoch))
      .replaceAll("{registration}", "window.registered = true;")
      .replaceAll("{script}", script);
  const epochContext = vm.createContext({});
  vm.runInContext("window = globalThis; effects = 0;", epochContext);
  const epoch = vm.runInContext(
    core("renderer_authorize_core_script"),
    epochContext,
  );
  vm.runInContext(core("renderer_release_fence_core_script"), epochContext);
  const late = await vm.runInContext(
    fenced("(++effects)", epoch),
    epochContext,
  );
  assert.equal(late.cancelled, true);
  assert.equal(vm.runInContext("effects", epochContext), 0);
  assert.equal(vm.runInContext("window.registered", epochContext), undefined);
  const fresh = vm.createContext({});
  vm.runInContext("window = globalThis; effects = 0;", fresh);
  await vm.runInContext(fenced("(++effects)", epoch + 7), fresh);
  assert.equal(
    vm.runInContext("effects", fresh),
    1,
    "registered generations must work in a fresh document",
  );
  for (const generations of [
    [1, 2],
    [2, 1],
  ]) {
    const navigation = vm.createContext({});
    vm.runInContext(
      "window = globalThis; effects = 0; latest = 0;",
      navigation,
    );
    for (const generation of generations)
      await vm.runInContext(
        fenced(`(latest = ${generation})`, generation),
        navigation,
      );
    assert.equal(
      vm.runInContext("latest", navigation),
      2,
      "latest managed catalog must win in a fresh realm",
    );
  }
  const awaiting = vm.createContext({});
  vm.runInContext(
    `window = globalThis; let finish; const pending = new Promise(r => finish = () => { delete window['${patchKey}']; r({ disposed: true, drained: true }); }); window['${patchKey}'] = { lifecycleVersion: '1', dispose: () => pending };`,
    awaiting,
  );
  const active = vm.runInContext(
    core("renderer_authorize_core_script"),
    awaiting,
  );
  const started = vm.runInContext(fenced(rendererScript, active), awaiting);
  vm.runInContext(core("renderer_release_fence_core_script"), awaiting);
  vm.runInContext("finish()", awaiting);
  assert.equal((await started).cancelled, true);
  assert.equal(
    vm.runInContext(`window['${patchKey}']`, awaiting),
    undefined,
    "real shipped activation must not recreate state after awaiting legacy disposal",
  );
  console.log(
    JSON.stringify({
      lateFirstInjectionFenced: true,
      freshDocumentActivated: true,
      awaitingActivationFenced: true,
    }),
  );
  const drain = await vm.runInNewContext(`(async () => {
${setup}
const hydration = rehydrateStaleHistoryLineages(manager);
state.historyLineageHydrationPromise = hydration;
let released = false;
const disposal = Promise.resolve(state.dispose()).then(() => { released = true; });
await Promise.resolve();
const releasedBeforeRequestSettled = released;
finishResume();
await hydration;
await disposal;
return { releasedBeforeRequestSettled, completeLoads, mutations, conversation };
})()`);
  console.log(JSON.stringify({ drain }));
  assert.equal(
    drain.releasedBeforeRequestSettled,
    false,
    "native release must drain an already issued history resume",
  );
  assert.equal(
    drain.completeLoads,
    0,
    "disposed history work must not issue a new complete-history request",
  );

  const cancelled = await vm.runInNewContext(`(async () => {
${setup}
await state.dispose();
const hydration = rehydrateStaleHistoryLineages(manager);
await Promise.resolve();
if (finishResume) finishResume();
await hydration;
return { mutations, completeLoads };
})()`);
  console.log(JSON.stringify({ cancelled }));
  assert.equal(
    cancelled.mutations,
    0,
    "disposed renderer must not mutate a cached conversation",
  );
  assert.equal(cancelled.completeLoads, 0);

  const preserve = await vm.runInNewContext(
    `(async () => {
${setup}
const hydration = rehydrateStaleHistoryLineages(manager);
state.historyLineageHydrationPromise = hydration;
const disposal = state.dispose();
const restoredBeforeDrain = conversation.rolloutPath === 'old.jsonl' && conversation.turns[0]?.id === 'synthetic-turn';
const mutationsAtNativeUpdate = mutations;
conversation.turns = [{ id: 'new-native-turn' }];
conversation.resumeState = 'native-resumed';
conversation.rolloutPath = 'native.jsonl';
const nativeWhilePending = JSON.stringify(conversation);
await new Promise(resolve => setTimeout(resolve, 4100));
const preservedPastCdpTimeout = JSON.stringify(conversation) === nativeWhilePending;
finishResume();
await hydration;
await disposal;
return { conversation, completeLoads, restoredBeforeDrain, mutationsAtNativeUpdate, mutations, preservedPastCdpTimeout };
})()`,
    { setTimeout },
  );
  assert.deepEqual(JSON.parse(JSON.stringify(preserve.conversation.turns)), [
    { id: "new-native-turn" },
  ]);
  assert.equal(preserve.conversation.resumeState, "native-resumed");
  assert.equal(preserve.conversation.rolloutPath, "native.jsonl");
  assert.equal(
    preserve.restoredBeforeDrain,
    true,
    "owned history must be restored synchronously before a possible CDP timeout",
  );
  assert.equal(
    preserve.mutations,
    preserve.mutationsAtNativeUpdate,
    "late RPC settlement cannot apply a delayed owned rollback",
  );
  assert.equal(
    preserve.preservedPastCdpTimeout,
    true,
    "native state must remain stable while a request outlives the 4 second CDP timeout",
  );
  assert.equal(preserve.completeLoads, 0);
  console.log(
    JSON.stringify({
      preservedNativeChanges: true,
      pendingBeyondCdpTimeout: true,
    }),
  );

  const context = vm.createContext({ setTimeout, performance });
  vm.runInContext(
    `
globalThis.window = globalThis;
const listeners = new Set();
globalThis.addEventListener = (_, listener) => listeners.add(listener);
globalThis.removeEventListener = (_, listener) => listeners.delete(listener);
const originalDispatch = () => true;
globalThis.dispatchEvent = originalDispatch;
let timers = 0;
globalThis.setInterval = () => ++timers;
globalThis.clearInterval = () => --timers;
let refreshStarted = false;
let finishRefresh;
let resumes = 0;
let mutations = 0;
const originalSend = async (_method, params) => params;
const client = { sendRequest: originalSend, getCompatibleThreadSortKey() {}, supportsPaginatedThreadHistory() {} };
const manager = {
  requestClient: client,
  threadStore: { threadsById: new Map([['synthetic-thread', { path: 'new.jsonl' }]]), refreshRecentConversations: () => { refreshStarted = true; return new Promise(resolve => { finishRefresh = resolve; }); } },
  getCachedConversations: () => [{ id: 'synthetic-thread', rolloutPath: 'old.jsonl', turns: [] }],
  getConversation: () => ({ id: 'synthetic-thread', rolloutPath: 'old.jsonl', turns: [] }),
  updateConversationState: () => { mutations += 1; },
  setConversation() {},
  resumeConversationForUnavailableOwner: async () => { resumes += 1; return { status: 'ready' }; },
};
const root = { '__reactFiber$synthetic': { manager }, childNodes: [] };
globalThis.document = { body: root, documentElement: root, scripts: [], getElementById: () => root, querySelectorAll: () => [] };
const seed = () => globalThis['${patchKey}'] = { lifecycleVersion: '2', modulePromises: new Map([
  ['app-server-manager-signals-', Promise.resolve({ client })],
  ['rpc-', Promise.resolve({ appServices: { localThreadCatalog: { requestStartupSync: async () => {}, readStatus: async () => ({ hosts: [{ hostId: 'local', isComplete: true }] }) } } })],
]) };
seed();
`,
    context,
  );
  const installation = vm.runInContext(rendererScript, context);
  for (
    let index = 0;
    index < 100 && !vm.runInContext("refreshStarted", context);
    index += 1
  )
    await new Promise(setImmediate);
  assert.equal(
    vm.runInContext("refreshStarted", context),
    true,
    "fixture must pause the real React history refresh",
  );
  const params = await vm.runInContext(
    `client.sendRequest('thread/resume', { model: 'official-model', modelProvider: 'openai' })`,
    context,
  );
  assert.equal(
    params.modelProvider,
    "custom",
    "managed fixture must actually install provider normalization",
  );
  let released = false;
  const release = vm.runInContext(releaseScript, context).then((value) => {
    released = true;
    return value;
  });
  await Promise.resolve();
  assert.equal(
    released,
    false,
    "release must wait for the React history refresh",
  );
  vm.runInContext("finishRefresh()", context);
  await installation;
  const releaseResult = await release;
  assert.equal(releaseResult.incomplete, false);
  const closed = JSON.parse(
    vm.runInContext(
      `JSON.stringify({ resumes, mutations, timers, listeners: listeners.size, restoredSend: client.sendRequest === originalSend, restoredDispatch: dispatchEvent === originalDispatch })`,
      context,
    ),
  );
  assert.deepEqual(closed, {
    resumes: 0,
    mutations: 0,
    timers: 0,
    listeners: 0,
    restoredSend: true,
    restoredDispatch: true,
  });
  const nativeParams = await vm.runInContext(
    `client.sendRequest('thread/resume', { model: 'official-model', modelProvider: 'openai' })`,
    context,
  );
  assert.equal(nativeParams.modelProvider, "openai");
  console.log(
    JSON.stringify({
      fullRendererAsyncRelease: closed,
      nativeProviderPreserved: true,
    }),
  );

  const failure = await vm.runInNewContext(`(async () => {
${setup}
state.historyRestorations.add(() => { throw new Error('synthetic restore failure'); });
return await ${releaseScript};
})()`);
  assert.equal(
    failure.incomplete,
    true,
    "failed owned restoration must reach the Rust recovery warning path",
  );
  const legacy = await vm.runInNewContext(`(async () => {
const window = globalThis;
const clearInterval = () => {};
window.__ccSwitchCodexAppCompatibilityV9 = { payload: { modelNames: ['old-model'] } };
return await ${releaseScript};
})()`);
  assert.equal(
    legacy.incomplete,
    true,
    "untracked legacy hooks cannot be reported as fully released",
  );
  console.log(
    JSON.stringify({
      incompleteRestorationReported: true,
      legacyReleaseReported: true,
    }),
  );

  for (const [preserve, cancel] of [
    [false, false],
    [true, false],
    [false, true],
  ]) {
    let embedded = template("run_renderer_history_disposal_probe");
    const values = {
      MODEL_PICKER_PATCH_KEY: patchKey,
      lifecycle: core("model_picker_lifecycle_core_script"),
      history: core("history_lineage_hydration_core_script"),
      preserve_native_update: String(preserve),
      cancel_before_start: String(cancel),
    };
    for (const [name, value] of Object.entries(values))
      embedded = embedded.replaceAll(`{${name}}`, value);
    const rustContext = vm.createContext({});
    await vm.runInContext(embedded, rustContext);
    const result = JSON.parse(
      vm.runInContext("globalThis.__rendererHistoryDisposeResult", rustContext),
    );
    assert.equal(result.completeLoads, 0);
    if (cancel) assert.equal(result.mutations, 0);
    else assert.equal(result.releasedBeforeRequestSettled, false);
    if (preserve)
      assert.equal(result.conversation.resumeState, "native-resumed");
  }
  let embeddedDispose = template(
    "native_official_dispose_restores_owned_renderer_hooks_and_model_changes",
  );
  for (const [name, value] of Object.entries({
    MODEL_PICKER_PATCH_KEY: patchKey,
    lifecycle: core("model_picker_lifecycle_core_script"),
    model_core: core("model_picker_patch_core_script"),
    release: releaseScript,
  }))
    embeddedDispose = embeddedDispose.replaceAll(`{${name}}`, value);
  const rustDisposeContext = vm.createContext({});
  await vm.runInContext(embeddedDispose, rustDisposeContext);
  const restored = JSON.parse(
    vm.runInContext("globalThis.__rendererDisposeResult", rustDisposeContext),
  );
  assert.equal(restored.removedState, true);
  assert.equal(restored.changedAfterDispose, false);
  console.log(JSON.stringify({ embeddedRustRegressionFixtures: true }));
}

const watchdog = setTimeout(() => {
  console.error(
    "Renderer disposal regression did not finish within 15 seconds",
  );
  process.exit(1);
}, 15_000);
main()
  .catch((error) => {
    console.error(error);
    process.exitCode = 1;
  })
  .finally(() => clearTimeout(watchdog));
