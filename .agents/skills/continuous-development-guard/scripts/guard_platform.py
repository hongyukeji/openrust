"""标准库平台边界：锁、受管进程树、工具解析与限时协议读取。"""
import os
from pathlib import Path
import queue
import shutil
import subprocess
import threading
import time

WINDOWS = os.name == 'nt'
if WINDOWS:
    import ctypes
    from ctypes import wintypes as w
    import msvcrt
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    for name, args, result in (
        ('CreateJobObjectW', [ctypes.c_void_p, w.LPCWSTR], w.HANDLE),
        ('SetInformationJobObject', [w.HANDLE, ctypes.c_int, ctypes.c_void_p, w.DWORD], w.BOOL),
        ('AssignProcessToJobObject', [w.HANDLE, w.HANDLE], w.BOOL),
        ('TerminateJobObject', [w.HANDLE, w.UINT], w.BOOL),
        ('CloseHandle', [w.HANDLE], w.BOOL),
        ('OpenProcess', [w.DWORD, w.BOOL, w.DWORD], w.HANDLE),
        ('GetProcessTimes', [w.HANDLE, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p], w.BOOL),
        ('WaitForSingleObject', [w.HANDLE, w.DWORD], w.DWORD),
    ):
        function = getattr(kernel, name)
        function.argtypes, function.restype = args, result
else:
    import fcntl


def lock_file(stream, blocking=True):
    if not WINDOWS:
        fcntl.flock(stream, fcntl.LOCK_EX | (0 if blocking else fcntl.LOCK_NB))
        return
    # 不转交 Windows 字节锁：由 supervisor 自己持有直到退出。
    while True:
        stream.seek(0)
        try:
            msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
            return
        except OSError as error:
            if error.errno not in (13, 11, 36):
                raise
            if not blocking:
                raise BlockingIOError from error
            time.sleep(.05)


def command(arguments):
    path = shutil.which(arguments[0])
    if not path:
        raise FileNotFoundError('缺少已安装工具：' + arguments[0] + '；不会自动安装')
    if WINDOWS and Path(path).suffix.lower() in ('.cmd', '.bat'):
        # npm 的标准 shim 可执行 node 与同目录 JS，避免 cmd 对消息内容二次展开。
        import json
        stem = Path(path).stem.lower()
        base = Path(path).parent
        node = str(base / 'node.exe') if (base / 'node.exe').is_file() else shutil.which('node')
        matches = []
        for pattern in ('*/package.json', '@*/*/package.json'):
            for manifest in (base / 'node_modules').glob(pattern):
                try:
                    package = json.loads(manifest.read_text(encoding='utf-8'))
                    bins = package.get('bin', {})
                    if isinstance(bins, str):
                        bins = {package.get('name', '').rsplit('/', 1)[-1]: bins}
                    relative = bins.get(stem) if isinstance(bins, dict) else None
                    if not isinstance(relative, str):
                        continue
                    script = (manifest.parent / relative).resolve()
                    if script.is_relative_to(manifest.parent.resolve()) and script.is_file():
                        matches.append(script)
                except (OSError, ValueError):
                    continue
        if not node or len(matches) != 1:
            raise FileNotFoundError('无法安全解析工具 shim：' + path + '；需要已有原生程序或唯一标准 npm bin 入口，不自动安装')
        script = matches[0]
        return [node, str(script), *arguments[1:]]
    return [path, *arguments[1:]]


def background():
    return {'creationflags': subprocess.CREATE_NO_WINDOW} if WINDOWS else {'start_new_session': True}


def process_identity(pid):
    if not WINDOWS:
        return None
    handle = kernel.OpenProcess(0x1000 | 0x100000, False, pid)
    if not handle:
        return None
    try:
        if kernel.WaitForSingleObject(handle, 0) != 258:
            return None
        values = [ctypes.c_ulonglong() for _ in range(4)]
        if not kernel.GetProcessTimes(handle, *(ctypes.byref(v) for v in values)):
            return None
        return values[0].value
    finally:
        kernel.CloseHandle(handle)


