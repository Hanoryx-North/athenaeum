/* Browser interaction checks for the real setup components with a simulated Tauri boundary.
 * Start a fresh Vite server (manual fixture imports must share its module graph), then: node product_tests/setup_ui_smoke.cjs
 * PLAYWRIGHT_MODULE can point to a temporary Playwright installation.
 * This does not claim to test the native webview or a real account.
 */
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
const path = require('node:path');

(async () => {
  const browser = await chromium.launch({
    headless: true,
    ...(process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE } : {}),
  });
  let page;
  try {
    page = await browser.newPage({ viewport: { width: 1200, height: 800 } });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.route('**/__setup_test__', route => route.fulfill({ contentType: 'text/html', body: `
      <html style="height:100%"><head><link rel="stylesheet" href="/src/index.css"></head><body style="height:100%"><div id="root" style="height:100%"></div>
      <script type="module">import RefreshRuntime from '/@react-refresh'; RefreshRuntime.injectIntoGlobalHook(window);
      window.$RefreshReg$ = () => {}; window.$RefreshSig$ = () => type => type;
      window.__vite_plugin_react_preamble_installed__ = true;</script></body></html>` }));
    await page.addInitScript(() => {
      window.calls = [];
      window.connected = false;
      window.failSetup = false;
      window.signedIn = false;
      window.connectedAgents = [];
      window.modelFails = false;
      window.modelBodyRead = false;
      window.chatgptSignedIn = false;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      window.__TAURI_INTERNALS__ = {
        transformCallback: () => 1,
        unregisterCallback() {},
        invoke: async (command, args) => {
          window.calls.push({ command, args });
          if (command === 'chatgpt_account') {
            if (args.action === 'start') return { loginId: 'test-login', authUrl: 'https://auth.openai.com/test-login' };
            if (args.action === 'logout' || args.action === 'cancel') { window.chatgptSignedIn = false; return {}; }
            return { account: window.chatgptSignedIn ? { type: 'chatgpt', email: 'tester@example.com', planType: 'plus' } : null };
          }
          if (command === 'plugin:http|fetch') { if (window.modelFails) throw new Error('Model login expired'); window.modelBodyRead = false; return 1; }
          if (command === 'plugin:http|fetch_send') return { status: 200, statusText: 'OK', url: 'https://model.test/v1/chat/completions', headers: [['content-type', 'text/event-stream']], rid: 2 };
          if (command === 'plugin:http|fetch_read_body') {
            if (window.modelBodyRead) return [1];
            window.modelBodyRead = true;
            return [...new TextEncoder().encode('data: {"choices":[{"delta":{"content":"ATHENAEUM_TEST_OK"}}]}\n\ndata: [DONE]\n\n'), 0];
          }
          if (command === 'cli_status') return { found: true, path: '/test/wiki', version: 'test', agentInstalled: false };
          if (command === 'cli_run') {
            const job = args.job.job;
            if (job === 'doctor') return { exitCode: window.connected ? 0 : 1, output: { ok: window.connected, checks: ['athenaeum', 'codex', 'claude', 'cursor'].map(id => ({ id: `agent-${id}`, ok: window.connectedAgents.includes(id), detail: window.connectedAgents.includes(id) ? 'connected' : 'not connected' })) }, stderr: [] };
            if (job === 'setup') {
              window.connected = !window.failSetup;
              if (window.connected) window.connectedAgents = [...new Set([...window.connectedAgents, ...args.job.agents])];
              return { exitCode: window.connected ? 0 : 1, output: { ok: window.connected, steps: [{ id: 'agents', status: window.connected ? 'ok' : 'failed', detail: window.connected ? 'connected' : 'Cannot write agent settings' }], next: [] }, stderr: [] };
            }
            if (job === 'library_status') return { exitCode: 0, output: window.signedIn ? [{ username: 'tester', detail: null }] : [], stderr: [] };
            if (job === 'library_sign_in') { window.signedIn = true; return { exitCode: 0, output: { username: 'tester' }, stderr: [] }; }
            if (job === 'library_logout') { window.signedIn = false; return { exitCode: 0, output: {}, stderr: [] }; }
          }
          return null;
        },
      };
    });
    await page.goto(`${process.env.SETUP_UI_BASE || 'http://127.0.0.1:1420'}/__setup_test__`);
    await page.evaluate(async () => {
      const runtime = await import('/src/test-helpers/setup-runtime.ts');
      window.React = runtime.React;
      window.root = runtime.createRoot(document.getElementById('root'));
      window.wiki = (await import('/src/stores/wiki-store.ts')).useWikiStore;
      window.setup = (await import('/src/stores/setup-store.ts')).useSetupStore;
      window.mount = async (file, name, props = {}) => {
        window.root.render(window.React.createElement((await import(file))[name], props));
      };
      await window.mount('/src/components/project/first-launch.tsx', 'FirstLaunch', { onComplete: async () => { window.completed = true; } });
    });
    await page.getByRole('heading', { name: 'Welcome to Athenaeum', exact: true }).waitFor();
    await page.getByLabel('Username or email').waitFor();
    assert.equal(await page.getByText(/device code|approve this device/i).count(), 0);
    await page.getByLabel('Username or email').fill('tester');
    await page.getByLabel('Password', { exact: true }).fill('test-only-password');
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
    await page.getByText('Signed in as tester').waitFor();
    const signIn = await page.evaluate(() => window.calls.find(call => call.args?.job?.job === 'library_sign_in'));
    assert.deepEqual(signIn.args.job, { job: 'library_sign_in', server: 'https://wiki.hanoryx.com' });
    assert.equal(signIn.args.credentials.password, 'test-only-password');
    await page.getByRole('button', { name: 'Open Athenaeum', exact: true }).click();
    assert.equal(await page.evaluate(() => window.completed), true);

    await page.evaluate(async () => {
      window.wiki.setState({ project: { id: 'test', name: 'Test project', path: 'C:/test-project' } });
      await window.mount('/src/components/setup/connectors-view.tsx', 'ConnectorsView');
    });
    await page.getByRole('button', { name: 'Add agent', exact: true }).click();
    await page.getByRole('radio', { name: /ChatGPT/ }).click();
    await page.screenshot({ path: path.join(process.env.TEMP || '.', 'athenaeum-chatgpt-picker.png') });
    await page.getByRole('button', { name: 'Next', exact: true }).click();
    await page.getByRole('heading', { name: 'Connect ChatGPT', exact: true }).waitFor();
    await page.getByRole('button', { name: 'Sign in with ChatGPT', exact: true }).waitFor();
    assert.equal(await page.getByRole('button', { name: 'Connect ChatGPT', exact: true }).isDisabled(), true);
    await page.getByRole('button', { name: 'Sign in with ChatGPT', exact: true }).click();
    await page.getByText(/Finish signing in with ChatGPT in your browser/).waitFor();
    await page.evaluate(() => { window.chatgptSignedIn = true; });
    await page.getByText(/Signed in as tester@example.com/).waitFor();
    await page.screenshot({ path: path.join(process.env.TEMP || '.', 'athenaeum-chatgpt-connected.png') });
    await page.getByRole('button', { name: 'Connect ChatGPT', exact: true }).click();
    await page.getByRole('heading', { name: 'ChatGPT is connected.' }).waitFor();
    assert.deepEqual(await page.evaluate(() => window.calls.find(call => call.args?.job?.agents?.includes('athenaeum')).args.job.agents), ['athenaeum']);
    await page.getByRole('button', { name: 'Open chat', exact: true }).click();
    assert.equal(await page.evaluate(() => window.wiki.getState().activeView), 'chat');
    assert.equal(await page.evaluate(() => JSON.parse(localStorage.getItem('athenaeum.ui:chat-agent-runtime'))), 'chatgpt');
    await page.evaluate(async () => { window.chatgptSignedIn = false; window.connectedAgents = []; window.connected = false; window.setup.setState({ doctor: { ok: true, checks: [] } }); (await import('/src/stores/chatgpt-store.ts')).useChatgptStore.setState({ account: null, workspace: null, login: null }); });
    await page.getByRole('button', { name: 'Add agent', exact: true }).click();
    await page.getByRole('radio', { name: /Athenaeum Agent/ }).click();
    await page.getByRole('button', { name: 'Next', exact: true }).click();
    await page.getByRole('heading', { name: 'Connect Athenaeum Agent', exact: true }).waitFor();
    await page.evaluate(() => { window.failSetup = true; });
    await page.getByRole('button', { name: 'Connect Athenaeum Agent', exact: true }).click();
    await page.getByRole('alert').waitFor();
    assert.equal(await page.getByRole('button', { name: 'Finish', exact: true }).count(), 0);
    await page.evaluate(() => { window.failSetup = false; });
    await page.getByRole('button', { name: 'Connect Athenaeum Agent', exact: true }).click();
    await page.getByRole('heading', { name: 'Athenaeum Agent is connected.' }).waitFor();
    await page.getByRole('button', { name: 'Finish', exact: true }).click();
    await page.getByRole('list', { name: 'Connected coding agents' }).waitFor();
    assert.equal(await page.getByRole('radio', { name: /Athenaeum Agent/ }).count(), 0);
    assert.equal(await page.getByText('Connected to this wiki', { exact: true }).count(), 1);
    await page.getByRole('button', { name: 'Add agent', exact: true }).click();
    await page.getByRole('radio', { name: /Claude Code/ }).click();
    await page.getByRole('button', { name: 'Next', exact: true }).click();
    await page.getByRole('button', { name: 'Connect Claude Code', exact: true }).click();
    await page.getByRole('button', { name: 'Finish', exact: true }).click();
    assert.equal(await page.getByText('Connected to this wiki', { exact: true }).count(), 2);
    await page.getByRole('button', { name: 'Reconnect Athenaeum Agent', exact: true }).click();
    await page.getByRole('heading', { name: 'Connect Athenaeum Agent', exact: true }).waitFor();
    await page.getByRole('button', { name: 'Coding agents', exact: true }).last().click();
    assert.equal(await page.evaluate(() => window.calls.some(call => call.args?.job?.job === 'sync_link')), false);
    await page.getByRole('button', { name: 'Library account', exact: true }).click();
    await page.getByRole('heading', { name: 'Your Library account' }).waitFor();
    assert.equal(await page.getByRole('radio', { name: /Athenaeum Agent/ }).count(), 0);
    await page.getByRole('button', { name: 'AI model', exact: true }).click();
    await page.getByRole('heading', { name: 'AI model', exact: true }).waitFor();
    assert.equal(await page.getByRole('heading', { name: 'Your Library account' }).count(), 0);
    await page.evaluate(() => window.wiki.getState().setLlmConfig({ ...window.wiki.getState().llmConfig, provider: 'custom', model: 'test-model', apiKey: 'test-only', customEndpoint: 'https://model.test/v1/chat/completions' }));
    await page.getByRole('button', { name: 'Check model', exact: true }).click();
    await page.getByText('Model responded successfully.', { exact: true }).waitFor();
    await page.evaluate(() => { window.modelFails = true; });
    await page.getByRole('button', { name: 'Check model', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: 'Model check failed' }).waitFor();
    assert.equal(await page.getByText('Model responded successfully.', { exact: true }).count(), 0);
    await page.evaluate(() => window.wiki.getState().setLlmConfig({ ...window.wiki.getState().llmConfig, model: 'changed-model' }));
    await page.getByText(/Not tested yet/).waitFor();

    // Project navigation is restored independently for each project, including
    // a selected wiki file, without retaining any content from another project.
    await page.evaluate(() => {
      const project = window.wiki.getState().project;
      window.wiki.getState().setActiveView('graph');
      window.wiki.getState().setSelectedFile('C:/test-project/wiki/index.md');
      window.wiki.getState().setProject({ id: 'other', name: 'Other', path: 'C:/other-project' });
      if (window.wiki.getState().selectedFile !== null) throw new Error('File leaked into another project');
      window.wiki.getState().setProject(project);
      if (window.wiki.getState().activeView !== 'graph' || window.wiki.getState().selectedFile !== 'C:/test-project/wiki/index.md') throw new Error('Navigation was not restored');
    });

    // The account trigger opens its current popover and preserves the saved identity.
    await page.evaluate(async () => {
      await window.mount('/src/components/setup/library-account-menu.tsx', 'LibraryAccountMenu');
    });
    await page.getByRole('button', { name: 'Library account: tester', exact: true }).waitFor();
    assert.deepEqual(errors, []);
    await page.getByRole('button', { name: 'Library account: tester', exact: true }).click();
    await page.getByRole('heading', { name: 'Your Library account', exact: true }).waitFor();
    await page.getByText('Signed in as tester', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'Sign out', exact: true }).click();
    await page.getByLabel('Username or email').fill('tester');
    await page.getByLabel('Password', { exact: true }).fill('test-only-password');
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
    await page.getByText('Signed in as tester', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'Close account menu', exact: true }).click();
    await page.getByRole('button', { name: 'Library account: tester', exact: true }).waitFor();

    await page.evaluate(async () => {
      const { useEngineStore } = await import('/src/stores/engine-store.ts');
      useEngineStore.setState({ library: null, libraryState: { kind: 'unlinked' }, loadLibrary: async () => {}, refresh: async () => {} });
      await window.mount('/src/components/knowledge/library-panel.tsx', 'LibraryPanel');
    });
    await page.getByRole('button', { name: 'Next', exact: true }).click();
    await page.getByRole('heading', { name: 'Ready to publish?' }).waitFor();
    assert.equal(await page.getByRole('radio', { name: /Athenaeum Agent/ }).count(), 0);
    assert.equal(await page.evaluate(() => window.calls.some(call => call.args?.job?.job === 'sync_link')), false);
    await page.getByRole('button', { name: 'Back', exact: true }).click();
    await page.getByRole('heading', { name: 'Publish this wiki', exact: true }).waitFor();
    await page.screenshot({ path: path.join(process.env.TEMP || '.', 'athenaeum-publish.png') });
    await page.evaluate(async () => { await window.mount('/src/components/setup/connectors-view.tsx', 'ConnectorsView'); });
    await page.getByRole('button', { name: 'Coding agents', exact: true }).click();
    await page.getByRole('list', { name: 'Connected coding agents' }).waitFor();
    await page.screenshot({ path: path.join(process.env.TEMP || '.', 'athenaeum-connectors.png') });
    await page.getByRole('button', { name: 'Add agent', exact: true }).click();
    await page.getByRole('radio', { name: /Cursor/ }).click();
    await page.getByRole('button', { name: 'Next', exact: true }).click();
    await page.getByRole('button', { name: 'AI model', exact: true }).click();
    await page.getByRole('button', { name: 'Coding agents', exact: true }).first().click();
    await page.getByRole('heading', { name: 'Connect Cursor', exact: true }).waitFor();
    await page.getByRole('button', { name: 'Back', exact: true }).click();
    await page.setViewportSize({ width: 720, height: 480 });
    const next = page.getByRole('button', { name: 'Next', exact: true });
    assert.equal(await next.isVisible(), true);
    const bounds = await next.boundingBox();
    assert.ok(bounds && bounds.y >= 0 && bounds.y + bounds.height <= 480, 'Next must stay in view in a small window');
    await next.click();
    await page.reload();
    await page.evaluate(async () => {
      const React = (await import('/node_modules/.vite/deps/react.js')).default;
      const root = (await import('/node_modules/.vite/deps/react-dom_client.js')).default.createRoot(document.getElementById('root'));
      const wiki = (await import('/src/stores/wiki-store.ts')).useWikiStore;
      wiki.getState().setProject({ id: 'test', name: 'Test project', path: 'C:/test-project' });
      if (wiki.getState().activeView !== 'graph' || wiki.getState().selectedFile !== 'C:/test-project/wiki/index.md') throw new Error('Navigation did not survive reload');
      root.render(React.createElement((await import('/src/components/setup/connectors-view.tsx')).ConnectorsView));
    });
    await page.getByRole('heading', { name: 'Connect Cursor', exact: true }).waitFor();
    assert.equal(await page.evaluate(() => window.calls.some(call => call.args?.job?.job === 'setup')), false, 'Restoring a draft must not reconnect automatically');
    assert.deepEqual(errors, []);
    console.log('Setup UI checks passed: bottom account menu sign-in/out, Next/Back, failure/retry, Finish returns to connected agents, multiple agents/reconnect, saved wizard draft, model success/failure/config changes, per-project navigation and reload restoration, publishing separation, and small-window navigation.');
  } catch (error) {
    if (page) console.error("Setup fixture at failure:", (await page.locator("body").innerText()).slice(0, 4000));
    throw error;
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
