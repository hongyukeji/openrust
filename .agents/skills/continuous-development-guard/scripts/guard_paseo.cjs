/* 复用本机Paseo正式客户端读取原始待答快照并按精确ID回答；CLI inspect仅保留id/tool。 */
'use strict';
const { pathToFileURL } = require('node:url');
const path = require('node:path');
const { isDeepStrictEqual } = require('node:util');
(async () => {
  const [base, agent, submission] = process.argv.slice(2);
  if (!base || !/^[a-f0-9-]{36}$/.test(agent || '')) throw new Error('Invalid binding');
  const utils = path.join(base, 'node_modules/@getpaseo/cli/dist/utils');
  const { connectToDaemon } = await import(pathToFileURL(path.join(utils, 'client.js')).href);
  const { selectDaemonTarget } = await import(pathToFileURL(path.join(utils, 'daemon-target.js')).href);
  const client = await connectToDaemon({ target: selectDaemonTarget({}) });
  try {
    const result = await client.fetchAgent({ agentId: agent });
    const a = result?.agent;
    if (!a || a.id !== agent || a.provider !== 'codex') throw new Error('Agent identity changed');
    const session = a.persistence?.sessionId;
    if (!session || session !== a.runtimeInfo?.sessionId || session !== a.persistence?.metadata?.threadId)
      throw new Error('Session identity changed');
    if (submission) {
      const expected = JSON.parse(submission);
      const binding = { id: a.id, thread: session, activeTurn: a.activeTurn ?? null, lastUserMessageAt: a.lastUserMessageAt ?? null };
      const pending = (a.pendingPermissions ?? []).find(p => p.id === expected.request.id);
      if (!isDeepStrictEqual(a.pendingPermissions ?? [], expected.pending)
          || a.archivedAt || a.cwd !== expected.cwd || !isDeepStrictEqual(binding, expected.binding) || !isDeepStrictEqual(pending, expected.request)
          || pending?.kind !== 'question' || !['request_user_input', 'request_user_input_async'].includes(pending?.name)
          || (a.pendingPermissions ?? []).some(p => p.kind !== 'question' || p.provider !== 'codex'
            || !['request_user_input', 'request_user_input_async'].includes(p.name)
            || p.input?.questions?.some(q => q.isSecret || q.multiSelect))) throw new Error('Question changed');
      const questions = pending.input?.questions;
      const answers = expected.answer?.answers;
      if (!Array.isArray(questions) || !questions.length || !answers
          || Object.keys(answers).length !== questions.length
          || questions.some(q => q.isSecret || q.multiSelect || typeof answers[q.header] !== 'string'
            || !q.options?.some(o => o.label === answers[q.header]))) throw new Error('Incomplete answer');
      // 直接用CLI同一正式客户端函数；不走CLI的前缀匹配与首项fallback。
      await client.respondToPermissionAndWait(agent, pending.id, { behavior: 'allow', updatedInput: expected.answer }, { timeoutMs: 10000 });
      console.log(JSON.stringify({ answered: pending.id }));
      return;
    }
    console.log(JSON.stringify({ id: a.id, provider: a.provider, cwd: a.cwd,
      thread: session, archivedAt: a.archivedAt, status: a.status,
      activeTurn: a.activeTurn ?? null, lastUserMessageAt: a.lastUserMessageAt ?? null,
      pendingPermissions: a.pendingPermissions ?? [] }));
  } finally { await client.close(); }
})().catch(() => { console.error('Paseo question snapshot unavailable'); process.exitCode = 1; });
