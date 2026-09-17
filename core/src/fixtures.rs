//! Synthetic test fixtures for unit tests — and ONLY for unit tests.
//!
//! Every name, path, hash, and byte string in this module is deliberately
//! fake (`CURE-SYNTH-*`, `C:\cure-synth\…`, `example.invalid`). Nothing here
//! is real malware, a real IOC, or a real machine path. Fixtures exist so
//! scoring, parsing, and provider logic can be tested deterministically on
//! any platform without touching the live system.

use crate::model::{PersistenceEntry, PersistenceSource};
use crate::scanners::services::{ServiceRecord, ServiceStart};

/// Microsoft-style SAFE startup entry: signed system binary, boring name.
pub fn safe_startup_entry() -> PersistenceEntry {
    PersistenceEntry::new(
        PersistenceSource::StartupFolder,
        "CURE-SYNTH-OneDriveStandaloneUpdater",
        r"C:\Windows\System32\CURE-SYNTH-Updater.exe /background",
        r"C:\cure-synth\Startup\CURE-SYNTH-OneDriveStandaloneUpdater.lnk",
    )
}

/// Signed third-party program-files entry (SAFE by location).
pub fn safe_third_party_entry() -> PersistenceEntry {
    PersistenceEntry::new(
        PersistenceSource::RegistryRun,
        "CURE-SYNTH-AcmeTray",
        r#""C:\Program Files\CURE-SYNTH-Acme\acmetray.exe" /quiet"#,
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
    )
}

/// Human-readable description of what a fixture is FOR (used in test names
/// and failure messages, never in product output).
pub fn label(entry: &PersistenceEntry) -> String {
    format!("{} ({})", entry.name, entry.source.tag())
}

/// REVIEW: unsigned executable in a user-writable drop zone.
/// NOTE: the path embeds real drop-zone token shapes
/// (`\Users\…\AppData\Local\Temp`) under a synthetic user so the heuristic
/// under test actually fires; nothing here exists on a real machine.
pub fn review_dropzone_entry() -> PersistenceEntry {
    PersistenceEntry::new(
        PersistenceSource::StartupFolder,
        "CURE-SYNTH-SyncHelper.exe",
        r"C:\Users\CURE-SYNTH\AppData\Local\Temp\CURE-SYNTH-SyncHelper.exe -q",
        r"C:\Users\CURE-SYNTH\AppData\Local\Temp\CURE-SYNTH-SyncHelper.exe",
    )
}

/// REVIEW: hidden PowerShell invocation.
pub fn review_hidden_powershell_entry() -> PersistenceEntry {
    PersistenceEntry::new(
        PersistenceSource::ScheduledTask,
        r"CURE-SYNTH\TelemetryUpload",
        "powershell.exe -WindowStyle Hidden -File C:\\cure-synth\\tools\\upload.ps1",
        r"C:\cure-synth\Tasks\CURE-SYNTH\TelemetryUpload",
    )
}

/// REVIEW: unknown publisher, user-writable startup executable.
pub fn review_unknown_publisher_entry() -> PersistenceEntry {
    PersistenceEntry::new(
        PersistenceSource::StartupFolder,
        "CURE-SYNTH-PortableTool.exe",
        r"C:\cure-synth\PortableApps\CURE-SYNTH-PortableTool.exe",
        r"C:\cure-synth\Startup\CURE-SYNTH-PortableTool.exe",
    )
}

/// HIGH: multiple strong indicators (drop zone + random-looking name).
pub fn high_multi_indicator_entry() -> PersistenceEntry {
    PersistenceEntry::new(
        PersistenceSource::StartupFolder,
        "xk9q2zv7m1qCURE-SYNTH",
        r"C:\Users\CURE-SYNTH\AppData\Local\Temp\xk9q2zv7m1qCURE-SYNTH.exe --silent",
        r"C:\Users\CURE-SYNTH\AppData\Local\Temp\xk9q2zv7m1qCURE-SYNTH.exe",
    )
}

