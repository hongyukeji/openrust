param(
    [Parameter(Mandatory=$true)][string]$Binary,
    [Parameter(Mandatory=$true)][string]$EvidenceDirectory
)
$ErrorActionPreference = 'Stop'

# Use a real Windows pseudoconsole: redirected stdin cannot exercise secure input.
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Text;
using System.Threading.Tasks;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class PailTerminal {
    public static string LastTranscript = "";
    [StructLayout(LayoutKind.Sequential)] struct Coord { public short X, Y; }
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct Startup {
        public int cb; public string reserved, desktop, title;
        public int x,y,xSize,ySize,xChars,yChars,fill,flags;
        public short show,reservedSize; public IntPtr reservedBytes,input,output,error;
    }
    [StructLayout(LayoutKind.Sequential)] struct Extended { public Startup startup; public IntPtr attributes; }
    [StructLayout(LayoutKind.Sequential)] struct ProcessInfo { public IntPtr process,thread; public int pid,tid; }
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool CreatePipe(out IntPtr read,out IntPtr write,IntPtr security,int size);
    [DllImport("kernel32.dll")] static extern int CreatePseudoConsole(Coord size,IntPtr input,IntPtr output,uint flags,out IntPtr console);
    [DllImport("kernel32.dll")] static extern void ClosePseudoConsole(IntPtr console);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool InitializeProcThreadAttributeList(IntPtr list,int count,int flags,ref IntPtr size);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool UpdateProcThreadAttribute(IntPtr list,uint flags,IntPtr attribute,IntPtr value,IntPtr size,IntPtr previous,IntPtr returned);
    [DllImport("kernel32.dll")] static extern void DeleteProcThreadAttributeList(IntPtr list);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool CreateProcessW(string app,StringBuilder command,IntPtr ps,IntPtr ts,bool inherit,uint flags,IntPtr env,string cwd,ref Extended startup,out ProcessInfo info);
    [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr handle,uint timeout);
    [DllImport("kernel32.dll")] static extern bool GetExitCodeProcess(IntPtr process,out uint code);
    [DllImport("kernel32.dll")] static extern bool TerminateProcess(IntPtr process,uint code);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll")] static extern IntPtr GetStdHandle(int kind);
    [DllImport("kernel32.dll")] static extern bool SetStdHandle(int kind,IntPtr handle);
    static void Check(bool ok) { if(!ok) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error()); }

    static void WriteRecord(string path,string record,Encoding encoding) {
        using(var file=new FileStream(path,FileMode.Open,FileAccess.Write,FileShare.ReadWrite | FileShare.Delete)) {
            // Match Rust File::lock's entire u64 byte range. LockFileEx retries below
            // avoid racing the native reader's mandatory Windows shared lock.
            var overlapped=new Overlapped();
            DateTime deadline=DateTime.UtcNow.AddSeconds(5);
            while(!LockFileEx(file.SafeFileHandle.DangerousGetHandle(),3,0,uint.MaxValue,uint.MaxValue,ref overlapped)) {
                int error=Marshal.GetLastWin32Error();
                if(error!=33 || DateTime.UtcNow>=deadline) throw new System.ComponentModel.Win32Exception(error);
                System.Threading.Thread.Sleep(10);
            }
            try { file.Seek(0,SeekOrigin.End); byte[] bytes=encoding.GetBytes(record); file.Write(bytes,0,bytes.Length); file.Flush(); }
            finally { Check(UnlockFileEx(file.SafeFileHandle.DangerousGetHandle(),0,uint.MaxValue,uint.MaxValue,ref overlapped)); }
        }
    }
    [StructLayout(LayoutKind.Sequential)] struct Overlapped { public IntPtr internal1,internal2; public uint offset,offsetHigh; public IntPtr eventHandle; }
    [DllImport("kernel32.dll",SetLastError=true)] static extern bool LockFileEx(IntPtr file,uint flags,uint reserved,uint low,uint high,ref Overlapped overlapped);
    [DllImport("kernel32.dll",SetLastError=true)] static extern bool UnlockFileEx(IntPtr file,uint reserved,uint low,uint high,ref Overlapped overlapped);

    public static string Run(string binary,string cwd,string arguments,string launcher,bool timeout,bool quiet) {
        IntPtr inputRead=IntPtr.Zero,inputWrite=IntPtr.Zero,outputRead=IntPtr.Zero,outputWrite=IntPtr.Zero;
        IntPtr console=IntPtr.Zero,list=IntPtr.Zero; ProcessInfo info=new ProcessInfo();
        StringBuilder transcript=new StringBuilder(); object gate=new object();
        FileStream reader=null,writer=null; Task pump=null;
        try {
            Check(CreatePipe(out inputRead,out inputWrite,IntPtr.Zero,0));
            Check(CreatePipe(out outputRead,out outputWrite,IntPtr.Zero,0));
            int hr=CreatePseudoConsole(new Coord {X=140,Y=40},inputRead,outputWrite,0,out console);
            if(hr!=0) Marshal.ThrowExceptionForHR(hr);
            CloseHandle(inputRead); inputRead=IntPtr.Zero;
            CloseHandle(outputWrite); outputWrite=IntPtr.Zero;
            IntPtr size=IntPtr.Zero;
            InitializeProcThreadAttributeList(IntPtr.Zero,1,0,ref size);
            list=Marshal.AllocHGlobal(size);
            Check(InitializeProcThreadAttributeList(list,1,0,ref size));
            Check(UpdateProcThreadAttribute(list,0,(IntPtr)0x00020016,console,(IntPtr)IntPtr.Size,IntPtr.Zero,IntPtr.Zero));
            Extended startup=new Extended(); startup.startup.cb=Marshal.SizeOf(typeof(Extended)); startup.attributes=list;
            reader=new FileStream(new SafeFileHandle(outputRead,false),FileAccess.Read);
            writer=new FileStream(new SafeFileHandle(inputWrite,false),FileAccess.Write);
            pump=Task.Run(()=> {
                byte[] buffer=new byte[4096]; Decoder decoder=Encoding.UTF8.GetDecoder(); char[] chars=new char[4096];
                try { int count; while((count=reader.Read(buffer,0,buffer.Length))>0) {
                    int decoded=decoder.GetChars(buffer,0,count,chars,0);
                    lock(gate) transcript.Append(chars,0,decoded);
                }} catch(IOException) {}
            });
            // Remote launchers redirect their own std handles. Let the child acquire its
            // pseudoconsole handles instead of copying those redirected parent handles.
            IntPtr savedInput=GetStdHandle(-10),savedOutput=GetStdHandle(-11),savedError=GetStdHandle(-12);
            try {
                SetStdHandle(-10,IntPtr.Zero); SetStdHandle(-11,IntPtr.Zero); SetStdHandle(-12,IntPtr.Zero);
                Check(CreateProcessW(null,new StringBuilder("\""+launcher+"\" \""+binary+"\" artisan pail "+arguments),IntPtr.Zero,IntPtr.Zero,false,0x00080000,IntPtr.Zero,cwd,ref startup,out info));
            } finally {
                SetStdHandle(-10,savedInput); SetStdHandle(-11,savedOutput); SetStdHandle(-12,savedError);
            }
            DateTime deadline=DateTime.UtcNow.AddSeconds(30); bool written=false,interrupted=false;
            string sessionDirectory=Path.Combine(cwd,"storage","pail");
            while(WaitForSingleObject(info.process,20)==258 && DateTime.UtcNow<deadline) {
                string text; lock(gate) text=transcript.ToString();
                if(!written && Directory.Exists(sessionDirectory)) {
                    string[] sessions=Directory.GetFiles(sessionDirectory,"*.pail");
                    if(sessions.Length==1) {
                        WriteRecord(sessions[0],"{\"message\":\"Windows terminal \u4e2d\u6587 payment\",\"context\":{\"__pail\":{\"origin\":{\"type\":\"console\",\"command\":\"fixture\"}}},\"level_name\":\"ERROR\",\"datetime\":\"2026-11-20T12:00:00+00:00\"}\n",new UTF8Encoding(false));
                        written=true;
                    }
                }
                if(!timeout && written && !interrupted && (quiet || text.Contains("Windows terminal \u4e2d\u6587 payment"))) {
                    // ETX goes through the pseudoconsole's processed-input keyboard path.
                    if(quiet) System.Threading.Thread.Sleep(150);
                    byte[] bytes=new byte[]{3}; writer.Write(bytes,0,1); writer.Flush(); interrupted=true;
                }
            }
            if(WaitForSingleObject(info.process,0)==258) throw new Exception("Windows Pail terminal timed out");
            uint exit; Check(GetExitCodeProcess(info.process,out exit));
            ClosePseudoConsole(console); console=IntPtr.Zero;
            if(!pump.Wait(5000)) throw new Exception("ConPTY output pump survived shutdown");
            string result; lock(gate) result=transcript.ToString();
            if(!written || exit!=0 || (!timeout && !interrupted)) throw new Exception("Incomplete Pail terminal path: exit="+exit+", written="+written);
            if(!quiet && !result.Contains("Windows terminal \u4e2d\u6587 payment")) throw new Exception("UTF-8 record was not displayed");
            if(timeout && !result.Contains("Maximum execution time exceeded.")) throw new Exception("Missing timeout message");
            if(Directory.GetFiles(sessionDirectory,"*.pail").Length!=0) throw new Exception("Owned Pail session survived shutdown");
            return result;
        } finally {
            lock(gate) LastTranscript=transcript.ToString();
            if(info.process!=IntPtr.Zero) {
                if(WaitForSingleObject(info.process,0)==258) {
                    using(var kill=System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo("taskkill.exe","/PID "+info.pid+" /T /F") {UseShellExecute=false,CreateNoWindow=true})) kill.WaitForExit();
                    TerminateProcess(info.process,1);
                }
                CloseHandle(info.process);
            }
            if(info.thread!=IntPtr.Zero) CloseHandle(info.thread);
            if(console!=IntPtr.Zero) ClosePseudoConsole(console);
            if(writer!=null) writer.Dispose(); if(reader!=null) reader.Dispose();
            foreach(IntPtr handle in new[]{inputRead,inputWrite,outputRead,outputWrite}) if(handle!=IntPtr.Zero) CloseHandle(handle);
            if(list!=IntPtr.Zero) { DeleteProcThreadAttributeList(list); Marshal.FreeHGlobal(list); }
        }
    }
}
'@

