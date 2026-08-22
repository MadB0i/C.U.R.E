//! Authenticode signature verification (Detection Engine v2).
//!
//! The [`SignatureStatus`] enum and the [`resolve_executable_path`] helper
//! compile on every platform so risk scoring stays unit-testable anywhere.
//! The real verification calls WinVerifyTrust (WinTrust) and only exists on
//! Windows, mirroring the cfg-gating used by `scanners::registry`.
//!
//! Two-stage verification on Windows, mirroring what tools like SigCheck do:
//!
//! 1. **Embedded check** — `WTD_CHOICE_FILE` verifies an Authenticode
//!    signature embedded in the PE itself (typical for third-party apps).
//! 2. **Catalog fallback** — many Windows system components carry NO embedded
//!    signature and are validated against security catalogs (.cat) instead;
//!    a plain embedded check reports those as `TRUST_E_NOSIGNATURE`. When
//!    that happens we hash the file (`CryptCATAdminCalcHashFromFileHandle`),
//!    enumerate every catalog claiming that hash under the
//!    `DRIVER_ACTION_VERIFY` subsystem, and re-run WinTrust per match with
//!    `WTD_CHOICE_CATALOG`. This reproduces what
//!    `Get-AuthenticodeSignature` reports for system binaries.
//!
//! MVP scope: verdict only — the publisher/certificate subject is NOT
//! extracted yet (that needs certificate store walking; tracked as a Phase 4
//! stretch goal in the README).

use std::path::{Path, PathBuf};

/// Outcome of Authenticode verification for one executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureStatus {
    /// A signature (embedded or catalog-backed) verifies cleanly.
    ValidSigned,
    /// No verifiable signature at all: neither embedded nor claimed by any
    /// security catalog.
    Unsigned,
    /// An EMBEDDED signature is present but does NOT verify (tampered bytes,
    /// broken or untrusted chain, revoked/expired signer).
    ///
    /// Note: on systems that sign everything catalog-side, tampering usually
    /// shows up as [`SignatureStatus::Unsigned`] instead — the modified file
    /// simply stops being claimed by its catalog. `Invalid` fires when a
    /// broken embedded signature was left behind.
    Invalid,
    /// No verdict: file missing/unreadable, path unresolvable, or WinTrust
    /// could not run. Never scored for or against the entry.
    Unknown,
}

/// Extracts just the executable from a persistence command line and returns
/// it as an existing file path.
///
/// Handles both quoted (`"C:\Program Files\App\app.exe" --flag`) and
/// unquoted (`C:\Users\x\app.exe -silent`) leading tokens via
/// [`crate::risk::extract_program_path`]. Returns `None` when the token is
/// empty, relative (bare names like `svchost.exe -k netsvcs` cannot be
/// resolved without Windows search-path semantics), or does not exist on
/// disk (already quarantined / malformed entry).
pub fn resolve_executable_path(command: &str) -> Option<PathBuf> {
    let raw = crate::risk::extract_program_path(command).trim();
    if raw.is_empty() {
        return None;
    }
    let candidate = Path::new(raw);
    if !candidate.is_absolute() || !candidate.is_file() {
        return None;
    }
    Some(candidate.to_path_buf())
}

/// Verifies a file's Authenticode signature. Non-Windows builds have no
/// WinTrust and always answer `Unknown`.
pub fn check_signature(exe_path: &Path) -> SignatureStatus {
    #[cfg(windows)]
    {
        imp::verify(exe_path)
    }
    #[cfg(not(windows))]
    {
        let _ = exe_path;
        SignatureStatus::Unknown
    }
}

