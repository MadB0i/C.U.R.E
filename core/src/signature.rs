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
//! Publisher names are extracted lazily for display only (see
//! [`signature_detail`]) and never feed scoring — signed malware exists.
//!
//! Revocation posture: whole-chain revocation is always requested. By default
//! (`RevocationMode::CacheOnly`) URL retrieval is forbidden so verification
//! never touches the network; chains whose revocation data is not cached
//! yield `ValidRevocationUnknown` instead of a verdict either way.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Outcome of Authenticode verification for one executable.
///
/// Display mapping used by the UI/report:
/// VALID = `ValidSigned`, INVALID = `Invalid`, UNSIGNED = `Unsigned`,
/// UNKNOWN = `Unknown` (unverifiable — file missing, unresolvable, or
/// WinTrust could not run), UNVERIFIED = `ValidRevocationUnknown` (the
/// signature itself verifies but revocation could not be checked against the
/// offline cache). There is deliberately no separate ERROR state: anything
/// unverifiable is UNKNOWN, and UNKNOWN is never scored against an entry —
/// absence of evidence is not evidence. UNVERIFIED is likewise never a
/// discount (not fully trusted) and never a penalty (not bad).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureStatus {
    /// A signature (embedded or catalog-backed) verifies cleanly, revocation
    /// included (or revocation was explicitly checked online via opt-in).
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
    /// The signature chain verifies but revocation status is UNKNOWN because
    /// the offline cache has no CRL/OCSP data for it (cache-only default, no
    /// network). Displayed as UNVERIFIED: never the trusted `-40` discount,
    /// never the `+40` penalty — scoreless evidence only.
    ValidRevocationUnknown,
}

/// How revocation is checked during [`check_signature`].
///
/// The default is [`RevocationMode::CacheOnly`]: revocation is verified
/// against the local Windows cache only, so C.U.R.E makes NO network calls.
/// [`RevocationMode::Online`] may fetch CRLs/OCSP and is strictly opt-in
/// (CLI `--online-revocation`); the GUI never enables it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RevocationMode {
    /// Whole-chain revocation, cache only. Offline-safe, zero network.
    #[default]
    CacheOnly,
    /// Whole-chain revocation with live fetch allowed. Explicit opt-in only.
    Online,
}

/// Process-wide revocation opt-in. `false` (default) = cache-only everywhere.
/// Flipped once at startup by the CLI `--online-revocation` flag; read on
/// every verification so scans stay consistent within a run.
static ONLINE_REVOCATION_ALLOWED: AtomicBool = AtomicBool::new(false);

/// Enable (`true`) or disable (`false`, default) online revocation fetching.
/// Off by default: C.U.R.E makes no network calls unless the operator opts in.
pub fn set_online_revocation_allowed(allowed: bool) {
    ONLINE_REVOCATION_ALLOWED.store(allowed, Ordering::SeqCst);
}

/// Effective revocation mode for this process.
pub fn revocation_mode() -> RevocationMode {
    if ONLINE_REVOCATION_ALLOWED.load(Ordering::SeqCst) {
        RevocationMode::Online
    } else {
        RevocationMode::CacheOnly
    }
}

/// Raw WinTrust flag words `(fdwRevocationChecks, dwProvFlags)` for a mode.
///
/// `WTD_REVOKE_WHOLECHAIN` (1) always; `WTD_CACHE_ONLY_URL_RETRIEVAL`
/// (0x10000) only in cache-only mode. Pure and cross-platform so the contract
/// is unit-tested without WinTrust; the Windows glue wraps these words in the
/// typed `windows` constants (same numeric values, verified against the
/// 0.58 projection).
pub fn wintrust_flag_words(mode: RevocationMode) -> (u32, u32) {
    match mode {
        RevocationMode::CacheOnly => (0x0000_0001, 0x0001_0000),
        RevocationMode::Online => (0x0000_0001, 0),
    }
}

