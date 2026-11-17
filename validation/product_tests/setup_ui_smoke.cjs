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
            if (args.action === 'start') { window.chatgptAttempt = true; return { loginId: 'test-login', authUrl: 'https://auth.openai.com/test-login' }; }
            if (args.action === 'logout' || args.action === 'cancel') { window.chatgptSignedIn = false; window.chatgptAttempt = false; return {}; }
            return { account: window.chatgptSignedIn ? { type: 'chatgpt', email: 'tester@example.com', planType: 'plus' } : null, login: window.chatgptAttempt ? { loginId: 'test-login' } : null };
          }
          if (command === 'plugin:http|fetch') { if (window.modelFails) throw new Error('Model login expired'); window.modelBodyRead = false; return 1; }
          if (command === 'plugin:http|fetch_send') return { status: 200, statusText: 'OK', url: 'https://model.test/v1/chat/completions', headers: [['content-type', 'text/event-stream']], rid: 2 };
          if (command === 'plugin:http|fetch_read_body') {
            if (window.modelBodyRead) return [1];
            window.modelBodyRead = true;
            return [...new TextEncoder().encode('data: {"choices":[{"delta":{"content":"ATHENAEUM_TEST_OK"}}]}\n\ndata: [DONE]\n\n'), 0];
          }
          if (command === 'plugin:opener|open_url') { if (window.browserFails) throw new Error('Browser unavailable'); return null; }
          if (command === 'plugin:store|load') return 1;
          if (command === 'plugin:store|get') return [args.key === 'recentProjects' ? [{ id: 'test', name: 'Test project', path: 'C:/test-project' }] : null, true];
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
            if (job === 'library_sign_in') { if (window.libraryFails) return { exitCode: 1, output: { error: { message: 'Invalid sign-in. Try again.' } }, stderr: [] }; window.signedIn = true; return { exitCode: 0, output: { username: 'tester' }, stderr: [] }; }
            if (job === 'library_logout') { window.signedIn = false; return { exitCode: 0, output: {}, stderr: [] }; }
          }
          return null;
        },
      };
    });
    await page.goto(`${process.env.SETUP_UI_BASE || 'http://localhost:1420'}/__setup_test__`);
    await page.evaluate(async () => {
      const runtime = await import('/src/test-helpers/setup-runtime.ts');
      window.React = runtime.React;
      window.root = runtime.createRoot(document.getElementById('root'));
      window.wiki = (await import('/src/stores/wiki-store.ts')).useWikiStore;
      window.setup = (await import('/src/stores/setup-store.ts')).useSetupStore;
      window.mount = async (file, name, props = {}) => {
        window.root.render(window.React.createElement((await import(file))[name], props));
      };
      await window.mount('/src/components/project/first-launch.tsx', 'FirstLaunch', { onComplete: async () => { window.completed = true; window.completionCount = (window.completionCount || 0) + 1; } });
    });
    await page.getByRole('heading', { name: 'Sign in to Athenaeum', exact: true }).waitFor();
    await page.getByLabel('Username or email').waitFor();
    assert.equal(await page.getByRole('button', { name: /Start locally|Quick start|Connect an account|Open project/ }).count(), 0);
    await page.screenshot({ animations: 'disabled', path: path.join(process.env.TEMP || '/tmp', 'athenaeum-sign-in.png') });
    await page.getByLabel('Username or email').fill('tester');
    await page.getByLabel('Password', { exact: true }).fill('test-only-password');
    await page.evaluate(() => { window.libraryFails = true; });
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: 'Invalid sign-in' }).waitFor();
    assert.equal(await page.evaluate(() => !!window.completed), false);
    await page.evaluate(() => { window.libraryFails = false; });
    await page.getByLabel('Password', { exact: true }).fill('test-only-password');
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
    await page.waitForFunction(() => window.completed === true);
    assert.equal(await page.evaluate(() => window.completionCount), 1);
    const signIn = await page.evaluate(() => window.calls.find(call => call.args?.job?.job === 'library_sign_in'));
    assert.deepEqual(signIn.args.job, { job: 'library_sign_in', server: 'https://wiki.hanoryx.com' });
    assert.equal(signIn.args.credentials.password, 'test-only-password');
    await page.evaluate(async () => {
      await window.mount('/src/components/project/welcome-screen.tsx', 'WelcomeScreen', {
        onCreateProject() {}, onOpenProject() {}, onSelectProject() {},
      });
    });
    await page.getByRole('heading', { name: 'Open a project' }).waitFor();
    await page.screenshot({ animations: 'disabled', path: path.join(process.env.TEMP || '/tmp', 'athenaeum-project-picker.png') });
    await page.evaluate(async () => {
      window.wiki.setState({ project: { id: 'test', name: 'Test project', path: 'C:/test-project' } });
      await window.mount('/src/components/setup/connectors-view.tsx', 'ConnectorsView');
    });
    const connect = page.getByRole('button', { name: 'Connect with ChatGPT', exact: true });
    await connect.waitFor();
    assert.equal(await page.getByRole('button', { name: /Next|Add agent|Check account|Check connections/ }).count(), 0);
    await connect.click();
    await page.getByText('Finish signing in in your browser.', { exact: true }).waitFor();
    assert.equal(await page.evaluate(() => window.calls.some(call => call.command === 'plugin:opener|open_url' && call.args.url === 'https://auth.openai.com/test-login')), true);
    await page.evaluate(() => { window.chatgptSignedIn = true; });
    await page.getByText('tester@example.com', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'Open chat', exact: true }).click();
    assert.equal(await page.evaluate(() => window.wiki.getState().activeView), 'chat');
    assert.equal(await page.evaluate(() => JSON.parse(localStorage.getItem('athenaeum.ui:chat-agent-runtime'))), 'chatgpt');
    await page.getByRole('button', { name: 'Sign out', exact: true }).click();
    await page.evaluate(() => { window.browserFails = true; });
    await connect.click();
    await page.getByRole('alert').filter({ hasText: 'Could not open your browser' }).waitFor();
    assert.equal(await page.getByRole('link', { name: 'https://auth.openai.com/test-login', exact: true }).count(), 1);
    await page.getByRole('button', { name: 'Cancel', exact: true }).click();
    await connect.waitFor();
    await page.evaluate(() => { window.browserFails = false; });
    await connect.click();
    await page.getByText('Finish signing in in your browser.', { exact: true }).waitFor();
    await page.evaluate(() => { window.chatgptAttempt = false; });
    await page.getByRole('alert').filter({ hasText: 'Sign-in expired' }).waitFor();
    await connect.waitFor();

    await page.getByText('External agents', { exact: true }).click();
    await page.evaluate(() => { window.failSetup = true; });
    await page.getByRole('button', { name: 'Connect Claude Code', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: 'Could not connect' }).waitFor();
    await page.evaluate(() => { window.failSetup = false; });
    await page.getByRole('button', { name: 'Connect Claude Code', exact: true }).click();
    await page.getByRole('button', { name: 'Reconnect Claude Code', exact: true }).waitFor();
    await page.getByRole('button', { name: 'Connect Cursor', exact: true }).click();
    await page.getByRole('button', { name: 'Reconnect Cursor', exact: true }).waitFor();
    assert.equal(await page.evaluate(() => window.calls.some(call => call.args?.job?.job === 'sync_link')), false);
    await page.screenshot({ animations: 'disabled', path: path.join(process.env.TEMP || '/tmp', 'athenaeum-connectors.png') });
    await page.setViewportSize({ width: 720, height: 480 });
    await page.evaluate(() => document.documentElement.classList.add('dark'));
    await page.screenshot({ animations: 'disabled', path: path.join(process.env.TEMP || '/tmp', 'athenaeum-connectors-small-dark.png') });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
    await page.setViewportSize({ width: 1200, height: 800 });
    await page.getByRole('button', { name: 'AI model', exact: true }).click();
    await page.getByRole('heading', { name: 'AI model', exact: true }).waitFor();
    await page.evaluate(() => window.wiki.getState().setLlmConfig({ ...window.wiki.getState().llmConfig, provider: 'custom', model: 'test-model', apiKey: 'test-only', customEndpoint: 'https://model.test/v1/chat/completions' }));
    await page.getByRole('button', { name: 'Check model', exact: true }).click();
    await page.getByText('Model responded successfully.', { exact: true }).waitFor();
    await page.evaluate(() => { window.modelFails = true; });
    await page.getByRole('button', { name: 'Check model', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: 'Model check failed' }).waitFor();
    assert.equal(await page.getByText('Model responded successfully.', { exact: true }).count(), 0);
    await page.evaluate(async () => {
      await window.mount('/src/components/setup/library-account-menu.tsx', 'LibraryAccountMenu');
    });
    await page.getByRole('button', { name: 'Library account: tester', exact: true }).click();
    await page.getByText('Signed in as tester', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'Sign out', exact: true }).click();
    await page.getByLabel('Username or email').fill('tester');
    await page.getByLabel('Password', { exact: true }).fill('test-only-password');
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
    await page.getByText('Signed in as tester', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'Close account menu', exact: true }).click();
    await page.evaluate(async () => {
      const { useProposalStore } = await import('/src/stores/proposal-store.ts');
      const { useEngineStore } = await import('/src/stores/engine-store.ts');
      useEngineStore.setState({ state: { kind: 'ready' } });
      useProposalStore.setState({ workspace: 'C:/test-project', proposals: [{ node: {
        id: 'pending', title: 'Validate callback state', summary: 'Validate the state before creating a session.',
        content: 'Keep state validation before session creation.', provenance: { detail: 'athenaeum-background-memory' },
      }, evidence: [] }], load: async () => {}, reject: async id => { window.discarded = id; } });
      await window.mount('/src/components/review/proposal-queue.tsx', 'ProposalQueue');
    });
    await page.getByText('Waiting for source evidence', { exact: true }).waitFor();
    assert.equal(await page.getByRole('button', { name: /Accept/ }).count(), 0);
    assert.equal(await page.getByRole('checkbox', { name: /Automatically save/ }).count(), 0);
    await page.screenshot({ animations: 'disabled', path: path.join(process.env.TEMP || '/tmp', 'athenaeum-pending-pages.png') });
    await page.getByRole('button', { name: 'Discard', exact: true }).click();
    assert.equal(await page.evaluate(() => window.discarded), 'pending');
    assert.deepEqual(errors, []);
    console.log('Setup UI passed: single sign-in and automatic completion; login failure/retry; direct browser OAuth, callback, cancellation, browser failure and expiry; direct external-agent failure/retry; model checks; account persistence; small-window light/dark layout.');
  } catch (error) {
    if (page) console.error("Setup fixture at failure:", (await page.locator("body").innerText()).slice(0, 4000));
    throw error;
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
