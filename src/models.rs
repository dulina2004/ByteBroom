pub(crate) struct Drive {
    pub(crate) letter: &'static str,
    pub(crate) name: &'static str,
    pub(crate) total_gb: u32,
    pub(crate) used_gb: u32,
    pub(crate) reclaimable_gb: u32,
}

impl Drive {
    pub(crate) fn free_gb(&self) -> u32 {
        self.total_gb - self.used_gb
    }
}

pub(crate) const MOCK_DRIVES: [Drive; 2] = [
    Drive {
        letter: "C:",
        name: "System",
        total_gb: 512,
        used_gb: 371,
        reclaimable_gb: 48,
    },
    Drive {
        letter: "D:",
        name: "Projects",
        total_gb: 1024,
        used_gb: 663,
        reclaimable_gb: 94,
    },
];

#[cfg(test)]
mod tests {
    use super::MOCK_DRIVES;

    #[test]
    fn mock_drives_keep_reclaimable_space_within_used_space() {
        for drive in MOCK_DRIVES {
            assert!(drive.reclaimable_gb <= drive.used_gb);
            assert!(drive.used_gb <= drive.total_gb);
        }
    }

    #[test]
    fn mock_overview_totals_reconcile() {
        let total: u32 = MOCK_DRIVES.iter().map(|drive| drive.total_gb).sum();
        let used: u32 = MOCK_DRIVES.iter().map(|drive| drive.used_gb).sum();
        let free: u32 = MOCK_DRIVES.iter().map(|drive| drive.free_gb()).sum();

        assert_eq!(total, used + free);
        assert_eq!(free, 502);
    }
}
