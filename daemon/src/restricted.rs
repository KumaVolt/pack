//! Windows: run a program without administrator rights. PostgreSQL refuses to
//! start when its token counts as an administrator (an elevated terminal, UAC
//! turned off, CI runners). `pg_ctl` gets around that by starting the server
//! with a restricted token, but then detaches; `pack run-restricted` does the
//! same and stays in the foreground, so the supervisor can watch and stop it.

use std::ffi::c_void;
use std::io;
use std::mem::{size_of, size_of_val, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_REVISION, ACL_SIZE_INFORMATION, AclSizeInformation, AddAccessAllowedAceEx, AddAce,
    CheckTokenMembership, CreateRestrictedToken, CreateWellKnownSid, DISABLE_MAX_PRIVILEGE, GetAce, GetAclInformation, GetLengthSid,
    GetTokenInformation, InitializeAcl, OBJECT_INHERIT_ACE, SECURITY_MAX_SID_SIZE, SID_AND_ATTRIBUTES, SetTokenInformation,
    TOKEN_ALL_ACCESS, TOKEN_DEFAULT_DACL, TOKEN_INFORMATION_CLASS, TOKEN_USER, TokenDefaultDacl, TokenUser, WELL_KNOWN_SID_TYPE,
    WinBuiltinAdministratorsSid, WinBuiltinPowerUsersSid,
};
use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CreateProcessAsUserW, GetCurrentProcess, GetExitCodeProcess, INFINITE, OpenProcessToken,
    PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOW, TerminateProcess, WaitForSingleObject,
};

/// Would PostgreSQL see this process as an administrator? The same check as
/// its `pgwin32_is_admin`: an enabled Administrators or Power Users group.
/// Normal UAC sessions have Administrators as deny-only, so they aren't.
pub fn is_admin() -> bool {
    [WinBuiltinAdministratorsSid, WinBuiltinPowerUsersSid].into_iter().any(|kind| {
        let Ok(mut sid) = well_known_sid(kind) else { return false };
        let mut member = 0;
        unsafe { CheckTokenMembership(null_mut(), sid.as_mut_ptr().cast(), &mut member) != 0 && member != 0 }
    })
}

/// Run `program` with a restricted token, wait for it and exit with its code.
pub fn run(program: &Path, args: &[String]) -> ! {
    match spawn_and_wait(program, args) {
        Ok(code) => std::process::exit(code as i32),
        Err(e) => {
            eprintln!("pack: could not start {} without admin rights: {e}", program.display());
            std::process::exit(127);
        }
    }
}

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn check(ok: i32) -> io::Result<()> {
    if ok != 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

fn spawn_and_wait(program: &Path, args: &[String]) -> io::Result<u32> {
    let token = restricted_token()?;
    let app: Vec<u16> = program.as_os_str().encode_wide().chain([0]).collect();
    let mut line: Vec<u16> = command_line(&program.display().to_string(), args).encode_utf16().chain([0]).collect();
    // Hand on our stdout/stderr (the service log), like pg_ctl's InheritStdHandles.
    let mut si: STARTUPINFOW = unsafe { zeroed() };
    si.cb = size_of::<STARTUPINFOW>() as u32;
    si.dwFlags = STARTF_USESTDHANDLES;
    unsafe {
        si.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
        si.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE);
        si.hStdError = GetStdHandle(STD_ERROR_HANDLE);
    }
    let mut pi: PROCESS_INFORMATION = unsafe { zeroed() };
    check(unsafe {
        CreateProcessAsUserW(
            token.0,
            app.as_ptr(),
            line.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_SUSPENDED | CREATE_NO_WINDOW,
            null(),
            null(),
            &si,
            &mut pi,
        )
    })?;
    let process = Handle(pi.hProcess);
    let thread = Handle(pi.hThread);
    // The supervisor stops us by killing this process: take the server and
    // every backend it starts down with us.
    let _job = match kill_on_close_job(process.0) {
        Ok(job) => job,
        Err(e) => {
            unsafe { TerminateProcess(process.0, 1) };
            return Err(e);
        }
    };
    unsafe {
        ResumeThread(thread.0);
        WaitForSingleObject(process.0, INFINITE);
    }
    let mut code = 0;
    check(unsafe { GetExitCodeProcess(process.0, &mut code) })?;
    Ok(code)
}

/// Our own token without the Administrators and Power Users groups and with
/// every privilege but SeChangeNotify removed — what pg_ctl and initdb use.
fn restricted_token() -> io::Result<Handle> {
    let mut own = null_mut();
    check(unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, &mut own) })?;
    let own = Handle(own);
    let mut admins = well_known_sid(WinBuiltinAdministratorsSid)?;
    let mut power_users = well_known_sid(WinBuiltinPowerUsersSid)?;
    let disable = [
        SID_AND_ATTRIBUTES { Sid: admins.as_mut_ptr().cast(), Attributes: 0 },
        SID_AND_ATTRIBUTES { Sid: power_users.as_mut_ptr().cast(), Attributes: 0 },
    ];
    let mut token = null_mut();
    check(unsafe {
        CreateRestrictedToken(own.0, DISABLE_MAX_PRIVILEGE, disable.len() as u32, disable.as_ptr(), 0, null(), 0, null(), &mut token)
    })?;
    let token = Handle(token);
    add_user_to_default_dacl(token.0)?;
    Ok(token)
}

