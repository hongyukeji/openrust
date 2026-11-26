use std::{
    cmp::Ordering,
    ffi::OsStr,
    fs::{File, OpenOptions},
    io,
    os::windows::{
        ffi::OsStrExt as _,
        io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle},
    },
    path::Path,
    ptr,
};

use windows_sys::Win32::{
    Foundation::{HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation},
    Globalization::{CSTR_EQUAL, CompareStringOrdinal},
    System::Threading::{
        CREATE_NEW_PROCESS_GROUP, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DETACHED_PROCESS,
        DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
        InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION, STARTF_USESTDHANDLES,
        STARTUPINFOEXW, STARTUPINFOW, UpdateProcThreadAttribute,
    },
};

pub(crate) fn spawn(
    executable: &Path,
    base_path: &Path,
    args: &[String],
    input: Option<File>,
) -> io::Result<()> {
    let program = wide(executable.as_os_str())?;
    let directory = wide(base_path.as_os_str())?;
    let mut command = Vec::new();
    append_argument(&mut command, &program[..program.len() - 1]);
    for arg in args {
        let arg = wide(arg.as_ref())?;
        command.push(u16::from(b' '));
        append_argument(&mut command, &arg[..arg.len() - 1]);
    }
    command.push(0);
    let environment = environment_block(&directory[..directory.len() - 1]);
    let output = input
        .as_ref()
        .map(|file| {
            let output = OpenOptions::new().write(true).open(r"\\.\NUL")?;
            make_inheritable(file)?;
            make_inheritable(&output)?;
            Ok::<_, io::Error>(output)
        })
        .transpose()?;
    let handles = input
        .as_ref()
        .zip(output.as_ref())
        .map(|(input, output)| [input.as_raw_handle(), output.as_raw_handle()]);
    let mut attributes = handles
        .as_ref()
        .map(|handles| AttributeList::new(handles))
        .transpose()?;
    let mut startup = STARTUPINFOEXW::default();
    let mut flags = CREATE_UNICODE_ENVIRONMENT | CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS;
    if let (Some(&[input, output]), Some(attributes)) = (handles.as_ref(), attributes.as_mut()) {
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = input;
        startup.StartupInfo.hStdOutput = output;
        startup.StartupInfo.hStdError = output;
        startup.lpAttributeList = attributes.as_mut_ptr();
        flags |= EXTENDED_STARTUPINFO_PRESENT;
    } else {
        startup.StartupInfo.cb = size_of::<STARTUPINFOW>() as u32;
    }
    let mut process = PROCESS_INFORMATION::default();

    // SAFETY: All buffers are live and terminated, and the command buffer is
    // writable. When inheritance is enabled, the attribute list and its
    // private stdio files remain live until the call completes.
    let success = unsafe {
        CreateProcessW(
            program.as_ptr(),
            command.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            i32::from(attributes.is_some()),
            flags,
            environment.as_ptr().cast(),
            directory.as_ptr(),
            ptr::from_ref(&startup).cast::<STARTUPINFOW>(),
            &mut process,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: A successful CreateProcessW returns two distinct owned handles.
    // Closing them leaves the background process running.
    unsafe {
        drop(OwnedHandle::from_raw_handle(process.hThread));
        drop(OwnedHandle::from_raw_handle(process.hProcess));
    }
    Ok(())
}

fn make_inheritable(file: &File) -> io::Result<()> {
    // SAFETY: The open file owns this handle. It is closed after spawning.
    if unsafe {
        SetHandleInformation(
            file.as_raw_handle(),
            HANDLE_FLAG_INHERIT,
            HANDLE_FLAG_INHERIT,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

struct AttributeList<'a> {
    storage: Vec<usize>,
    _handles: &'a [HANDLE],
}

impl<'a> AttributeList<'a> {
    fn new(handles: &'a [HANDLE]) -> io::Result<Self> {
        let mut size = 0;
        // SAFETY: A null list queries the required size through the out parameter.
        unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut size) };
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut storage = vec![0_usize; size.div_ceil(size_of::<usize>())];
        // SAFETY: The aligned buffer has at least the requested number of bytes.
        if unsafe {
            InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), 1, 0, &mut size)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut list = Self {
            storage,
            _handles: handles,
        };
        // SAFETY: The list is initialized, and it borrows the array until deletion.
        if unsafe {
            UpdateProcThreadAttribute(
                list.as_mut_ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr().cast(),
                size_of_val(handles),
                ptr::null_mut(),
                ptr::null(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(list)
    }

    fn as_mut_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
}

impl Drop for AttributeList<'_> {
    fn drop(&mut self) {
        // SAFETY: The initialized attribute list is released before its buffer.
        unsafe { DeleteProcThreadAttributeList(self.as_mut_ptr()) };
    }
}

fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut units: Vec<_> = value.encode_wide().collect();
    if units.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "background process argument or path contains NUL",
        ));
    }
    units.push(0);
    Ok(units)
}

// Windows receives a single command line. Quote every argument and double
// backslashes before a closing quote or an embedded literal quote.
fn append_argument(command: &mut Vec<u16>, argument: &[u16]) {
    const QUOTE: u16 = b'"' as u16;
    const SLASH: u16 = b'\\' as u16;
    command.push(QUOTE);
    let mut slashes = 0;
    for &unit in argument {
        if unit == SLASH {
            slashes += 1;
            continue;
        }
        if unit == QUOTE {
            slashes = slashes * 2 + 1;
        }
        command.extend(std::iter::repeat_n(SLASH, slashes));
        command.push(unit);
        slashes = 0;
    }
    command.extend(std::iter::repeat_n(SLASH, slashes * 2));
    command.push(QUOTE);
}

fn environment_block(base_path: &[u16]) -> Vec<u16> {
    let base_key: Vec<_> = "APP_BASE_PATH".encode_utf16().collect();
    let mut vars: Vec<_> = std::env::vars_os()
        .map(|(key, value)| (key.encode_wide().collect::<Vec<_>>(), value))
        .filter(|(key, _)| compare_keys(key, &base_key) != Ordering::Equal)
        .collect();
    vars.push((
        base_key,
        std::os::windows::ffi::OsStringExt::from_wide(base_path),
    ));
    vars.sort_by(|(left, _), (right, _)| compare_keys(left, right));

    let mut block = Vec::new();
    for (key, value) in vars {
        block.extend(key);
        block.push(u16::from(b'='));
        block.extend(value.encode_wide());
        block.push(0);
    }
    block.push(0);
    block
}

fn compare_keys(left: &[u16], right: &[u16]) -> Ordering {
    // SAFETY: Both pointers are live for their supplied UTF-16 lengths.
    // Windows environment names fit in signed 32-bit lengths.
    let result = unsafe {
        CompareStringOrdinal(
            left.as_ptr(),
            left.len() as i32,
            right.as_ptr(),
            right.len() as i32,
            1,
        )
    };
    result.cmp(&CSTR_EQUAL)
}
