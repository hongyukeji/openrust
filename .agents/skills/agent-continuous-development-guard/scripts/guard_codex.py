"""原生 Codex 平台边界；只连接既有 app-server，不接管线程。"""
from pathlib import Path
import uuid


def select_backend(args, environment, require_thread=True):
    """缺 Paseo 身份时只使用明确的原生线程身份，不猜测最近会话。"""
    backend = getattr(args, 'backend', 'auto')
    agent = args.agent or environment.get('PASEO_AGENT_ID')
    thread = args.thread or environment.get('CODEX_THREAD_ID')
    if backend == 'auto':
        backend = 'paseo' if agent else 'codex'
    if (backend == 'paseo' and not agent
            or not thread and (require_thread or backend == 'codex')):
        raise ValueError('缺少可信任务身份，不能猜测最新任务')
    if thread:
        uuid.UUID(thread)
    if backend == 'paseo':
        uuid.UUID(agent)
    return backend, agent if backend == 'paseo' else thread, thread


def validate_thread(thread, config, root):
    """正式线程与可信本地 metadata 必须同项目、同主线程。"""
    if (thread.get('id') != config['thread']
            or Path(thread.get('cwd', '/')).resolve() != root
            or thread.get('parentThreadId') or isinstance(thread.get('source'), dict)):
        raise ValueError('原生 Codex 主线程绑定不一致')


def snapshot(config, root, rpc):
    """只相信已加载线程的正式运行状态；未知状态不得映射为 running。"""
    thread = rpc('thread/read', {'threadId': config['thread'], 'includeTurns': False}).get('thread') or {}
    validate_thread(thread, config, root)
    status = thread.get('status') or {}
    kind = status.get('type')
    flags = status.get('activeFlags')
    reason = None
    if kind == 'active' and isinstance(flags, list):
        if any(flag not in ('waitingOnApproval', 'waitingOnUserInput') for flag in flags):
            reason = 'native_state_unverifiable'
        elif 'waitingOnApproval' in flags:
            reason = 'native_approval_pending'
        elif 'waitingOnUserInput' in flags:
            reason = 'native_input_pending'
        mapped = 'running' if not flags and not reason else 'unknown'
    elif kind in ('idle', 'systemError') and thread.get('canAcceptDirectInput') is True:
        mapped = 'idle' if kind == 'idle' else 'error'
    else:
        mapped, reason = 'unknown', 'native_state_unverifiable'
    queue = rpc('thread/queue/list', {'threadId': config['thread'], 'limit': 1})
    if queue.get('data') or queue.get('nextCursor'):
        reason = reason or 'native_input_queued'
    elif not isinstance(queue.get('data'), list):
        reason = reason or 'native_state_unverifiable'
    return {'Cwd': str(root), 'Provider': 'codex', 'Status': mapped,
            'PendingPermissions': [], 'NativeWaitReason': reason, 'NativeThread': thread}


def enqueue(config, prompt, rpc, root, dispatch_key, before_start):
    """已有发送意图由调用方持久化；回执未知不重试，不启动临时执行器。"""
    before = snapshot(config, root, rpc)
    if before['Status'] not in ('idle', 'error') or before['NativeWaitReason']:
        raise ValueError('原生线程不在可确认的空闲边界')
    client_id = queue_client_id(config, dispatch_key)
    result = rpc('thread/queue/add', {'threadId': config['thread'],
                 'clientUserMessageId': client_id, 'input': [{'type': 'text', 'text': prompt}]})
    queued = result.get('queuedSubmission') or {}
    if queued.get('clientUserMessageId') != client_id or not queued.get('id'):
        raise ValueError('原生入队回执不明，保留发送意图')
    # 入队是独立往返；期间的新运行/审批/真人排队输入优先，自己的项保留供终态清理。
    thread = rpc('thread/read', {'threadId': config['thread'], 'includeTurns': False}).get('thread') or {}
    validate_thread(thread, config, root)
    if ((thread.get('status') or {}).get('type') not in ('idle', 'systemError')
            or thread.get('canAcceptDirectInput') is not True):
        raise ValueError('原生入队后线程状态变化，不启动恢复项')
    queue = rpc('thread/queue/list', {'threadId': config['thread'], 'limit': 100})
    rows = queue.get('data')
    if (not isinstance(rows, list) or len(rows) != 1 or queue.get('nextCursor')
            or rows[0].get('id') != queued['id']
            or rows[0].get('clientUserMessageId') != client_id):
        raise ValueError('原生入队后队列变化，不启动恢复项')
    before_start()
    # 共享 daemon 持有执行生命周期；关闭 proxy 不关闭 daemon 或已启动 turn。
    return rpc('thread/queue/start', {'threadId': config['thread'], 'queuedSubmissionId': queued['id']})



def queue_client_id(config, dispatch_key):
    """从已落盘的发送意图派生精确ID，供停止时只清理本守护队列。"""
    if not isinstance(dispatch_key, str) or not dispatch_key:
        raise ValueError('原生入队缺少已持久化发送意图')
    return str(uuid.uuid5(uuid.NAMESPACE_URL, 'agent-continuous-development-guard:' + config['thread'] + ':' + config['goal_id'] + ':' + dispatch_key))


def cancel_queued(config, dispatch_key, rpc, before_delete=None):
    """只撤销同一发送意图的未消费队列项，真人输入全部保留。"""
    if not dispatch_key:
        return
    client_id = queue_client_id(config, dispatch_key)
    cursor = None
    attempted = set()
    # 有界扫描；无法确认已清理时不得宣称完整停止。
    for _ in range(10):
        result = rpc('thread/queue/list', {'threadId': config['thread'], 'limit': 100, 'cursor': cursor})
        rows = result.get('data')
        if not isinstance(rows, list):
            raise ValueError('原生队列未知，保留停止核验边界')
        deleted = False
        for row in rows:
            if row.get('clientUserMessageId') == client_id:
                if not row.get('id'):
                    raise ValueError('原生自有队列ID未知')
                if row['id'] in attempted:
                    raise ValueError('自有队列删除后仍可见，未重复删除')
                if before_delete is not None:
                    before_delete(row['id'])
                attempted.add(row['id'])
                rpc('thread/queue/delete', {'threadId': config['thread'], 'queuedSubmissionId': row['id']})
                deleted = True
        if deleted:
            # 删除回执不替代正式列表确认；从首页复核，也避免删除使cursor跳项。
            cursor = None
            continue
        cursor = result.get('nextCursor')
        if not cursor:
            return
    raise ValueError('原生队列超过有界核验范围，停止尚未确认')

def interrupt(config, root, rpc):
    """只中断正式快照中唯一的 active turn；未知状态不猜测 turn ID。"""
    thread = rpc('thread/read', {'threadId': config['thread'], 'includeTurns': False}).get('thread') or {}
    validate_thread(thread, config, root)
    kind = (thread.get('status') or {}).get('type')
    if kind == 'idle':
        return
    turns = rpc('thread/turns/list', {'threadId': config['thread'], 'limit': 2,
                'sortDirection': 'desc', 'itemsView': 'summary'}).get('data')
    if not isinstance(turns, list):
        raise ValueError('原生turn快照未知，未中断任何线程')
    active = [turn for turn in turns if turn.get('status') == 'inProgress']
    if kind != 'active' or len(active) != 1 or not active[0].get('id'):
        raise ValueError('原生运行状态不明，未中断任何线程')
    rpc('turn/interrupt', {'threadId': config['thread'], 'turnId': active[0]['id']})