/// HIGH: invalid signature combined with a suspicious location
/// (signature is injected at scoring time — see risk tests).
pub fn high_suspicious_location_entry() -> PersistenceEntry {
    PersistenceEntry::new(
        PersistenceSource::ScheduledTask,
        r"CURE-SYNTH\Updater",
        r"C:\Users\CURE-SYNTH\Downloads\CURE-SYNTH-updater.exe --silent",
        r"C:\cure-synth\Tasks\CURE-SYNTH\Updater",
    )
}

/// SAFE service: signed system binary, automatic.
pub fn safe_service_record() -> ServiceRecord {
    service_record(
        "CURE-SYNTH-TimeBroker",
        r"C:\Windows\System32\CURE-SYNTH-TimeBroker.exe -k netsvcs",
        ServiceStart::Automatic,
        "LocalSystem",
        "Running",
    )
}

/// REVIEW service: unsigned third-party image under Program Files.
pub fn review_service_record() -> ServiceRecord {
    service_record(
        "CURE-SYNTH-AcmeAgent",
        r"C:\Program Files\CURE-SYNTH-Acme\agent.exe --service",
        ServiceStart::AutomaticDelayed,
        "LocalSystem",
        "Running",
    )
}

/// HIGH service: missing image + automatic + LocalSystem.
pub fn high_missing_service_record() -> ServiceRecord {
    service_record(
        "CURE-SYNTH-NativePush",
        r"C:\cure-synth\NativePush\CURE-SYNTH-Push.exe",
        ServiceStart::Automatic,
        "LocalSystem",
        "Stopped",
    )
}

fn service_record(
    name: &str,
    image: &str,
    start: ServiceStart,
    account: &str,
    state: &str,
) -> ServiceRecord {
    ServiceRecord {
        entry: PersistenceEntry::new(
            PersistenceSource::WindowsService,
            name,
            image,
            format!(r"HKLM\SYSTEM\CurrentControlSet\Services\{name}"),
        ),
        display_name: format!("CURE-SYNTH {name}"),
        start_type: start,
        state: state.to_string(),
        account: account.to_string(),
        image_path: image.to_string(),
        pid: None,
    }
}

/// Normal scheduled-task XML (single Exec, author, logon trigger).
pub fn normal_task_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>CURE-SYNTH Vendor</Author>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <Enabled>true</Enabled>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>C:\Program Files\CURE-SYNTH-Vendor\updater.exe</Command>
      <Arguments>/checknow</Arguments>
      <WorkingDirectory>C:\Program Files\CURE-SYNTH-Vendor</WorkingDirectory>
    </Exec>
  </Actions>
</Task>"#
        .to_string()
}

/// REVIEW task XML: two Exec actions, highest run level, hidden PowerShell.
pub fn multi_action_task_xml() -> String {
    r#"<?xml version="1.0"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>CURE-SYNTH Unknown</Author>
  </RegistrationInfo>
  <Triggers>
    <BootTrigger>
      <Enabled>true</Enabled>
    </BootTrigger>
    <CalendarTrigger>
      <Enabled>false</Enabled>
      <StartBoundary>2026-01-01T03:00:00</StartBoundary>
    </CalendarTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>S-1-5-18</UserId>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <Enabled>true</Enabled>
    <Hidden>true</Hidden>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>powershell.exe</Command>
      <Arguments>-WindowStyle Hidden -File C:\cure-synth\tools\stage1.ps1</Arguments>
    </Exec>
    <Exec>
      <Command>C:\cure-synth\Temp\CURE-SYNTH-stage2.exe</Command>
      <Arguments>--silent</Arguments>
      <WorkingDirectory>C:\cure-synth\Temp</WorkingDirectory>
    </Exec>
  </Actions>
</Task>"#
        .to_string()
}

