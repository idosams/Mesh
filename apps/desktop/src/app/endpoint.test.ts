// Where the window looks for the background service, in the order it looks.
//
// The rules are duplicated in `crates/mesh-daemon/examples/serve.rs`, which is safe for exactly
// one reason: they resolve ONE path, and a path that disagrees fails loudly and immediately — the
// window says the service is not running while the service says it is. This file pins the rules on
// this side so a change here is a change somebody had to write down.

import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { DEFAULT_ENDPOINT_SEGMENTS, ENDPOINT_VARIABLE, namedArgument, resolveEndpoint } from './endpoint.ts';

describe('where the window looks for the background service', () => {
  it('takes --endpoint over everything else', () => {
    const env = { HOME: '/home/person', [ENDPOINT_VARIABLE]: '/from/the/environment.sock' };
    assert.equal(resolveEndpoint(['--endpoint', '/asked/for.sock'], env), '/asked/for.sock');
  });

  it('takes the environment variable over the default', () => {
    const env = { HOME: '/home/person', [ENDPOINT_VARIABLE]: '/from/the/environment.sock' };
    assert.equal(resolveEndpoint([], env), '/from/the/environment.sock');
  });

  it('falls back to a predictable path under the home directory', () => {
    assert.equal(resolveEndpoint([], { HOME: '/home/person' }), `/home/person/${DEFAULT_ENDPOINT_SEGMENTS.join('/')}`);
  });

  it('does not double a separator when the home directory carries one', () => {
    assert.equal(resolveEndpoint([], { HOME: '/home/person/' }), '/home/person/.mesh/run/daemon.sock');
  });

  it('uses the temporary directory when there is no home at all', () => {
    assert.equal(resolveEndpoint([], { TMPDIR: '/scratch' }), '/scratch/.mesh/run/daemon.sock');
    assert.equal(resolveEndpoint([], {}), '/tmp/.mesh/run/daemon.sock');
  });

  it('ignores an environment variable that is set to nothing', () => {
    assert.equal(resolveEndpoint([], { HOME: '/home/person', [ENDPOINT_VARIABLE]: '' }), '/home/person/.mesh/run/daemon.sock');
  });

  it('refuses an option with no value rather than swallowing the next option', () => {
    assert.throws(() => resolveEndpoint(['--endpoint'], {}), /needs a path/);
    assert.throws(() => resolveEndpoint(['--endpoint', '--once'], {}), /needs a path/);
    assert.throws(() => namedArgument(['--session'], '--session'), /needs a value/);
    assert.equal(namedArgument(['--once'], '--session'), null);
    assert.equal(namedArgument(['--session', 'window-2'], '--session'), 'window-2');
  });
});