/// An elevated token's default DACL grants only Administrators and SYSTEM, so
/// with Administrators disabled the server couldn't open its own pipes and
/// child processes. Grant the user as well, like PostgreSQL's AddUserToTokenDacl.
fn add_user_to_default_dacl(token: HANDLE) -> io::Result<()> {
    let dacl = token_info(token, TokenDefaultDacl)?;
    let user = token_info(token, TokenUser)?;
    unsafe {
        let old = (*dacl.as_ptr().cast::<TOKEN_DEFAULT_DACL>()).DefaultDacl;
        let sid = (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
        let mut info = ACL_SIZE_INFORMATION { AceCount: 0, AclBytesInUse: size_of::<ACL>() as u32, AclBytesFree: 0 };
        if !old.is_null() {
            check(GetAclInformation(old, (&raw mut info).cast(), size_of_val(&info) as u32, AclSizeInformation))?;
        }
        let len = info.AclBytesInUse + size_of::<ACCESS_ALLOWED_ACE>() as u32 + GetLengthSid(sid) - size_of::<u32>() as u32;
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        let acl = buf.as_mut_ptr().cast::<ACL>();
        check(InitializeAcl(acl, len, ACL_REVISION))?;
        for i in 0..info.AceCount {
            let mut ace = null_mut();
            check(GetAce(old, i, &mut ace))?;
            check(AddAce(acl, ACL_REVISION, u32::MAX, ace, (*ace.cast::<ACE_HEADER>()).AceSize as u32))?;
        }
        check(AddAccessAllowedAceEx(acl, ACL_REVISION, OBJECT_INHERIT_ACE, GENERIC_ALL, sid))?;
        let new = TOKEN_DEFAULT_DACL { DefaultDacl: acl };
        check(SetTokenInformation(token, TokenDefaultDacl, (&raw const new).cast(), size_of_val(&new) as u32))
    }
}

/// A job that kills its processes once its last handle closes.
fn kill_on_close_job(process: HANDLE) -> io::Result<Handle> {
    let job = unsafe { CreateJobObjectW(null(), null()) };
    if job.is_null() {
        return Err(io::Error::last_os_error());
    }
    let job = Handle(job);
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    check(unsafe {
        SetInformationJobObject(job.0, JobObjectExtendedLimitInformation, (&raw const limits).cast::<c_void>(), size_of_val(&limits) as u32)
    })?;
    check(unsafe { AssignProcessToJobObject(job.0, process) })?;
    Ok(job)
}

/// Token information in an 8-byte aligned buffer.
fn token_info(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<u64>> {
    let mut len = 0;
    unsafe { GetTokenInformation(token, class, null_mut(), 0, &mut len) };
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    check(unsafe { GetTokenInformation(token, class, buf.as_mut_ptr().cast(), len, &mut len) })?;
    Ok(buf)
}

fn well_known_sid(kind: WELL_KNOWN_SID_TYPE) -> io::Result<Vec<u32>> {
    let mut sid = vec![0u32; (SECURITY_MAX_SID_SIZE as usize).div_ceil(4)];
    let mut len = SECURITY_MAX_SID_SIZE;
    check(unsafe { CreateWellKnownSid(kind, null_mut(), sid.as_mut_ptr().cast(), &mut len) })?;
    Ok(sid)
}

/// A command line that the C runtime splits back into `program` and `args`.
fn command_line(program: &str, args: &[String]) -> String {
    let mut line = String::new();
    for arg in std::iter::once(program).chain(args.iter().map(String::as_str)) {
        if !line.is_empty() {
            line.push(' ');
        }
        if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\x0b', '"']) {
            line.push_str(arg);
            continue;
        }
        line.push('"');
        let mut backslashes = 0;
        for c in arg.chars() {
            if c == '\\' {
                backslashes += 1;
                continue;
            }
            // Backslashes only escape when a quote follows.
            let n = if c == '"' { backslashes * 2 + 1 } else { backslashes };
            line.extend(std::iter::repeat_n('\\', n));
            line.push(c);
            backslashes = 0;
        }
        line.extend(std::iter::repeat_n('\\', backslashes * 2));
        line.push('"');
    }
    line
}

#[cfg(test)]
mod tests {
    use super::command_line;

    #[test]
    fn quotes_like_the_c_runtime() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(command_line(r"C:\pg\postgres.exe", &args(&["-p", "5432"])), r"C:\pg\postgres.exe -p 5432");
        assert_eq!(command_line(r"C:\Program Files\pg.exe", &args(&[""])), r#""C:\Program Files\pg.exe" """#);
        assert_eq!(command_line("x", &args(&[r"C:\my data\"])), r#"x "C:\my data\\""#);
        assert_eq!(command_line("x", &args(&[r#"a "b" c"#])), r#"x "a \"b\" c""#);
        assert_eq!(command_line("x", &args(&[r#"a\"b c"#])), r#"x "a\\\"b c""#);
    }
}