/// Malformed task XML variants: unclosed tags, empty command, entities.
pub fn malformed_task_xmls() -> Vec<(&'static str, String)> {
    vec![
        ("unclosed", "<Task><Actions><Exec><Command>foo.exe".to_string()),
        (
            "empty-command",
            "<Task><Actions><Exec><Command>   </Command></Exec></Actions></Task>".to_string(),
        ),
        (
            "entities",
            "<Task><Actions><Exec><Command>C:\\cure-synth\\a &amp; b\\x.exe</Command></Exec></Actions></Task>"
                .to_string(),
        ),
        ("not-xml", "this is not xml at all".to_string()),
        ("empty", String::new()),
    ]
}

/// Minimal valid .lnk bytes (Unicode args/working-dir + target carried in
/// an EnvironmentVariableDataBlock, avoiding PIDL construction).
/// Hand-built per MS-SHLLINK: header + empty LinkTargetIDList + empty
/// LinkInfo + StringData(NAME, WORKING_DIR, ARGUMENTS as Unicode) +
/// ExtraData(EnvironmentVariableDataBlock with TargetUnicode) + terminal.
pub fn minimal_lnk_unicode(target: &str, args: &str, workdir: &str) -> Vec<u8> {
    let mut b = Vec::new();
    // HeaderSize + CLSID + LinkFlags(HasName|HasWorkingDir|HasArguments|IsUnicode) + attrs...
    b.extend_from_slice(&76u32.to_le_bytes());
    b.extend_from_slice(&[
        0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x46,
    ]);
    let flags: u32 = 0x0000_0084 | 0x0000_0020 | 0x0000_0010 | 0x0000_0004; // Unicode|Args|WorkDir|Name
    b.extend_from_slice(&flags.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes()); // attributes
    b.extend_from_slice(&[0u8; 8 + 8 + 8 + 4]); // creation/access/write times + filesize
    b.extend_from_slice(&0i32.to_le_bytes()); // icon
    b.extend_from_slice(&1u32.to_le_bytes()); // show
    b.extend_from_slice(&0u16.to_le_bytes()); // hotkey
    b.extend_from_slice(&[0u8; 10]); // reserved
    b.extend_from_slice(&0u16.to_le_bytes()); // target id list size = 0
                                              // No LinkInfo at all (HasLinkInfo unset) — not even a size field.
                                              // StringData: NAME, WORKING_DIR, ARGUMENTS (each: u16 char-count incl NUL + UTF-16)
    for s in ["CURE-SYNTH-Updater", workdir, args] {
        let chars: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        b.extend_from_slice(&(chars.len() as u16).to_le_bytes());
        for u in chars {
            b.extend_from_slice(&u.to_le_bytes());
        }
    }
    // ExtraData: EnvironmentVariableDataBlock (0xA0000001, size 0x314)
    b.extend_from_slice(&0x314u32.to_le_bytes());
    b.extend_from_slice(&0xA000_0001u32.to_le_bytes());
    let mut ansi = [0u8; 260];
    let tb = target.as_bytes();
    let n = tb.len().min(259);
    ansi[..n].copy_from_slice(&tb[..n]);
    b.extend_from_slice(&ansi);
    let mut uni = [0u8; 520];
    let tu: Vec<u16> = target.encode_utf16().collect();
    let m = tu.len().min(259);
    for (i, u) in tu.iter().take(m).enumerate() {
        uni[2 * i..2 * i + 2].copy_from_slice(&u.to_le_bytes());
    }
    b.extend_from_slice(&uni);
    b.extend_from_slice(&0u32.to_le_bytes()); // terminal block
    b
}

/// Malformed .lnk byte strings: bad magic, truncated header, empty.
pub fn malformed_lnks() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("bad-magic", vec![0u8; 100]),
        ("truncated", vec![0x4C, 0x00, 0x00]),
        ("empty", Vec::new()),
    ]
}
