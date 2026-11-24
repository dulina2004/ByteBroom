# Storage Analyzer

Open **Analyzer** after scanning, or select **View analyzer** from Scan. Tile area represents logical file length within the active view. Select a tile or the corresponding list row to see the path, measurement status, and classification. Use breadcrumbs to return to a parent. The **Analyze this folder** action at the retained depth limit starts a new scan; browsing, sorting, and filtering the current snapshot use retained data.

## Accounting

- **Directories** show disjoint measured child folders, files directly in the current folder, and measured bytes from omitted child folders. A folder at the depth limit has a measured total with child detail available through an explicit scan.
- **Developer artifacts, projects, and ecosystems** are alternative views of detected artifact bytes. A project total covers its detected artifacts. An ecosystem owns each artifact's bytes once, and a project can contribute to multiple ecosystem project counts.
- **Global resources** use the independently selected Development Storage scope. Each grouping is an alternative view of those same measurements. Protected bytes are shown separately for interpretation and are part of the measured global total.
- **Logical size** is the sum of measured file lengths. Sparse files, compression, hard links, inaccessible data, and filesystem changes can affect comparison with physical storage. ByteBroom reports Windows capacity separately.

Byte totals use saturating unsigned 64-bit arithmetic. Treemap geometry uses wider integer weights for partition totals before converting proportions to floating-point coordinates.

The largest 512 rows per non-directory view are searchable in the Analyzer. Smaller retained rows are available in the detailed Artifacts or Dev Storage pages. A chart groups excess positive-size rows into **Other visible results**. Zero and unknown sizes remain distinguishable in the list. Partial totals describe the observed portion of the scan.

## Comparison and privacy

Select **Compare latest scans** to compare two complete scans of the same verified directory. When both immediate child lists are fully retained, the comparison can label newly or no longer detected directories. When either list is bounded, it compares only shared retained entries. Choose **Discard previous scan** to release the older snapshot. Analysis and paths remain local and in memory for the session. CSV and JSON export are deferred.

## Reproducible timing

Run `cargo test --release --locked benchmark_analysis -- --ignored --nocapture` on the Windows development machine. The fixture uses 1,000, 10,000, and 100,000 synthetic weighted items. It prints separate durations for aggregation, filtering and sorting the bounded result, and layout. It does not traverse a drive or time rendering. Record the machine, build profile, and timings for any published performance comparison.

One release-profile run on Windows build 26300 with an Intel Core i7-11800H produced the following timings. The application uses Slint's software renderer. The fixture measures Rust calculations with `Instant`.

| Input items | Aggregate and retain | Filter and sort | Treemap |
| ---: | ---: | ---: | ---: |
| 1,000 | 0.121 ms | 0.049 ms | 0.051 ms |
| 10,000 | 1.214 ms | 0.039 ms | 0.012 ms |
| 100,000 | 18.075 ms | 0.049 ms | 0.018 ms |

Each run retained 512 rows and produced 80 tiles. Startup, renderer latency, and peak memory are outside these timings.
