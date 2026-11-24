use std::io;

#[cfg(windows)]
mod windows;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Capacity {
    total_bytes: u64,
    free_bytes: u64,
}

impl Capacity {
    pub(crate) fn new(total_bytes: u64, free_bytes: u64) -> Option<Self> {
        (total_bytes > 0 && free_bytes <= total_bytes).then_some(Self {
            total_bytes,
            free_bytes,
        })
    }

    pub(crate) fn total_bytes(self) -> u64 {
        self.total_bytes
    }

    pub(crate) fn free_bytes(self) -> u64 {
        self.free_bytes
    }

    pub(crate) fn used_bytes(self) -> u64 {
        self.total_bytes - self.free_bytes
    }

    pub(crate) fn used_fraction(self) -> f64 {
        self.used_bytes() as f64 / self.total_bytes as f64
    }
}

pub(crate) struct Drive {
    pub(crate) letter: char,
    pub(crate) label: String,
    pub(crate) kind: &'static str,
    pub(crate) capacity: Capacity,
}

#[derive(Default)]
pub(crate) struct Discovery {
    pub(crate) drives: Vec<Drive>,
    pub(crate) unavailable: Vec<char>,
}

impl Discovery {
    pub(crate) fn total_capacity(&self) -> Option<Capacity> {
        let (total, free) =
            self.drives
                .iter()
                .try_fold((0_u64, 0_u64), |(total, free), drive| {
                    Some((
                        total.checked_add(drive.capacity.total_bytes())?,
                        free.checked_add(drive.capacity.free_bytes())?,
                    ))
                })?;
        Capacity::new(total, free)
    }
}

/// Read volume metadata for drive letters visible to the current Windows session.
pub(crate) fn discover() -> io::Result<Discovery> {
    #[cfg(windows)]
    return windows::discover();

    #[cfg(not(windows))]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Drive discovery is currently available on Windows only.",
    ))
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut unit = 1;
    let mut divisor = 1024;
    while unit < UNITS.len() - 1 && bytes / divisor >= 1024 {
        unit += 1;
        divisor *= 1024;
    }

    // Round in tenths without converting the byte count to a floating-point value.
    let mut tenths = (u128::from(bytes) * 10 + u128::from(divisor) / 2) / u128::from(divisor);
    if tenths == 10240 && unit < UNITS.len() - 1 {
        unit += 1;
        tenths = 10;
    }
    format!("{}.{} {}", tenths / 10, tenths % 10, UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::{Capacity, Discovery, Drive, format_bytes};

    #[test]
    fn capacity_reconciles_and_reports_usage() {
        let capacity = Capacity::new(800, 200).unwrap();
        assert_eq!(capacity.total_bytes(), 800);
        assert_eq!(capacity.used_bytes(), 600);
        assert_eq!(capacity.free_bytes(), 200);
        assert_eq!(capacity.used_fraction(), 0.75);
        assert_eq!(Capacity::new(1, 0).unwrap().used_fraction(), 1.0);
        assert_eq!(Capacity::new(1, 1).unwrap().used_fraction(), 0.0);
    }

    #[test]
    fn capacity_rejects_missing_or_inconsistent_values() {
        assert_eq!(Capacity::new(0, 0), None);
        assert_eq!(Capacity::new(100, 101), None);
    }

    #[test]
    fn discovery_totals_listed_drives_and_handles_overflow() {
        let drive = |letter, total, free| Drive {
            letter,
            label: String::new(),
            kind: "Fixed drive",
            capacity: Capacity::new(total, free).unwrap(),
        };
        let mut result = Discovery {
            drives: vec![drive('C', 800, 200), drive('D', 400, 100)],
            unavailable: vec!['E'],
        };
        assert_eq!(result.total_capacity(), Capacity::new(1200, 300));

        result.drives.push(drive('F', u64::MAX, 0));
        assert_eq!(result.total_capacity(), None);
        assert_eq!(Discovery::default().total_capacity(), None);
    }

    #[test]
    fn formats_binary_units_and_rounds_across_boundaries() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes((1 << 20) - 1), "1.0 MiB");
        assert_eq!(format_bytes(512 << 30), "512.0 GiB");
        assert_eq!(format_bytes(1 << 40), "1.0 TiB");
        assert_eq!(format_bytes(u64::MAX), "16.0 EiB");
    }
}
