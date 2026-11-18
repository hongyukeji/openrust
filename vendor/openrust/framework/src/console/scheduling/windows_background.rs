use std::{
    cmp::Ordering,
    ffi::OsStr,
    io,
    os::windows::{
        ffi::OsStrExt as _,
        io::{FromRawHandle as _, OwnedHandle},
    },
    path::Path,
    ptr,
};

use windows_sys::Win32::{
    Globalization::{CSTR_EQUAL, CompareStringOrdinal},
    System::Threading::{
        CREATE_NEW_PROCESS_GROUP, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DETACHED_PROCESS,
        PROCESS_INFORMATION, STARTUPINFOW,
    },
};

pub(super) fn spawn(executable: &Path, base_path: &Path, args: &[String]) -> io::Result<()> {
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
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..STARTUPINFOW::default()
    };
    let mut process = PROCESS_INFORMATION::default();

    // SAFETY: All buffers are live and terminated, and the command buffer is
    // writable. A detached console process with handle inheritance disabled
    // cannot keep the launching command's redirected streams open.
    let success = unsafe {
        CreateProcessW(
            program.as_ptr(),
            command.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            CREATE_UNICODE_ENVIRONMENT | CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS,
            environment.as_ptr().cast(),
            directory.as_ptr(),
            &startup,
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
