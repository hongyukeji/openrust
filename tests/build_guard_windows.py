"""为原生 Windows 验收提供 Cargo 文件锁和受控的进程树。"""

import ctypes
from ctypes import wintypes as win
from contextlib import contextmanager
import msvcrt
import os
import subprocess
import sys
import time
from uuid import uuid4


kernel = ctypes.WinDLL('kernel32', use_last_error=True)


def api(name, result, *arguments):
    function = getattr(kernel, name)
    function.restype = result
    function.argtypes = arguments
    return function


class Overlapped(ctypes.Structure):
    _fields_ = [('Internal', ctypes.c_size_t), ('InternalHigh', ctypes.c_size_t),
                ('Offset', win.DWORD), ('OffsetHigh', win.DWORD), ('Event', win.HANDLE)]


class JobAccounting(ctypes.Structure):
    _fields_ = [('times', ctypes.c_longlong * 4), ('page_faults', win.DWORD),
                ('total_processes', win.DWORD), ('active_processes', win.DWORD),
                ('terminated_processes', win.DWORD)]


class JobLimits(ctypes.Structure):
    _fields_ = [('per_process_time', ctypes.c_longlong), ('per_job_time', ctypes.c_longlong),
                ('flags', win.DWORD), ('min_working_set', ctypes.c_size_t),
                ('max_working_set', ctypes.c_size_t), ('active_limit', win.DWORD),
                ('affinity', ctypes.c_size_t), ('priority', win.DWORD),
                ('scheduling', win.DWORD)]


class ExtendedJobLimits(ctypes.Structure):
    _fields_ = [('basic', JobLimits), ('io_counters', ctypes.c_ulonglong * 6),
                ('memory_limits', ctypes.c_size_t * 4)]


lock_file_ex = api('LockFileEx', win.BOOL, win.HANDLE, win.DWORD, win.DWORD,
                   win.DWORD, win.DWORD, ctypes.POINTER(Overlapped))
create_job = api('CreateJobObjectW', win.HANDLE, win.LPVOID, win.LPCWSTR)
set_job = api('SetInformationJobObject', win.BOOL, win.HANDLE, ctypes.c_int, win.LPVOID, win.DWORD)
assign_job = api('AssignProcessToJobObject', win.BOOL, win.HANDLE, win.HANDLE)
terminate_job = api('TerminateJobObject', win.BOOL, win.HANDLE, win.UINT)
query_job = api('QueryInformationJobObject', win.BOOL, win.HANDLE, ctypes.c_int,
                win.LPVOID, win.DWORD, win.LPVOID)
create_event = api('CreateEventW', win.HANDLE, win.LPVOID, win.BOOL, win.BOOL, win.LPCWSTR)
open_event = api('OpenEventW', win.HANDLE, win.DWORD, win.BOOL, win.LPCWSTR)
set_event = api('SetEvent', win.BOOL, win.HANDLE)
wait_event = api('WaitForSingleObject', win.DWORD, win.HANDLE, win.DWORD)
open_process = api('OpenProcess', win.HANDLE, win.DWORD, win.BOOL, win.DWORD)
close_handle = api('CloseHandle', win.BOOL, win.HANDLE)


def check(result):
    if not result:
        raise ctypes.WinError(ctypes.get_last_error())
    return result


def lock_file(handle, nonblocking=False):
    # Rust 的 Windows File::lock 锁定整个 u64 范围。关闭文件句柄时释放。
    flags = 2 | int(nonblocking)
    if not lock_file_ex(msvcrt.get_osfhandle(handle.fileno()), flags, 0,
                        0xffffffff, 0xffffffff, ctypes.byref(Overlapped())):
        error = ctypes.get_last_error()
        if error == 33:
            raise BlockingIOError('构建锁正被其他进程使用')
        raise ctypes.WinError(error)


def active_builds():
    # Windows 的公开进程清单不提供可靠的当前目录。清理时保守等待相关构建进程退出。
    powershell = os.path.join(os.environ['SystemRoot'], 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe')
    script = r"""
$ErrorActionPreference='Stop'
Get-CimInstance Win32_Process | Where-Object {
    $_.Name -match '^(cargo|rustc|rustdoc|cargo-clippy|clippy-driver)\.exe$' -or
    ($_.Name -match '^(node|npm)\.exe$' -and
     (!$_.CommandLine -or $_.CommandLine -match 'vitepress.*build|npm.*run.*build'))
} | ForEach-Object ProcessId
"""
    output = subprocess.check_output([powershell, '-NoProfile', '-NonInteractive', '-Command', script],
                                     text=True, errors='replace')
    return [int(line) for line in output.splitlines() if line.strip()]


class Job:
    def __init__(self):
        self.handle = check(create_job(None, None))
        limits = ExtendedJobLimits()
        limits.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        try:
            check(set_job(self.handle, 9, ctypes.byref(limits), ctypes.sizeof(limits)))
        except OSError:
            check(close_handle(self.handle))
            raise

    def active(self):
        accounting = JobAccounting()
        check(query_job(self.handle, 1, ctypes.byref(accounting), ctypes.sizeof(accounting), None))
        return accounting.active_processes

    def stop(self):
        if self.active():
            check(terminate_job(self.handle, 1))
        deadline = time.monotonic() + 30
        while self.active():
            if time.monotonic() >= deadline:
                raise RuntimeError('无法确认 Windows 验收进程树已退出')
            time.sleep(.05)


@contextmanager
def launch(command, env, cwd):
    # 启动器先等事件。加入 Job 前没有机会启动 Cargo 或任何后代进程。
    job = Job()
    name = 'Local\\OpenRustBuildGuard-' + uuid4().hex
    try:
        event = check(create_event(None, True, False, name))
        try:
            with subprocess.Popen([sys.executable, '-B', __file__, name, *command],
                                  env=env, cwd=cwd, creationflags=subprocess.CREATE_NEW_PROCESS_GROUP) as child:
                assigned = False
                try:
                    process = check(open_process(0x0101, False, child.pid))  # SET_QUOTA | TERMINATE
                    try:
                        check(assign_job(job.handle, process))
                        assigned = True
                    finally:
                        check(close_handle(process))
                    check(set_event(event))
                    yield child, job.stop
                finally:
                    if assigned:
                        job.stop()
                    elif child.poll() is None:
                        child.terminate()
        finally:
            check(close_handle(event))
    finally:
        check(close_handle(job.handle))


def run_command(name, command):
    event = check(open_event(0x00100000, False, name))  # SYNCHRONIZE
    try:
        if wait_event(event, 30_000) != 0:
            raise RuntimeError('未收到 Windows 验收守卫的启动许可')
    finally:
        check(close_handle(event))
    return subprocess.call(command)


if __name__ == '__main__':
    sys.exit(run_command(sys.argv[1], sys.argv[2:]))
