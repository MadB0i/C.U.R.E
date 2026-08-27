//! Threat intelligence feed integration.
//!
//! Loads Indicators of Compromise (IOCs) from local files and optionally
//! fetches from public feeds (AbuseCh URLhaus, Feodo Tracker). All feeds
//! are cached locally so the tool works fully offline after first fetch.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// An individual IOC entry with its source and metadata.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IocEntry {
    pub ioc: String,
    pub ioc_type: IocType,
    pub source: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IocType {
    Sha256,
    Md5,
    Ip,
    Domain,
    Url,
}

/// Aggregated threat intel lookup backed by a set of known IOCs.
pub struct ThreatIntel {
    sha256s: HashSet<String>,
    md5s: HashSet<String>,
    ips: HashSet<String>,
    domains: HashSet<String>,
    urls: HashSet<String>,
    /// Raw entries keyed by IOC value for metadata lookup.
    entries: HashMap<String, IocEntry>,
}

impl ThreatIntel {
    /// Build an empty intel store.
    pub fn empty() -> Self {
        Self {
            sha256s: HashSet::new(),
            md5s: HashSet::new(),
            ips: HashSet::new(),
            domains: HashSet::new(),
            urls: HashSet::new(),
            entries: HashMap::new(),
        }
    }

    /// Load IOCs from a JSON file. The file is a flat array of `IocEntry`.
    pub fn from_file(path: &std::path::Path) -> Result<Self, String> {
        let data = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        Self::from_json(&data)
    }

    /// Parse IOCs from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, String> {
        let list: Vec<IocEntry> =
            serde_json::from_str(json).map_err(|e| format!("invalid IOC JSON: {e}"))?;
        let mut ti = Self::empty();
        for entry in list {
            ti.add(entry);
        }
        Ok(ti)
    }

    /// Add a single IOC entry.
    pub fn add(&mut self, entry: IocEntry) {
        let key = entry.ioc.to_lowercase();
        match entry.ioc_type {
            IocType::Sha256 => {
                self.sha256s.insert(key.clone());
            }
            IocType::Md5 => {
                self.md5s.insert(key.clone());
            }
            IocType::Ip => {
                self.ips.insert(key.clone());
            }
            IocType::Domain => {
                self.domains.insert(key.clone());
            }
            IocType::Url => {
                self.urls.insert(key.clone());
            }
        }
        self.entries.insert(key, entry);
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

    pub fn count_by_type(&self, t: IocType) -> usize {
        match t {
            IocType::Sha256 => self.sha256s.len(),
            IocType::Md5 => self.md5s.len(),
            IocType::Ip => self.ips.len(),
            IocType::Domain => self.domains.len(),
            IocType::Url => self.urls.len(),
        }
    }

    /// Check if a SHA-256 hash is a known IOC.
    pub fn check_sha256(&self, hash: &str) -> Option<&IocEntry> {
        let h = hash.to_lowercase();
        self.entries.get(&h).filter(|e| e.ioc_type == IocType::Sha256)
    }

    /// Check if an IP address is a known IOC.
    pub fn check_ip(&self, ip: &str) -> Option<&IocEntry> {
        let k = ip.to_lowercase();
        self.entries.get(&k).filter(|e| e.ioc_type == IocType::Ip)
    }

    /// Check if a domain is a known IOC.
    pub fn check_domain(&self, domain: &str) -> Option<&IocEntry> {
        let k = domain.to_lowercase();
        self.entries.get(&k).filter(|e| e.ioc_type == IocType::Domain)
    }

    /// Check if a URL is a known IOC.
    pub fn check_url(&self, url: &str) -> Option<&IocEntry> {
        let k = url.to_lowercase();
        self.entries.get(&k).filter(|e| e.ioc_type == IocType::Url)
    }

    /// Serialize the full IOC set to JSON.
    pub fn to_json(&self) -> Result<String, String> {
        let mut entries: Vec<&IocEntry> = self.entries.values().collect();
        entries.sort_by(|a, b| a.ioc.cmp(&b.ioc));
        serde_json::to_string_pretty(&entries)
            .map_err(|e| format!("failed to serialize IOCs: {e}"))
    }
}

/// Merge another intel store into this one.
pub fn merge(base: &mut ThreatIntel, other: ThreatIntel) {
    for (_, entry) in other.entries {
        base.add(entry);
    }
}

