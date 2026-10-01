//! Northstar mod manifest script targets and load order.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{VmTargets, condition_targets};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LoadOrder {
    pub priority: i64,
    pub index: usize,
}

#[derive(Clone, Debug)]
pub struct ScriptEntry {
    pub targets: VmTargets,
    pub load_order: LoadOrder,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Manifest {
    #[serde(default)]
    load_priority: i64,
    #[serde(default)]
    scripts: Vec<ManifestScript>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ManifestScript {
    path: String,
    run_on: Option<String>,
}

pub fn read_manifest(manifest_path: &Path) -> Vec<(PathBuf, ScriptEntry)> {
    let Ok(text) = std::fs::read_to_string(manifest_path) else {
        return Vec::new();
    };
    let Ok(manifest) = serde_json::from_str::<Manifest>(&text) else {
        return Vec::new();
    };
    let Some(root) = manifest_path.parent() else {
        return Vec::new();
    };
    let scripts = root.join("mod").join("scripts").join("vscripts");
    manifest
        .scripts
        .into_iter()
        .enumerate()
        .map(|(index, script)| {
            let targets = script
                .run_on
                .as_deref()
                .map_or(VmTargets::ALL, condition_targets);
            (
                scripts.join(script.path.replace('\\', "/")),
                ScriptEntry {
                    targets,
                    load_order: LoadOrder {
                        priority: manifest.load_priority,
                        index,
                    },
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn reads_script_targets_and_load_order() {
        let root = loop {
            let suffix = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let candidate = std::env::temp_dir().join(format!(
                "sqformat-lint-manifest-{}-{suffix}",
                std::process::id()
            ));
            if std::fs::create_dir(&candidate).is_ok() {
                break candidate;
            }
        };
        let scripts = root.join("mod").join("scripts").join("vscripts");
        std::fs::create_dir_all(&scripts).unwrap();
        let manifest_path = root.join("mod.json");
        std::fs::write(
            &manifest_path,
            r#"{"LoadPriority":3,"Scripts":[{"Path":"ui/thing.nut","RunOn":"UI"},{"Path":"server\\thing.nut","RunOn":"SERVER"},{"Path":"shared.nut"}]}"#,
        )
        .unwrap();

        let entries = read_manifest(&manifest_path);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].0, scripts.join("ui/thing.nut"));
        assert_eq!(entries[0].1.targets, VmTargets::UI);
        assert_eq!(
            entries[0].1.load_order,
            LoadOrder {
                priority: 3,
                index: 0
            }
        );
        assert_eq!(entries[1].0, scripts.join("server/thing.nut"));
        assert_eq!(entries[1].1.targets, VmTargets::SERVER);
        assert_eq!(
            entries[1].1.load_order,
            LoadOrder {
                priority: 3,
                index: 1
            }
        );
        assert_eq!(entries[2].0, scripts.join("shared.nut"));
        assert_eq!(entries[2].1.targets, VmTargets::ALL);
        assert_eq!(
            entries[2].1.load_order,
            LoadOrder {
                priority: 3,
                index: 2
            }
        );

        std::fs::remove_dir_all(root).unwrap();
    }
}
