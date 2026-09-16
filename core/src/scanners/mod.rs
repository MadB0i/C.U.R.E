pub mod appinit;
pub mod com;
pub mod ifeo;
pub mod scheduled_tasks;
#[cfg(windows)]
pub mod registry;
pub mod services;
pub mod startup;
#[cfg(windows)]
pub mod wmi;

use std::path::Path;

use crate::model::PersistenceEntry;

pub fn collect_all(startup_root: &Path, tasks_root: &Path) -> Vec<PersistenceEntry> {
    let mut all = Vec::new();
    all.extend(startup::scan(startup_root));
    all.extend(scheduled_tasks::scan(tasks_root));
    #[cfg(windows)]
    {
        all.extend(registry::scan().unwrap_or_default());
        all.extend(ifeo::scan());
        all.extend(appinit::scan());
        all.extend(com::scan());
        all.extend(wmi::scan());
    }
    all
}

/// Auto-start service records (scored separately via `risk::score_service`
/// — services carry start-type/account evidence plain entries lack).
/// Their unified `entry` halves are included for baseline diffing and
/// quarantine-lookup (manual guidance, never auto-remediation).
pub fn collect_services() -> Vec<services::ServiceRecord> {
    services::scan()
}
