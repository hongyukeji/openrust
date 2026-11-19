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
public static class InitTerminal {
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

    public static string Run(string binary,string cwd,string secret,string launcher) {
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
                Check(CreateProcessW(null,new StringBuilder("\""+launcher+"\" \""+binary+"\" artisan init --extensions=api"),IntPtr.Zero,IntPtr.Zero,false,0x00080000,IntPtr.Zero,cwd,ref startup,out info));
            } finally {
                SetStdHandle(-10,savedInput); SetStdHandle(-11,savedOutput); SetStdHandle(-12,savedError);
            }
            DateTime deadline=DateTime.UtcNow.AddSeconds(90); int consumed=0,answers=0; bool password=false,retry=false;
            while(WaitForSingleObject(info.process,20)==258 && DateTime.UtcNow<deadline) {
                string text; lock(gate) text=transcript.ToString();
                string pending=System.Text.RegularExpressions.Regex.Replace(text.Substring(consumed),@"\x1b\][^\x07]*(?:\x07|\x1b\\)","");
                pending=System.Text.RegularExpressions.Regex.Replace(pending,@"\x1b\[[0-?]*[ -/]*[@-~]","").TrimEnd();
                if(pending.EndsWith(":") || pending.EndsWith("[Y/n]") || pending.EndsWith("[y/N]")) {
                    string answer="\r";
                    if(pending.Contains("DB_PASSWORD [")) { answer=secret+"\r"; password=true; }
                    if(pending.Contains("Edit connection settings and retry?")) { answer="n\r"; retry=true; }
                    byte[] bytes=Encoding.UTF8.GetBytes(answer); writer.Write(bytes,0,bytes.Length); writer.Flush();
                    consumed=text.Length; answers++;
                }
            }
            if(WaitForSingleObject(info.process,0)==258) throw new Exception("Windows interactive init timed out");
            uint exit; Check(GetExitCodeProcess(info.process,out exit));
            ClosePseudoConsole(console); console=IntPtr.Zero;
            pump.Wait(5000);
            string result; lock(gate) result=transcript.ToString();
            if(!password || !retry || answers<15 || exit==0) throw new Exception("Incomplete secure-input / cancellation path: answers="+answers+", exit="+exit);
            if(result.Contains(secret)) throw new Exception("Secure password was echoed to the terminal");
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

$root = Join-Path ([IO.Path]::GetTempPath()) ('openrust-init-terminal-' + [Guid]::NewGuid().ToString('N'))
$previousBase = $env:APP_BASE_PATH
New-Item -ItemType Directory -Path $EvidenceDirectory -Force | Out-Null
try {
    New-Item -ItemType Directory -Path $root, (Join-Path $root 'bootstrap'), (Join-Path $root 'routes'), (Join-Path $root 'config') | Out-Null
    [IO.File]::WriteAllText((Join-Path $root 'Cargo.toml'), "[package]`nname = `"fixture`"`nversion = `"0.0.0`"`n")
    foreach ($name in @('bootstrap/app.rs', 'routes/mod.rs', 'config/mod.rs')) {
        [IO.File]::WriteAllText((Join-Path $root $name), "// terminal fixture`n")
    }
    $original = "APP_ENV=local`nAPP_KEY=01234567890123456789012345678901`nDB_CONNECTION=mysql`nDB_HOST=127.0.0.1`nDB_PORT=1`nDB_DATABASE=terminal_fixture`nDB_USERNAME=fixture`n"
    [IO.File]::WriteAllText((Join-Path $root '.env'), $original)
    $env:APP_BASE_PATH = $root
    # A remote host can give the pseudoconsole child redirected standard handles.
    # Open the console devices with read/write access; write-only CONOUT$ handles
    # cannot be queried by GetConsoleMode and would be treated as unattended.
    $launcherSource = Join-Path $root 'terminal_launcher.rs'
    $launcherBinary = Join-Path $root 'terminal_launcher.exe'
    @'
use std::{fs::OpenOptions, process::{Command, Stdio}};
fn main() {
    let mut args = std::env::args_os().skip(1);
    let binary = args.next().expect("application binary");
    let input = OpenOptions::new().read(true).write(true).open("CONIN$").unwrap();
    let output = OpenOptions::new().read(true).write(true).open("CONOUT$").unwrap();
    let error = output.try_clone().unwrap();
    let status = Command::new(binary).args(args).stdin(Stdio::from(input))
        .stdout(Stdio::from(output)).stderr(Stdio::from(error)).status().unwrap();
    std::process::exit(status.code().unwrap_or(1));
}
'@ | Set-Content -Encoding UTF8 $launcherSource
    & rustc --edition=2024 $launcherSource -o $launcherBinary
    if ($LASTEXITCODE -ne 0) { throw 'Could not build the console handle launcher' }
    $secret = 'terminal-secret-' + [Guid]::NewGuid().ToString('N')
    $transcript = [InitTerminal]::Run([IO.Path]::GetFullPath($Binary), $root, $secret, $launcherBinary)
    if ([IO.File]::ReadAllText((Join-Path $root '.env')) -cne $original) { throw 'Cancelled init changed configuration' }
    if (Test-Path (Join-Path $root 'routes/api.rs')) { throw 'Cancelled init installed API routes' }
    New-Item -ItemType Directory -Path $EvidenceDirectory -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $EvidenceDirectory 'terminal.log'), $transcript)
    Write-Output 'PASS: Windows ConPTY wizard, hidden password, connection failure cancellation, preserved configuration'
} catch {
    [IO.File]::WriteAllText((Join-Path $EvidenceDirectory 'terminal-failure.log'), [InitTerminal]::LastTranscript)
    throw
} finally {
    $env:APP_BASE_PATH = $previousBase
    if (Test-Path $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}