// ---------------------------------------------------------------------------
// Windows implementation (WinTrust + catalog fallback)
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HANDLE, HWND};
    use windows::Win32::Security::Cryptography::Catalog::{
        CryptCATAdminAcquireContext, CryptCATAdminCalcHashFromFileHandle,
        CryptCATAdminEnumCatalogFromHash, CryptCATAdminReleaseCatalogContext,
        CryptCATAdminReleaseContext, CryptCATCatalogInfoFromContext, CATALOG_INFO,
    };
    use windows::Win32::Security::WinTrust::{
        WinVerifyTrust, DRIVER_ACTION_VERIFY, WINTRUST_ACTION_GENERIC_VERIFY_V2,
        WINTRUST_CATALOG_INFO, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
        WINTRUST_DATA_UNION_CHOICE, WTD_CHOICE_CATALOG, WTD_CHOICE_FILE, WTD_REVOKE_NONE,
        WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
    };

    use super::SignatureStatus;

    /// NUL-terminated UTF-16 for Win32 string parameters.
    fn wide(os: &std::ffi::OsStr) -> Vec<u16> {
        let mut v: Vec<u16> = os.encode_wide().collect();
        v.push(0);
        v
    }

    pub(super) fn verify(path: &Path) -> SignatureStatus {
        // Stage 1: embedded signature.
        let status = hr_to_status(wintrust_file_verify(path));
        if !matches!(status, SignatureStatus::Unsigned) {
            return status;
        }
        // Stage 2: catalog-backed signature.
        catalog_verify(path)
    }

    fn wintrust_file_verify(path: &Path) -> i32 {
        let wpath = wide(path.as_os_str());
        let mut file_info = WINTRUST_FILE_INFO {
            cbStruct: u32::try_from(std::mem::size_of::<WINTRUST_FILE_INFO>()).unwrap_or(0),
            pcwszFilePath: PCWSTR(wpath.as_ptr()),
            hFile: HANDLE::default(),
            pgKnownSubject: std::ptr::null_mut(),
        };
        let mut action_guid = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        unsafe {
            run_winverifytrust(
                &mut action_guid,
                WTD_CHOICE_FILE,
                WINTRUST_DATA_0 {
                    pFile: &mut file_info,
                },
            )
        }
    }

    /// Catalog stage: does any security catalog claim this exact binary?
    fn catalog_verify(path: &Path) -> SignatureStatus {
        let Some(hash) = catalog_file_hash(path) else {
            return SignatureStatus::Unknown;
        };
        let wpath = wide(path.as_os_str());
        // The provider matches members by tag; system catalogs key them by file name.
        let Some(file_name) = path.file_name() else {
            return SignatureStatus::Unsigned;
        };
        let member_tag = wide(file_name);

        let mut admin: isize = 0;
        // NOTE: the GENERIC_VERIFY_V2 subsystem finds nothing here; the
        // driver-verification context indexes the system component catalogs
        // we need (verified empirically against Get-AuthenticodeSignature).
        if unsafe { CryptCATAdminAcquireContext(&mut admin, Some(&DRIVER_ACTION_VERIFY), 0) }
            .is_err()
        {
            return SignatureStatus::Unknown;
        }

        // Enumerate ALL matching handles before verifying anything —
        // releasing contexts mid-walk corrupts the iterator.
        let mut prev: isize = 0;
        let mut matched: Vec<isize> = Vec::new();
        loop {
            let cat = unsafe { CryptCATAdminEnumCatalogFromHash(admin, &hash, 0, Some(&mut prev)) };
            if cat == 0 || matched.len() >= 64 {
                break;
            }
            matched.push(cat);
        }

        let mut verified = false;
        for &cat in &matched {
            let mut info = CATALOG_INFO::default();
            info.cbStruct = u32::try_from(std::mem::size_of::<CATALOG_INFO>()).unwrap_or(0);
            if unsafe { CryptCATCatalogInfoFromContext(cat, &mut info, 0) }.is_err() {
                continue;
            }
            let catalog_path = wide_truncated(&info.wszCatalogFile);

            let mut catalog_info = WINTRUST_CATALOG_INFO {
                cbStruct: u32::try_from(std::mem::size_of::<WINTRUST_CATALOG_INFO>())
                    .unwrap_or(0),
                dwCatalogVersion: 0,
                pcwszCatalogFilePath: PCWSTR(catalog_path.as_ptr()),
                pcwszMemberTag: PCWSTR(member_tag.as_ptr()),
                pcwszMemberFilePath: PCWSTR(wpath.as_ptr()),
                hMemberFile: HANDLE::default(),
                pbCalculatedFileHash: hash.as_ptr() as *mut u8,
                cbCalculatedFileHash: u32::try_from(hash.len()).unwrap_or(0),
                pcCatalogContext: std::ptr::null_mut(),
                hCatAdmin: admin,
            };
            let mut action_guid = WINTRUST_ACTION_GENERIC_VERIFY_V2;
            let hr = unsafe {
                run_winverifytrust(
                    &mut action_guid,
                    WTD_CHOICE_CATALOG,
                    WINTRUST_DATA_0 {
                        pCatalog: &mut catalog_info,
                    },
                )
            };
            if hr == 0 {
                verified = true;
                break;
            }
        }

        for cat in matched {
            unsafe {
                let _ = CryptCATAdminReleaseCatalogContext(admin, cat, 0);
            }
        }
        unsafe {
            let _ = CryptCATAdminReleaseContext(admin, 0);
        }

        if verified {
            SignatureStatus::ValidSigned
        } else {
            SignatureStatus::Unsigned
        }
    }

    fn catalog_file_hash(path: &Path) -> Option<Vec<u8>> {
        use std::os::windows::io::AsRawHandle;

        let file = std::fs::File::open(path).ok()?;
        let handle = HANDLE(file.as_raw_handle() as _);
        unsafe {
            let mut cb: u32 = 0;
            if !CryptCATAdminCalcHashFromFileHandle(handle, &mut cb, None, 0).as_bool() {
                return None;
            }
            let mut buf = vec![0u8; cb as usize];
            if !CryptCATAdminCalcHashFromFileHandle(handle, &mut cb, Some(buf.as_mut_ptr()), 0)
                .as_bool()
            {
                return None;
            }
            buf.truncate(cb as usize);
            Some(buf)
        }
    }

    fn wide_truncated(buf: &[u16]) -> Vec<u16> {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let mut v = buf[..end].to_vec();
        v.push(0);
        v
    }

    /// Runs WinVerifyTrust once and closes the provider state afterwards
    /// (skipping the CLOSE call leaks the provider's state blob).
    unsafe fn run_winverifytrust(
        action_guid: &mut windows::core::GUID,
        union_choice: WINTRUST_DATA_UNION_CHOICE,
        anonymous: WINTRUST_DATA_0,
    ) -> i32 {
        let mut data = WINTRUST_DATA {
            cbStruct: u32::try_from(std::mem::size_of::<WINTRUST_DATA>()).unwrap_or(0),
            dwUIChoice: WTD_UI_NONE,
            fdwRevocationChecks: WTD_REVOKE_NONE,
            dwUnionChoice: union_choice,
            dwStateAction: WTD_STATEACTION_VERIFY,
            Anonymous: anonymous,
            ..Default::default()
        };
        let hr = WinVerifyTrust(
            HWND::default(),
            action_guid,
            &mut data as *mut WINTRUST_DATA as *mut std::ffi::c_void,
        );
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        let _ = WinVerifyTrust(
            HWND::default(),
            action_guid,
            &mut data as *mut WINTRUST_DATA as *mut std::ffi::c_void,
        );
        hr
    }

    fn hr_to_status(hr: i32) -> SignatureStatus {
        const S_OK: i32 = 0;
        const TRUST_E_NOSIGNATURE: i32 = 0x800B_0100u32 as i32; // no signature found
        const CRYPT_E_FILE_ERROR: i32 = 0x8009_2003u32 as i32; // file unreadable
        const TRUST_E_PROVIDER_UNKNOWN: i32 = 0x800B_0001u32 as i32;
        const TRUST_E_ACTION_UNKNOWN: i32 = 0x800B_0002u32 as i32;
        const TRUST_E_SUBJECT_FORM_UNKNOWN: i32 = 0x800B_0003u32 as i32;

        if hr == S_OK {
            return SignatureStatus::ValidSigned;
        }
        if hr == TRUST_E_NOSIGNATURE {
            return SignatureStatus::Unsigned;
        }
        match hr {
            // environmental problems: no verdict rather than "bad"
            code
                if code == CRYPT_E_FILE_ERROR
                    || code == TRUST_E_PROVIDER_UNKNOWN
                    || code == TRUST_E_ACTION_UNKNOWN
                    || code == TRUST_E_SUBJECT_FORM_UNKNOWN =>
            {
                SignatureStatus::Unknown
            }
            // anything else means verification RAN and FAILED:
            // tampered digest, bad cert chain, expired/revoked signer, distrust
            _ => SignatureStatus::Invalid,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_leading_token_is_extracted() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("app tool.exe");
        std::fs::write(&exe, b"MZ").unwrap();

        let cmd = format!(r#""{}" --flag value"#, exe.display());
        assert_eq!(resolve_executable_path(&cmd).as_deref(), Some(exe.as_path()));
    }

    #[test]
    fn unquoted_leading_token_is_extracted() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("tool.exe");
        std::fs::write(&exe, b"MZ").unwrap();

        let cmd = format!("{} -silent", exe.display());
        assert_eq!(resolve_executable_path(&cmd).as_deref(), Some(exe.as_path()));
    }

    #[test]
    fn missing_file_resolves_to_none() {
        assert_eq!(
            resolve_executable_path(r"C:\definitely\not\here\ghost.exe --x"),
            None
        );
    }

    #[test]
    fn bare_relative_names_are_not_resolved() {
        assert_eq!(resolve_executable_path("svchost.exe -k netsvcs"), None);
        assert_eq!(resolve_executable_path(r"tools\thing.exe"), None);
        assert_eq!(resolve_executable_path("   "), None);
    }

    #[test]
    fn directories_do_not_count_as_executables() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = format!("{}", dir.path().display());
        assert_eq!(resolve_executable_path(&cmd), None);
    }

    #[cfg(windows)]
    fn system32(name: &str) -> std::path::PathBuf {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        std::path::Path::new(&root).join("System32").join(name)
    }

    /// Reads the PE certificate-table data directory entry (index 4) and
    /// reports whether an embedded Authenticode signature blob exists.
    #[cfg(windows)]
    fn has_embedded_cert_table(path: &std::path::Path) -> bool {
        use std::io::{Read, Seek, SeekFrom};

        let Ok(mut f) = std::fs::File::open(path) else {
            return false;
        };
        let u16_at = |f: &mut std::fs::File, off: u64| -> Option<u16> {
                f.seek(SeekFrom::Start(off)).ok()?;
                let mut b = [0u8; 2];
                f.read_exact(&mut b).ok()?;
                Some(u16::from_le_bytes(b))
            };
        let u32_at =
            |f: &mut std::fs::File, off: u64| -> Option<u32> {
                f.seek(SeekFrom::Start(off)).ok()?;
                let mut b = [0u8; 4];
                f.read_exact(&mut b).ok()?;
                Some(u32::from_le_bytes(b))
            };

        let pe_off = match u32_at(&mut f, 0x3C) {
            Some(v) => v as u64,
            None => return false,
        };
        let magic = match u16_at(&mut f, pe_off + 24) {
            Some(v) => v,
            None => return false,
        };
        let dd = pe_off + 24 + if magic == 0x20B { 112 } else { 96 };
        // data directory index 4 = certificate table; +4 skips the VA to Size
        matches!(u32_at(&mut f, dd + 4 * 8 + 4), Some(size) if size > 0)
    }

    #[cfg(windows)]
    #[test]
    fn real_microsoft_binary_verifies_as_signed() {
        let notepad = system32("notepad.exe");
        if !notepad.is_file() {
            return;
        }
        assert_eq!(
            check_signature(&notepad),
            SignatureStatus::ValidSigned,
            "{notepad:?} should verify (embedded or via security catalog)"
        );
    }

    #[cfg(windows)]
    #[test]
    fn unsigned_test_binary_is_reported_unsigned() {
        // Our own cargo-built test runner is a perfectly normal PE that was
        // never signed and is claimed by no security catalog. (A garbage
        // non-PE file would answer Unknown instead — WinTrust cannot even
        // parse its subject form.)
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        assert_eq!(
            check_signature(&exe),
            SignatureStatus::Unsigned,
            "{exe:?} is a cargo build artifact and must verify as unsigned"
        );
    }

    #[cfg(windows)]
    #[test]
    fn tampered_system_binary_loses_its_signature_verdict() {
        use std::io::Write;

        let source = system32("chkdsk.exe");
        if !source.is_file() {
            return;
        }
        let had_embedded = has_embedded_cert_table(&source);

        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join("tampered-chkdsk.exe");
        let mut bytes = std::fs::read(&source).unwrap();
        let flip_at = bytes.len() / 2; // deep enough for executable content
        bytes[flip_at] ^= 0xFF;
        let mut out = std::fs::File::create(&copy).unwrap();
        out.write_all(&bytes).unwrap();
        drop(out);

        let status = check_signature(&copy);
        if had_embedded {
            assert_eq!(
                status,
                SignatureStatus::Invalid,
                "flipping a byte in an embedded-signed binary must break its digest"
            );
        } else {
            assert_eq!(
                status,
                SignatureStatus::Unsigned,
                "on catalog-signed systems a modified binary is no longer claimed by any catalog"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn nonexistent_file_is_unknown_not_an_error() {
        let ghost = std::env::temp_dir().join("cure-no-such-binary-evert.exe");
        let _ = std::fs::remove_file(&ghost);
        assert_eq!(check_signature(&ghost), SignatureStatus::Unknown);
    }
}