class Job:
    """仅 supervisor 持有非继承 job handle；关闭即回收其 worker 及后代。"""
    def __init__(self):
        class Basic(ctypes.Structure):
            _fields_ = [('process_time', ctypes.c_longlong), ('job_time', ctypes.c_longlong),
                        ('flags', w.DWORD), ('minimum', ctypes.c_size_t), ('maximum', ctypes.c_size_t),
                        ('active', w.DWORD), ('affinity', ctypes.c_size_t), ('priority', w.DWORD), ('scheduling', w.DWORD)]
        class Limits(ctypes.Structure):
            _fields_ = [('basic', Basic), ('io', ctypes.c_ulonglong * 6),
                        ('process_memory', ctypes.c_size_t), ('job_memory', ctypes.c_size_t),
                        ('peak_process', ctypes.c_size_t), ('peak_job', ctypes.c_size_t)]
        self.handle = kernel.CreateJobObjectW(None, None)
        if not self.handle:
            raise ctypes.WinError(ctypes.get_last_error())
        limits = Limits()
        limits.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        if not kernel.SetInformationJobObject(self.handle, 9, ctypes.byref(limits), ctypes.sizeof(limits)):
            self.close()
            raise ctypes.WinError(ctypes.get_last_error())

    def assign(self, process):
        if not kernel.AssignProcessToJobObject(self.handle, int(process._handle)):
            raise ctypes.WinError(ctypes.get_last_error())

    def close(self):
        if self.handle:
            kernel.CloseHandle(self.handle)
            self.handle = None


class Lines:
    """管道读取线程每个 RPC 一个；队列有界，超时由主线程判断。"""
    def __init__(self, stream):
        self.items = queue.Queue(maxsize=16)
        self.closed = threading.Event()
        def read():
            try:
                while not self.closed.is_set():
                    line = stream.readline(1024 * 1024 + 1)
                    if len(line) > 1024 * 1024:
                        line = RuntimeError('协议单行超过1MiB')
                    while not self.closed.is_set():
                        try:
                            self.items.put(line, timeout=.1)
                            break
                        except queue.Full:
                            pass
                    if not line or isinstance(line, Exception):
                        break
            except (OSError, ValueError):
                pass
        self.thread = threading.Thread(target=read, daemon=True)
        self.thread.start()

    def get(self, deadline):
        try:
            value = self.items.get(timeout=max(0, deadline - time.monotonic()))
        except queue.Empty:
            raise TimeoutError('协议读取超时') from None
        if isinstance(value, Exception):
            raise value
        if not value:
            raise RuntimeError('协议进程退出')
        return value

    def close(self):
        self.closed.set()
        self.thread.join(timeout=2)


def launch_tool(arguments, **kwargs):
    """Windows 在工具派生之前绑定独立 job；协议退出也回收其 CLI 后代。"""
    resolved = command(arguments)
    if not WINDOWS:
        return subprocess.Popen(resolved, start_new_session=True, **kwargs)
    import json
    job = Job()
    process = None
    try:
        process = subprocess.Popen([os.sys.executable, '-B', str(Path(__file__).resolve()),
                                    '_exec', json.dumps(resolved)], **background(), **kwargs)
        process._guard_job = job
        job.assign(process)
        process.stdin.write(b'1')
        process.stdin.flush()
        return process
    except BaseException:
        job.close()
        if process is not None:
            if process.poll() is None:
                process.kill()
            process.wait(timeout=10)
            for stream in (process.stdin, process.stdout, process.stderr):
                if stream:
                    stream.close()
        raise


if __name__ == '__main__':
    import json
    import sys
    if len(sys.argv) != 3 or sys.argv[1] != '_exec' or os.read(sys.stdin.fileno(), 1) != b'1':
        raise SystemExit('无效工具启动协议')
    raise SystemExit(subprocess.call(json.loads(sys.argv[2])))