$root = Join-Path ([IO.Path]::GetTempPath()) ('openrust-pail-terminal-' + [Guid]::NewGuid().ToString('N'))
$previousEnvironment = @{}
foreach ($name in @('APP_BASE_PATH', 'APP_ENV', 'APP_CONFIG_CACHE', 'APP_PACKAGES_CACHE', 'VAPOR_SSM_PATH', 'PAIL_TESTS', 'NO_COLOR', 'FORCE_COLOR')) {
    $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
    [Environment]::SetEnvironmentVariable($name, $null, 'Process')
}
$env:APP_ENV = 'local'
New-Item -ItemType Directory -Path $EvidenceDirectory -Force | Out-Null
try {
    New-Item -ItemType Directory -Path $root | Out-Null
    [IO.File]::WriteAllText((Join-Path $root '.env'), "APP_ENV=local`nAPP_KEY=01234567890123456789012345678901`n")
    $env:APP_BASE_PATH = $root
    $launcherSource = Join-Path $root 'terminal_launcher.rs'
    $launcherBinary = Join-Path $root 'terminal_launcher.exe'
    @'
use std::{fs::OpenOptions, os::windows::io::AsRawHandle, process::{Command, Stdio}};
#[link(name="kernel32")]
unsafe extern "system" {
    fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32)->i32>, add:i32)->i32;
    fn GetConsoleMode(handle:*mut std::ffi::c_void, mode:*mut u32)->i32;
}
unsafe extern "system" fn keep_launcher_alive(_:u32)->i32 {1}
fn mode(file:&std::fs::File)->u32 {
    let mut value=0;
    assert_ne!(unsafe {GetConsoleMode(file.as_raw_handle(), &mut value)},0);
    value
}
fn main() {
    assert_ne!(unsafe {SetConsoleCtrlHandler(Some(keep_launcher_alive),1)},0);
    let mut args=std::env::args_os().skip(1);
    let binary=args.next().unwrap();
    let input=OpenOptions::new().read(true).write(true).open("CONIN$").unwrap();
    let output=OpenOptions::new().read(true).write(true).open("CONOUT$").unwrap();
    let before=(mode(&input), mode(&output));
    let mut child=Command::new(binary).args(args)
        .stdin(Stdio::from(input.try_clone().unwrap()))
        .stdout(Stdio::from(output.try_clone().unwrap()))
        .stderr(Stdio::from(output.try_clone().unwrap())).spawn().unwrap();
    let root=std::path::PathBuf::from(std::env::var_os("APP_BASE_PATH").unwrap());
    std::fs::write(root.join("child.pid"),child.id().to_string()).unwrap();
    let status=child.wait().unwrap();
    assert_eq!(mode(&input),before.0,"console input mode was not restored");
    // console enables virtual-terminal processing for ANSI output; retain that capability.
    assert_eq!(mode(&output) & !4,before.1 & !4,"console output mode was not restored");
    std::process::exit(status.code().unwrap_or(1));
}
'@ | Set-Content -Encoding UTF8 $launcherSource
    & rustc --edition=2024 $launcherSource -o $launcherBinary
    if ($LASTEXITCODE -ne 0) { throw 'Could not build the console handle launcher' }
    foreach ($mode in @('default', 'no-ansi', 'quiet', 'silent', 'timeout')) {
        $arguments = '--timeout=0 --level=error --message=payment -vv'
        if ($mode -eq 'timeout') { $arguments='--timeout=2 --no-ansi' }
        if ($mode -eq 'no-ansi') { $arguments+=' --no-ansi' }
        if ($mode -eq 'quiet') { $arguments+=' --quiet' }
        if ($mode -eq 'silent') { $arguments+=' --silent' }
        $transcript = [PailTerminal]::Run([IO.Path]::GetFullPath($Binary), $root, $arguments, $launcherBinary, ($mode -eq 'timeout'), ($mode -eq 'quiet' -or $mode -eq 'silent'))
        [IO.File]::WriteAllText((Join-Path $EvidenceDirectory ($mode+'.log')), $transcript)
        $plain = $transcript -replace '\x1b\][^\x07]*(?:\x07|\x1b\\)', '' -replace '\x1b\[[0-?]*[ -/]*[@-~]', ''
        if (($mode -eq 'quiet' -or $mode -eq 'silent') -and $plain.Trim().Length -ne 0) { throw "$mode output was not suppressed" }
        if ($mode -eq 'no-ansi' -and [regex]::IsMatch($transcript, '\x1b\[(?:\d+;)*44(?:;\d+)*m')) { throw 'no-ansi retained the colored banner' }
        if ($mode -eq 'default' -and !([regex]::IsMatch($transcript, '\x1b\[(?:\d+;)*44(?:;\d+)*m'))) { throw 'default terminal did not decorate output' }
        $childPid=[int]([IO.File]::ReadAllText((Join-Path $root 'child.pid')))
        if (Get-Process -Id $childPid -ErrorAction SilentlyContinue) { throw 'Pail process survived terminal shutdown' }
        Write-Output "PASS: Windows ConPTY $mode, UTF-8/filter, clean session and console modes"
    }
    & $Binary artisan inspire --no-ansi
    if ($LASTEXITCODE -ne 0) { throw 'Independent Artisan failed after terminal tests' }
} catch {
    [IO.File]::WriteAllText((Join-Path $EvidenceDirectory 'terminal-failure.log'), [PailTerminal]::LastTranscript)
    throw
} finally {
    foreach ($name in $previousEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($name, $previousEnvironment[$name], 'Process')
    }
    if (Test-Path $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}