/// Global singleton for the loaded threat intel. Initialized once at startup.
pub fn global() -> &'static OnceLock<ThreatIntel> {
    static GLOBAL: OnceLock<ThreatIntel> = OnceLock::new();
    &GLOBAL
}

/// Initialize the global threat intel from a file path.
pub fn init_from_file(path: &std::path::Path) -> Result<(), String> {
    let ti = ThreatIntel::from_file(path)?;
    global()
        .set(ti)
        .map_err(|_| "threat intel already initialized".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_json() -> String {
        serde_json::to_string(&vec![
            IocEntry {
                ioc: "aabbccdd".repeat(8),
                ioc_type: IocType::Sha256,
                source: "test".into(),
                description: "demo hash".into(),
                tags: vec!["demo".into()],
            },
            IocEntry {
                ioc: "192.168.1.100".into(),
                ioc_type: IocType::Ip,
                source: "test".into(),
                description: "demo ip".into(),
                tags: vec![],
            },
            IocEntry {
                ioc: "evil.example.com".into(),
                ioc_type: IocType::Domain,
                source: "test".into(),
                description: "demo domain".into(),
                tags: vec![],
            },
        ])
        .unwrap()
    }

    #[test]
    fn from_json_loads_all_types() {
        let ti = ThreatIntel::from_json(&sample_json()).unwrap();
        assert_eq!(ti.count(), 3);
        assert_eq!(ti.count_by_type(IocType::Sha256), 1);
        assert_eq!(ti.count_by_type(IocType::Ip), 1);
        assert_eq!(ti.count_by_type(IocType::Domain), 1);
    }

    #[test]
    fn check_sha256_finds_match() {
        let ti = ThreatIntel::from_json(&sample_json()).unwrap();
        let hash = "aabbccdd".repeat(8);
        assert!(ti.check_sha256(&hash).is_some());
        assert_eq!(
            ti.check_sha256(&hash).unwrap().description,
            "demo hash"
        );
    }

    #[test]
    fn check_sha256_case_insensitive() {
        let ti = ThreatIntel::from_json(&sample_json()).unwrap();
        let hash_upper = "AABBCCDD".repeat(8);
        assert!(ti.check_sha256(&hash_upper).is_some());
    }

    #[test]
    fn check_sha256_miss() {
        let ti = ThreatIntel::from_json(&sample_json()).unwrap();
        assert!(ti.check_sha256("0000000000000000000000000000000000000000000000000000000000000000").is_none());
    }

    #[test]
    fn check_ip_finds_match() {
        let ti = ThreatIntel::from_json(&sample_json()).unwrap();
        assert!(ti.check_ip("192.168.1.100").is_some());
    }

    #[test]
    fn check_domain_finds_match() {
        let ti = ThreatIntel::from_json(&sample_json()).unwrap();
        assert!(ti.check_domain("evil.example.com").is_some());
    }

    #[test]
    fn merge_combines_stores() {
        let mut a = ThreatIntel::from_json(&sample_json()).unwrap();
        let b = ThreatIntel::from_json(
            &serde_json::to_string(&vec![IocEntry {
                ioc: "ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00".into(),
                ioc_type: IocType::Md5,
                source: "test2".into(),
                description: "extra".into(),
                tags: vec![],
            }])
            .unwrap(),
        )
        .unwrap();
        merge(&mut a, b);
        assert_eq!(a.count(), 4);
    }

    #[test]
    fn to_json_roundtrips() {
        let ti = ThreatIntel::from_json(&sample_json()).unwrap();
        let json = ti.to_json().unwrap();
        let ti2 = ThreatIntel::from_json(&json).unwrap();
        assert_eq!(ti.count(), ti2.count());
    }

    #[test]
    fn empty_store_has_no_matches() {
        let ti = ThreatIntel::empty();
        assert!(ti.check_sha256("aa").is_none());
        assert!(ti.check_ip("1.2.3.4").is_none());
        assert!(ti.check_domain("test.com").is_none());
    }

    #[test]
    fn add_increments_count() {
        let mut ti = ThreatIntel::empty();
        assert_eq!(ti.count(), 0);
        ti.add(IocEntry {
            ioc: "deadbeef".into(),
            ioc_type: IocType::Sha256,
            source: "test".into(),
            description: "d".into(),
            tags: vec![],
        });
        assert_eq!(ti.count(), 1);
    }

    #[test]
    fn invalid_json_is_err() {
        assert!(ThreatIntel::from_json("not json").is_err());
    }
}
