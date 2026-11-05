import type { E2EConfig } from 'e2e';
import { web } from '@e2e-dev/web';

// No `agents` block: these tests use plain locators and assertions only, so they
// never call an AI model and need no API key.
export default {
  targets: [
    {
      engine: web(),
      app: {
        // Port 0: the runner picks a free port and passes it to serve.mjs as {port}.
        url: 'http://127.0.0.1:0',
        command: {
          executable: 'node',
          args: ['serve.mjs', '{port}'],
          inheritEnv: ['PRINTBRIDGE_BIN', 'AGENT_PORT'],
          log: '.e2e/logs/app.log',
        },
      },
    },
  ],
} satisfies E2EConfig;
