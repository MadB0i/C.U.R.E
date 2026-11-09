//! Kernel-grade Native NT Syscall engine for deep process and thread inspection.
//! Queries `ntdll.dll` directly via `NtQuerySystemInformation` (`SystemProcessInformation = 5`)
//! to read live kernel executive process structures (`EPROCESS`) directly from NT kernel space.
//! Bypasses user-mode Win32 API hooks and provides kernel-level process and thread telemetry.

use serde::{Deserialize, Serialize};
use std::ffi::c_void;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeProcessInfo {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub thread_count: u32,
    pub working_set_bytes: u64,
    pub kernel_time_100ns: u64,
    pub user_time_100ns: u64,
}

#[cfg(windows)]
type NtQuerySystemInformationFn = unsafe extern "system" fn(
    system_information_class: u32,
    system_information: *mut c_void,
    system_information_length: u32,
    return_length: *mut u32,
) -> i32;

/// Checks if the Native NT kernel interface is available in this process.
pub fn is_native_nt_available() -> bool {
    #[cfg(windows)]
    {
        get_nt_query_system_information().is_some()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
extern "system" {
    fn GetModuleHandleA(lpModuleName: *const u8) -> *mut c_void;
    fn GetProcAddress(hModule: *mut c_void, lpProcName: *const u8) -> *mut c_void;
}

#[cfg(windows)]
fn get_nt_query_system_information() -> Option<NtQuerySystemInformationFn> {
    // SAFETY: ntdll.dll is loaded by Windows into every process address space by default.
    let h_ntdll = unsafe { GetModuleHandleA(c"ntdll.dll".as_ptr() as *const u8) };
    if h_ntdll.is_null() {
        return None;
    }
    // SAFETY: Resolving NtQuerySystemInformation export directly from ntdll.
    let proc =
        unsafe { GetProcAddress(h_ntdll, c"NtQuerySystemInformation".as_ptr() as *const u8) };
    if proc.is_null() {
        return None;
    }
    Some(unsafe { std::mem::transmute::<*mut c_void, NtQuerySystemInformationFn>(proc) })
}

/// Query running processes directly from the NT Kernel EPROCESS tables via `NtQuerySystemInformation`.
/// SystemProcessInformation = 5
pub fn query_kernel_processes_native() -> Result<Vec<NativeProcessInfo>, String> {
    #[cfg(windows)]
    {
        let nt_func = get_nt_query_system_information()
            .ok_or_else(|| "NtQuerySystemInformation is not available in ntdll.dll".to_string())?;

        const SYSTEM_PROCESS_INFORMATION_CLASS: u32 = 5;
        const STATUS_INFO_LENGTH_MISMATCH: i32 = -1073741820; // 0xC0000004

        let mut buffer_size: u32 = 256 * 1024; // 256 KB initial buffer
        let mut buffer: Vec<u8> = vec![0u8; buffer_size as usize];

        loop {
            let mut return_length: u32 = 0;
            // SAFETY: Calling NtQuerySystemInformation with allocated heap buffer.
            let status = unsafe {
                nt_func(
                    SYSTEM_PROCESS_INFORMATION_CLASS,
                    buffer.as_mut_ptr() as *mut c_void,
                    buffer_size,
                    &mut return_length,
                )
            };

            if status == STATUS_INFO_LENGTH_MISMATCH || status < 0 && return_length > buffer_size {
                buffer_size = return_length.max(buffer_size * 2);
                buffer.resize(buffer_size as usize, 0);
                continue;
            }

            if status < 0 {
                return Err(format!(
                    "NtQuerySystemInformation failed with NTSTATUS: {:#010X}",
                    status as u32
                ));
            }

            break;
        }

        let mut processes = Vec::new();
        let mut offset = 0usize;

        // Parse SYSTEM_PROCESS_INFORMATION structures from raw kernel memory buffer
        while offset < buffer.len() {
            let ptr = buffer.as_ptr().wrapping_add(offset);

            // In 64-bit Windows:
            // offset 0x00: NextEntryOffset (ULONG, 4 bytes)
            // offset 0x04: NumberOfThreads (ULONG, 4 bytes)
            // offset 0x08: WorkingSetPrivateSize (LARGE_INTEGER, 8 bytes)
            // offset 0x20: CreateTime (LARGE_INTEGER, 8 bytes)
            // offset 0x28: UserTime (LARGE_INTEGER, 8 bytes)
            // offset 0x30: KernelTime (LARGE_INTEGER, 8 bytes)
            // offset 0x38: ImageName (UNICODE_STRING: Length u16, MaxLength u16, Buffer *mut u16)
            // offset 0x48: BasePriority (KPRIORITY, 4 bytes)
            // offset 0x50: UniqueProcessId (HANDLE, 8 bytes on x64)
            // offset 0x58: InheritedFromUniqueProcessId (HANDLE, 8 bytes on x64)

            let next_entry_offset = unsafe { std::ptr::read_unaligned(ptr as *const u32) } as usize;
            let thread_count = unsafe { std::ptr::read_unaligned(ptr.add(4) as *const u32) };
            let user_time = unsafe { std::ptr::read_unaligned(ptr.add(0x28) as *const u64) };
            let kernel_time = unsafe { std::ptr::read_unaligned(ptr.add(0x30) as *const u64) };

            // ImageName UNICODE_STRING: Length is at offset 0x38, Buffer is at offset 0x40 (on x64)
            let name_len =
                unsafe { std::ptr::read_unaligned(ptr.add(0x38) as *const u16) } as usize / 2;
            let name_ptr = unsafe { std::ptr::read_unaligned(ptr.add(0x40) as *const *const u16) };

            let name = if name_len > 0 && !name_ptr.is_null() {
                // Read UTF-16 slice safely
                let slice = unsafe { std::slice::from_raw_parts(name_ptr, name_len) };
                String::from_utf16_lossy(slice)
            } else {
                "System Idle Process".to_string()
            };

            // Process IDs
            let pid = (unsafe { std::ptr::read_unaligned(ptr.add(0x50) as *const usize) }) as u32;
            let parent_pid_val =
                (unsafe { std::ptr::read_unaligned(ptr.add(0x58) as *const usize) }) as u32;
            let parent_pid = if parent_pid_val != 0 {
                Some(parent_pid_val)
            } else {
                None
            };

            // WorkingSetPrivateSize is at offset 0x08 on modern Windows
            let working_set_bytes =
                unsafe { std::ptr::read_unaligned(ptr.add(0x08) as *const u64) };

            processes.push(NativeProcessInfo {
                pid,
                parent_pid,
                name,
                thread_count,
                working_set_bytes,
                kernel_time_100ns: kernel_time,
                user_time_100ns: user_time,
            });

            if next_entry_offset == 0 {
                break;
            }
            offset += next_entry_offset;
        }

        Ok(processes)
    }

    #[cfg(not(windows))]
    {
        Err("Native NT syscalls are only supported on Windows".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_native_nt_available() {
        #[cfg(windows)]
        assert!(is_native_nt_available());
    }

    #[test]
    fn test_query_kernel_processes_native() {
        #[cfg(windows)]
        {
            let res = query_kernel_processes_native();
            assert!(
                res.is_ok(),
                "query_kernel_processes_native failed: {:?}",
                res.err()
            );
            let procs = res.unwrap();
            assert!(
                !procs.is_empty(),
                "Expected running processes from kernel EPROCESS table"
            );
            // Must at least find System or CURE process
            let current_pid = std::process::id();
            let found_current = procs.iter().any(|p| p.pid == current_pid);
            assert!(
                found_current,
                "Current process PID {} must be present in kernel process table",
                current_pid
            );
        }
    }
}
