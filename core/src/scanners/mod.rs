pub mod scheduled_tasks;
#[cfg(windows)]
pub mod registry;
pub mod startup;

use std::path::Path;

use crate::model::PersistenceEntry;

pub fn collect_all(startup_root: &Path, tasks_root: &Path) -> Vec<PersistenceEntry> {
    let mut all = Vec::new();
    all.extend(startup::scan(startup_root));
    all.extend(scheduled_tasks::scan(tasks_root));
    #[cfg(windows)]
    all.extend(registry::scan().unwrap_or_default());
    all
}
