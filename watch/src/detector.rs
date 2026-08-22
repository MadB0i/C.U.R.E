use std::collections::HashSet;

pub fn newly_arrived(previous: &HashSet<String>, current: &HashSet<String>) -> Vec<String> {
    let mut arrived: Vec<String> = current.difference(previous).cloned().collect();
    arrived.sort();
    arrived
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reports_new_drives_sorted() {
        let previous = set(&["C:\\", "D:\\"]);
        let current = set(&["C:\\", "D:\\", "A:\\", "E:\\"]);
        assert_eq!(newly_arrived(&previous, &current), vec!["A:\\", "E:\\"]);
    }

    #[test]
    fn ignores_removed_and_unchanged_drives() {
        let previous = set(&["C:\\", "D:\\", "E:\\"]);
        let current = set(&["C:\\"]);
        assert!(newly_arrived(&previous, &current).is_empty());
    }

    #[test]
    fn first_poll_reports_everything() {
        let previous = HashSet::new();
        let current = set(&["F:\\", "C:\\", "Z:\\"]);
        assert_eq!(newly_arrived(&previous, &current), vec!["C:\\", "F:\\", "Z:\\"]);
    }

    #[test]
    fn no_change_yields_nothing() {
        let both = set(&["C:\\", "X:\\"]);
        assert!(newly_arrived(&both, &both).is_empty());
        assert!(newly_arrived(&HashSet::new(), &HashSet::new()).is_empty());
    }
}