/// Pure HRESULT → status mapping shared by the embedded and catalog stages.
///
/// - `S_OK` → valid (under whole-chain checking this means revocation
///   verified too).
/// - `TRUST_E_NOSIGNATURE` → unsigned (caller tries the catalog fallback).
/// - Revoked-signer codes → [`SignatureStatus::Invalid`] (untrusted, +40).
/// - Revocation-indeterminate codes (offline cache miss, no provider) →
///   [`SignatureStatus::ValidRevocationUnknown`] (scoreless, UNVERIFIED).
/// - Environmental codes → [`SignatureStatus::Unknown`].
/// - Anything else (bad digest, expired, untrusted root, …) → Invalid.
pub fn status_from_hresult(hr: i32) -> SignatureStatus {
    const S_OK: i32 = 0;
    const TRUST_E_NOSIGNATURE: i32 = 0x800B_0100u32 as i32;
    const CRYPT_E_FILE_ERROR: i32 = 0x8009_2003u32 as i32;
    const TRUST_E_PROVIDER_UNKNOWN: i32 = 0x800B_0001u32 as i32;
    const TRUST_E_ACTION_UNKNOWN: i32 = 0x800B_0002u32 as i32;
    const TRUST_E_SUBJECT_FORM_UNKNOWN: i32 = 0x800B_0003u32 as i32;
    // Revoked signers: untrusted, full penalty.
    const CERT_E_REVOKED: i32 = 0x800B_010Cu32 as i32;
    const CRYPT_E_REVOKED: i32 = 0x8009_2010u32 as i32;
    // Signature verifies but revocation cannot be established offline.
    const CRYPT_E_NO_REVOCATION_DLL: i32 = 0x8009_2011u32 as i32;
    const CRYPT_E_NO_REVOCATION_CHECK: i32 = 0x8009_2012u32 as i32;
    const CRYPT_E_REVOCATION_OFFLINE: i32 = 0x8009_2013u32 as i32;

    if hr == S_OK {
        return SignatureStatus::ValidSigned;
    }
    if hr == TRUST_E_NOSIGNATURE {
        return SignatureStatus::Unsigned;
    }
    if hr == CERT_E_REVOKED || hr == CRYPT_E_REVOKED {
        return SignatureStatus::Invalid;
    }
    if hr == CRYPT_E_NO_REVOCATION_DLL
        || hr == CRYPT_E_NO_REVOCATION_CHECK
        || hr == CRYPT_E_REVOCATION_OFFLINE
    {
        return SignatureStatus::ValidRevocationUnknown;
    }
    match hr {
        // environmental problems: no verdict rather than "bad"
        code if code == CRYPT_E_FILE_ERROR
            || code == TRUST_E_PROVIDER_UNKNOWN
            || code == TRUST_E_ACTION_UNKNOWN
            || code == TRUST_E_SUBJECT_FORM_UNKNOWN =>
        {
            SignatureStatus::Unknown
        }
        // anything else means verification RAN and FAILED:
        // tampered digest, bad cert chain, expired signer, distrust
        _ => SignatureStatus::Invalid,
    }
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

/// Verifies a file's Authenticode signature with the process revocation mode
/// (cache-only by default — no network; CLI `--online-revocation` opts in).
/// Non-Windows builds have no WinTrust and always answer `Unknown`.
pub fn check_signature(exe_path: &Path) -> SignatureStatus {
    check_signature_with_mode(exe_path, revocation_mode())
}

/// [`check_signature`] with an explicit [`RevocationMode`], bypassing the
/// process-wide opt-in. Prefer the global-aware entry points unless a caller
/// has a documented reason to pin a mode.
pub fn check_signature_with_mode(exe_path: &Path, mode: RevocationMode) -> SignatureStatus {
    signature_detail_with_mode(exe_path, mode).status
}

impl SignatureStatus {
    /// Stable display token for UI/report/export: VALID, INVALID,
    /// UNSIGNED, UNKNOWN, or UNVERIFIED (signature verifies, revocation
    /// unknown — never trust it like VALID, never score it like INVALID).
    /// UNKNOWN covers every unverifiable case (missing file, unresolvable
    /// path, WinTrust failure) and must never be presented as suspicion.
    pub fn label(&self) -> &'static str {
        match self {
            Self::ValidSigned => "VALID",
            Self::Invalid => "INVALID",
            Self::Unsigned => "UNSIGNED",
            Self::Unknown => "UNKNOWN",
            Self::ValidRevocationUnknown => "UNVERIFIED",
        }
    }
}

