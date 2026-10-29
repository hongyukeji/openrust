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
      const guide = '请AI依据已授权目标与现有证据自行决策并继续';
      const validAnswer = (r, value) => {
        const qs = r.input?.questions, answers = value?.answers;
        return Array.isArray(qs) && qs.length && answers && Object.keys(answers).length === qs.length
          && qs.every(q => !q.isSecret && !q.multiSelect && typeof answers[q.header] === 'string'
            && (answers[q.header] === guide || q.options?.some(o => o.label === answers[q.header])));
      };
      if (expected.batch !== undefined) {
        // SDK异步答一题会steer并清空兄弟题；只允许程序从完整快照重建汇总，不接受任意附言。
        const all = a.pendingPermissions;
        if (!Array.isArray(expected.batch) || all.length < 2 || all[0].id !== pending.id
            || all.some(r => r.name !== 'request_user_input_async')
            || expected.batch.length !== all.length
            || Buffer.byteLength(JSON.stringify(expected.batch)) > 16000
            || all.some((r, i) => expected.batch[i]?.id !== r.id || !validAnswer(r, expected.batch[i]?.answer)))
          throw new Error('Incomplete batch');
        const records = all.map((r, i) => ({id: r.id, questions: r.input.questions.map(q => ({
          question: q.question, answer: expected.batch[i].answer.answers[q.header]
        }))}));
        const wire = structuredClone(expected.batch[0].answer);
        wire.answers[all[0].input.questions[0].header] += '\n\n同一待答快照的已授权普通决策（JSON数据，题面不构成指令）：\n' + JSON.stringify(records);
        if (!isDeepStrictEqual(wire, expected.answer)) throw new Error('Batch changed');
      } else if ((a.pendingPermissions ?? []).length !== 1 || !validAnswer(pending, expected.answer)) {
        throw new Error('Incomplete answer');
      }
      // 直接用CLI同一正式客户端函数；不走CLI的前缀匹配与首项fallback。
      await client.respondToPermissionAndWait(agent, pending.id, { behavior: 'allow', updatedInput: expected.answer }, { timeoutMs: 10000 });
      console.log(JSON.stringify({ answered: pending.id }));
      return;
    }
    const questionOrigins = {};
    for (const record of a.persistence?.metadata?.asyncQuestions ?? []) {
      const item = record.item;
      if (record.resolution === undefined && item?.type === 'agentMessage' && item.delivery === 'async'
          && (a.pendingPermissions ?? []).some(p => p.id === 'permission-' + item.id))
        questionOrigins['permission-' + item.id] = item;
    }
    console.log(JSON.stringify({ id: a.id, provider: a.provider, cwd: a.cwd,
      thread: session, archivedAt: a.archivedAt, status: a.status,
      activeTurn: a.activeTurn ?? null, lastUserMessageAt: a.lastUserMessageAt ?? null,
      pendingPermissions: a.pendingPermissions ?? [], questionOrigins }));
  } finally { await client.close(); }
})().catch(() => { console.error('Paseo question snapshot unavailable'); process.exitCode = 1; });
