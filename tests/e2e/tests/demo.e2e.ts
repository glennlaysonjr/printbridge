import { test } from '@e2e-dev/web';
import { expect } from 'e2e';

const AGENT_PORT = Number(process.env.AGENT_PORT ?? 9651);

test('demo connects to the agent and lists printers', async ({ app, screen }) => {
  await app.open(`/demo/?agentPort=${AGENT_PORT}`);

  await expect(screen.getByText('Connected to PrintBridge agent')).toBeVisible();
  await expect(screen.getByRole('button', 'Rescan network')).toBeEnabled();
  await expect(screen.getByText(/^\d+ printers?$/)).toBeVisible();
});

test('rescan runs a discovery pass and reports completion', async ({ app, screen }) => {
  await app.open(`/demo/?agentPort=${AGENT_PORT}`);
  await expect(screen.getByRole('button', 'Rescan network')).toBeEnabled();

  await screen.getByRole('button', 'Rescan network').tap();

  await expect(screen.getByText('Scan complete', { exact: false })).toBeVisible({ timeout: 30_000 });
  await expect(screen.getByRole('button', 'Rescan network')).toBeEnabled();
});

test('demo shows the install hint when no agent is running', async ({ app, screen }) => {
  // Port 9 (discard) has no PrintBridge agent behind it.
  await app.open('/demo/?agentPort=9');

  await expect(screen.getByText('Agent not reachable', { exact: false })).toBeVisible();
  await expect(screen.getByRole('button', 'Rescan network')).toBeDisabled();
});