/// Verdict plus publisher extraction (both lazy — call only for display).
///
/// The publisher is the signer's `CERT_NAME_SIMPLE_DISPLAY_TYPE`
/// (typically "Microsoft Corporation", "Acme Inc"). It is informational:
/// UNSIGNED/UNKNOWN files have no publisher (`None`), and a present
/// publisher says nothing about intent — signed malware exists.
/// Extraction failures degrade to `publisher: None`; the verdict stands.
pub fn signature_detail(exe_path: &Path) -> SignatureDetail {
    signature_detail_with_mode(exe_path, revocation_mode())
}

/// [`signature_detail`] with an explicit [`RevocationMode`].
pub fn signature_detail_with_mode(exe_path: &Path, mode: RevocationMode) -> SignatureDetail {
    #[cfg(windows)]
    {
        let status = imp::verify_with_mode(exe_path, mode);
        let publisher = imp::signer_publisher(exe_path, mode);
        SignatureDetail { status, publisher }
    }
    #[cfg(not(windows))]
    {
        let _ = (exe_path, mode);
        SignatureDetail {
            status: SignatureStatus::Unknown,
            publisher: None,
        }
    }
}

/// Verdict + optional publisher for one executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureDetail {
    pub status: SignatureStatus,
    pub publisher: Option<String>,
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
        WINTRUST_CATALOG_INFO, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_DATA_UNION_CHOICE,
        WINTRUST_FILE_INFO, WTD_CHOICE_CATALOG, WTD_CHOICE_FILE, WTD_STATEACTION_CLOSE,
        WTD_STATEACTION_VERIFY, WTD_UI_NONE,
    };

    use super::{status_from_hresult, wintrust_flag_words, RevocationMode, SignatureStatus};

    /// NUL-terminated UTF-16 for Win32 string parameters.
    fn wide(os: &std::ffi::OsStr) -> Vec<u16> {
        let mut v: Vec<u16> = os.encode_wide().collect();
        v.push(0);
        v
    }

    pub(super) fn verify_with_mode(path: &Path, mode: RevocationMode) -> SignatureStatus {
        // Stage 1: embedded signature.
        let status = status_from_hresult(wintrust_file_verify(path, mode));
        if !matches!(status, SignatureStatus::Unsigned) {
            return status;
        }
        // Stage 2: catalog-backed signature.
        catalog_verify(path, mode)
    }

    /// Best-effort signer display name (`CERT_NAME_SIMPLE_DISPLAY_TYPE`).
    /// `None` on any failure — the caller keeps the verdict regardless.
    /// Read-only: opens the file's certificate view, never modifies trust.
    ///
    /// Embedded signatures are read from the file itself; catalog-signed
    /// system binaries (no embedded blob — trust comes from a `.cat`) fall
    /// back to the verifying catalog's own PKCS#7 signer.
    pub(super) fn signer_publisher(path: &Path, mode: RevocationMode) -> Option<String> {
        use windows::Win32::Security::Cryptography::{
            CERT_QUERY_CONTENT_FLAG_ALL, CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED,
        };
        if let Some(name) = publisher_from_query(path, CERT_QUERY_CONTENT_FLAG_ALL) {
            return Some(name);
        }
        let catalog = verified_catalog_path(path, mode)?;
        publisher_from_query(&catalog, CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED)
    }

    fn publisher_from_query(
        path: &Path,
        content_flags: windows::Win32::Security::Cryptography::CERT_QUERY_CONTENT_TYPE_FLAGS,
    ) -> Option<String> {
        use windows::Win32::Security::Cryptography::{
            CertCloseStore, CertEnumCertificatesInStore, CertFreeCertificateContext, CryptMsgClose,
            CryptQueryObject, CERT_QUERY_FORMAT_FLAG_ALL, CERT_QUERY_OBJECT_FILE, HCERTSTORE,
        };

        struct StoreGuard {
            store: HCERTSTORE,
            msg: *mut core::ffi::c_void,
        }
        impl Drop for StoreGuard {
            fn drop(&mut self) {
                unsafe {
                    if !self.msg.is_null() {
                        let _ = CryptMsgClose(Some(self.msg as *const core::ffi::c_void));
                    }
                    if !self.store.is_invalid() {
                        let _ = CertCloseStore(self.store, 0);
                    }
                }
            }
        }

        unsafe {
            let wpath = wide(path.as_os_str());
            let mut store = HCERTSTORE::default();
            let mut msg: *mut core::ffi::c_void = std::ptr::null_mut();
            CryptQueryObject(
                CERT_QUERY_OBJECT_FILE,
                wpath.as_ptr() as *const core::ffi::c_void,
                content_flags,
                CERT_QUERY_FORMAT_FLAG_ALL,
                0,
                None,
                None,
                None,
                Some(&mut store),
                Some(&mut msg),
                None,
            )
            .ok()?;
            let _guard = StoreGuard { store, msg };
            if store.is_invalid() || store == HCERTSTORE::default() {
                return None;
            }
            // The store holds the file's chain (embedded) or the claiming
            // catalog's chain. Prefer the first non-self-signed cert (the
            // end-entity signer); fall back to the first cert overall.
            // A null msg is normal for catalog-signed files — the store is
            // what matters.
            let mut certs: Vec<*const windows::Win32::Security::Cryptography::CERT_CONTEXT> =
                Vec::new();
            let mut prev: Option<*const windows::Win32::Security::Cryptography::CERT_CONTEXT> =
                None;
            loop {
                let ctx = CertEnumCertificatesInStore(store, prev);
                if ctx.is_null() {
                    break;
                }
                certs.push(ctx);
                if certs.len() >= 16 {
                    break;
                }
                prev = Some(ctx);
            }
            let chosen = certs
                .iter()
                .find(|c| !is_self_signed(&***c))
                .or_else(|| certs.first());
            let name = match chosen {
                Some(&ctx) => name_of(ctx),
                None => None,
            };
            for ctx in certs {
                let _ = CertFreeCertificateContext(Some(ctx));
            }
            name
        }
    }

    fn blob_bytes(blob: &windows::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB) -> &[u8] {
        if blob.pbData.is_null() || blob.cbData == 0 {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(blob.pbData, blob.cbData as usize) }
    }

    fn is_self_signed(ctx: &windows::Win32::Security::Cryptography::CERT_CONTEXT) -> bool {
        if ctx.pCertInfo.is_null() {
            return false;
        }
        let info = unsafe { &*ctx.pCertInfo };
        blob_bytes(&info.Subject) == blob_bytes(&info.Issuer)
            && !blob_bytes(&info.Subject).is_empty()
    }

    fn name_of(ctx: *const windows::Win32::Security::Cryptography::CERT_CONTEXT) -> Option<String> {
        use windows::Win32::Security::Cryptography::{
            CertGetNameStringW, CERT_NAME_SIMPLE_DISPLAY_TYPE,
        };
        unsafe {
            let len = CertGetNameStringW(ctx, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, None, None);
            if len <= 1 {
                return None;
            }
            let mut name = vec![0u16; len as usize];
            let got = CertGetNameStringW(
                ctx,
                CERT_NAME_SIMPLE_DISPLAY_TYPE,
                0,
                None,
                Some(name.as_mut_slice()),
            );
            if got <= 1 {
                return None;
            }
            let text = String::from_utf16_lossy(&name[..got as usize - 1]);
            let text = text.trim().to_string();
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
    }

    fn wintrust_file_verify(path: &Path, mode: RevocationMode) -> i32 {
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
                mode,
            )
        }
    }

    /// Catalog stage: does any security catalog claim this exact binary?
    fn catalog_verify(path: &Path, mode: RevocationMode) -> SignatureStatus {
        match verified_catalog_path(path, mode) {
            Some(_) => SignatureStatus::ValidSigned,
            None => {
                // Distinguish "no catalog claims it" (Unsigned) from
                // "could not even hash it" (Unknown).
                if catalog_file_hash(path).is_none() {
                    SignatureStatus::Unknown
                } else {
                    SignatureStatus::Unsigned
                }
            }
        }
    }

    /// Path of the first catalog that verifies for this file, if any.
    /// Shared by the catalog verdict and the publisher fallback.
    fn verified_catalog_path(path: &Path, mode: RevocationMode) -> Option<std::path::PathBuf> {
        let hash = catalog_file_hash(path)?;
        let wpath = wide(path.as_os_str());
        // The provider matches members by tag; system catalogs key them by file name.
        let file_name = path.file_name()?;
        let member_tag = wide(file_name);

        let mut admin: isize = 0;
        // NOTE: the GENERIC_VERIFY_V2 subsystem finds nothing here; the
        // driver-verification context indexes the system component catalogs
        // we need (verified empirically against Get-AuthenticodeSignature).
        if unsafe { CryptCATAdminAcquireContext(&mut admin, Some(&DRIVER_ACTION_VERIFY), 0) }
            .is_err()
        {
            return None;
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

        let mut verified_path: Option<std::path::PathBuf> = None;
        for &cat in &matched {
            let mut info = CATALOG_INFO {
                cbStruct: u32::try_from(std::mem::size_of::<CATALOG_INFO>()).unwrap_or(0),
                ..CATALOG_INFO::default()
            };
            if unsafe { CryptCATCatalogInfoFromContext(cat, &mut info, 0) }.is_err() {
                continue;
            }
            let catalog_path = wide_truncated(&info.wszCatalogFile);

            let mut catalog_info = WINTRUST_CATALOG_INFO {
                cbStruct: u32::try_from(std::mem::size_of::<WINTRUST_CATALOG_INFO>()).unwrap_or(0),
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
                    mode,
                )
            };
            if hr == 0 {
                verified_path = Some(std::path::PathBuf::from(String::from_utf16_lossy(
                    &catalog_path[..catalog_path.len().saturating_sub(1)],
                )));
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

        verified_path
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
    ///
    /// Revocation is always whole-chain. In cache-only mode the provider
    /// flags additionally forbid URL retrieval, so no CRL/OCSP fetch can
    /// leave the machine; a chain whose revocation data is not cached fails
    /// with an offline/indeterminate code instead of phoning home.
    unsafe fn run_winverifytrust(
        action_guid: &mut windows::core::GUID,
        union_choice: WINTRUST_DATA_UNION_CHOICE,
        anonymous: WINTRUST_DATA_0,
        mode: RevocationMode,
    ) -> i32 {
        use windows::Win32::Security::WinTrust::{
            WINTRUST_DATA_PROVIDER_FLAGS, WINTRUST_DATA_REVOCATION_CHECKS,
        };
        let (revocation, prov_flags) = wintrust_flag_words(mode);
        let mut data = WINTRUST_DATA {
            cbStruct: u32::try_from(std::mem::size_of::<WINTRUST_DATA>()).unwrap_or(0),
            dwUIChoice: WTD_UI_NONE,
            fdwRevocationChecks: WINTRUST_DATA_REVOCATION_CHECKS(revocation),
            dwProvFlags: WINTRUST_DATA_PROVIDER_FLAGS(prov_flags),
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
        assert_eq!(
            resolve_executable_path(&cmd).as_deref(),
            Some(exe.as_path())
        );
    }

    #[test]
    fn unquoted_leading_token_is_extracted() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("tool.exe");
        std::fs::write(&exe, b"MZ").unwrap();

        let cmd = format!("{} -silent", exe.display());
        assert_eq!(
            resolve_executable_path(&cmd).as_deref(),
            Some(exe.as_path())
        );
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
        let u32_at = |f: &mut std::fs::File, off: u64| -> Option<u32> {
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
        // Cache-only revocation (the default) makes this environment
        // dependent: a warm CRL/OCSP cache yields VALID; a cold one yields
        // UNVERIFIED. Both prove the chain verifies — neither may be
        // Unsigned/Invalid/Unknown for a stock system binary.
        assert!(
            matches!(
                check_signature(&notepad),
                SignatureStatus::ValidSigned | SignatureStatus::ValidRevocationUnknown
            ),
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

    #[cfg(windows)]
    #[test]
    fn missing_file_detail_is_unknown_without_publisher() {
        let ghost = std::env::temp_dir().join("cure-no-such-publisher-evert.exe");
        let _ = std::fs::remove_file(&ghost);
        let detail = signature_detail(&ghost);
        assert_eq!(detail.status, SignatureStatus::Unknown);
        assert_eq!(detail.publisher, None);
    }

    // --- Pure HRESULT mapping: every branch, no WinTrust needed. ---

    #[test]
    fn success_is_valid() {
        assert_eq!(status_from_hresult(0), SignatureStatus::ValidSigned);
    }

    #[test]
    fn no_signature_is_unsigned() {
        assert_eq!(
            status_from_hresult(0x800B_0100u32 as i32),
            SignatureStatus::Unsigned
        );
    }

    #[test]
    fn environmental_failures_are_unknown() {
        for hr in [
            0x8009_2003u32 as i32, // CRYPT_E_FILE_ERROR
            0x800B_0001u32 as i32, // TRUST_E_PROVIDER_UNKNOWN
            0x800B_0002u32 as i32, // TRUST_E_ACTION_UNKNOWN
            0x800B_0003u32 as i32, // TRUST_E_SUBJECT_FORM_UNKNOWN
        ] {
            assert_eq!(
                status_from_hresult(hr),
                SignatureStatus::Unknown,
                "hr={hr:#X}"
            );
        }
    }

    #[test]
    fn revoked_signers_are_invalid() {
        for hr in [
            0x800B_010Cu32 as i32, // CERT_E_REVOKED
            0x8009_2010u32 as i32, // CRYPT_E_REVOKED
        ] {
            assert_eq!(
                status_from_hresult(hr),
                SignatureStatus::Invalid,
                "hr={hr:#X}"
            );
        }
    }

    #[test]
    fn offline_revocation_is_valid_but_unverified() {
        for hr in [
            0x8009_2011u32 as i32, // CRYPT_E_NO_REVOCATION_DLL
            0x8009_2012u32 as i32, // CRYPT_E_NO_REVOCATION_CHECK
            0x8009_2013u32 as i32, // CRYPT_E_REVOCATION_OFFLINE
        ] {
            assert_eq!(
                status_from_hresult(hr),
                SignatureStatus::ValidRevocationUnknown,
                "hr={hr:#X}"
            );
        }
    }

    #[test]
    fn other_verification_failures_are_invalid() {
        for hr in [
            0x800B_0101u32 as i32, // CERT_E_EXPIRED
            0x800B_0109u32 as i32, // CERT_E_UNTRUSTEDROOT
            0x8009_6010u32 as i32, // TRUST_E_BAD_DIGEST
            0x800B_010Fu32 as i32, // CERT_E_WRONG_USAGE
            -1,                    // unexpected failure
        ] {
            assert_eq!(
                status_from_hresult(hr),
                SignatureStatus::Invalid,
                "hr={hr:#X}"
            );
        }
    }

    #[test]
    fn unverified_label_is_distinct_from_valid() {
        assert_eq!(
            SignatureStatus::ValidRevocationUnknown.label(),
            "UNVERIFIED"
        );
        assert_eq!(SignatureStatus::ValidSigned.label(), "VALID");
    }

    #[test]
    fn revocation_flag_words_match_wintrust_contract() {
        // WTD_REVOKE_WHOLECHAIN = 1, WTD_CACHE_ONLY_URL_RETRIEVAL = 0x10000.
        assert_eq!(
            wintrust_flag_words(RevocationMode::CacheOnly),
            (0x0000_0001, 0x0001_0000)
        );
        assert_eq!(
            wintrust_flag_words(RevocationMode::Online),
            (0x0000_0001, 0)
        );
    }

    #[test]
    fn revocation_defaults_to_cache_only() {
        // Read-only: the process default must be offline unless the CLI
        // opt-in ran. (The setter itself is intentionally untested here —
        // flipping process-global state from a test could race parallel
        // tests that verify real binaries.)
        assert_eq!(revocation_mode(), RevocationMode::CacheOnly);
        assert_eq!(RevocationMode::default(), RevocationMode::CacheOnly);
    }

    #[test]
    fn unverified_status_is_scoreless_in_risk() {
        // Belt-and-braces alongside risk.rs tests: UNVERIFIED must add no
        // discount and no penalty, only an evidence reason.
        let entry = crate::model::PersistenceEntry {
            id: "t".to_string(),
            name: "app.exe".to_string(),
            command: r"C:\Program Files\App\app.exe".to_string(),
            location: "loc".to_string(),
            source: crate::model::PersistenceSource::RegistryRun,
        };
        let scored =
            crate::risk::score_with_signals(&entry, SignatureStatus::ValidRevocationUnknown, None);
        assert_eq!(scored.score, 0);
        assert_eq!(scored.risk, crate::model::RiskLevel::Safe);
        assert!(
            scored
                .reasons
                .iter()
                .any(|r| r.contains("revocation unverified")),
            "reasons: {:?}",
            scored.reasons
        );
    }

    #[test]
    fn directories_and_empty_paths_are_unknown() {
        // WinTrust cannot verify a directory (or nothing) — Unknown, never
        // a risk signal in either direction.
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(check_signature(dir.path()), SignatureStatus::Unknown);
        let detail = signature_detail(dir.path());
        assert_eq!(detail.status, SignatureStatus::Unknown);
        assert_eq!(detail.publisher, None);
    }

    #[cfg(windows)]
    #[test]
    fn microsoft_binary_reports_microsoft_publisher() {
        let notepad = system32("notepad.exe");
        if !notepad.is_file() {
            return;
        }
        let detail = signature_detail(&notepad);
        // See real_microsoft_binary_verifies_as_signed: cold revocation
        // caches yield UNVERIFIED instead of VALID; the publisher fallback
        // works from the verifying catalog either way.
        assert!(
            matches!(
                detail.status,
                SignatureStatus::ValidSigned | SignatureStatus::ValidRevocationUnknown
            ),
            "unexpected status: {:?}",
            detail.status
        );
        let publisher = detail
            .publisher
            .expect("signed MS binary must name a publisher");
        assert!(
            publisher.to_ascii_lowercase().contains("microsoft"),
            "unexpected publisher: {publisher}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn unsigned_binary_has_no_publisher() {
        // Our own cargo-built test runner: a normal unsigned PE.
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let detail = signature_detail(&exe);
        assert_eq!(detail.status, SignatureStatus::Unsigned);
        assert_eq!(detail.publisher, None);
    }
}
