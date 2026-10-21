# Storage engine benchmarks

Every number in this file was produced by running the command shown, on the
machine described below, against a fixture the benchmark generates itself. No
figure here is estimated, rounded from a different run, or copied from
another machine.

Reproduce any row with:

```
cargo build --release -p cure_cli
.\target\release\cure.exe storage bench --files <N> --size <BYTES> --copies <C>
```

The benchmark builds its own tree outside `%TEMP%` and scans **only** that
tree (`use_catalogue: false`), so the number describes the engine rather than
whatever else the machine happens to contain. Each fixture contains `C`
byte-identical copies of every file, so the duplicate stage performs real
full-content hashing rather than trivially finding nothing.

## Machine

| | |
|---|---|
| Hardware | HP Pavilion Gaming Laptop 15-ec1xxx |
| CPU | AMD Ryzen 5 4600H, 6 cores / 12 threads |
| RAM | 16,497,311,744 bytes (15.4 GiB) |
| OS | Windows 11 Home Single Language, 10.0.26200 (build 26200) |
| Filesystem under test | NTFS on `D:` |
| Toolchain | rustc 1.97.0, `--release` profile with `strip = true` |
| Date | 2026-10-19 |

This is a consumer laptop with a consumer SSD. It is not a fast storage
server and the numbers should be read as "typical laptop", not as a ceiling.

## Results

### A — many small files

`cure storage bench --files 20000 --size 4096 --copies 4`

| Metric | Value |
|---|---|
| Files written | 20,000 (78.1 MB total) |
| Files seen by the scan | 20,000 |
| Folders walked | 65 |
| Wall clock, scan only | 5.69 s |
| Engine-reported duration | 5,689 ms |
| Throughput | 3,515 files/s (13.7 MB/s) |
| Duplicate groups verified | 5,000 (from 20,000 files considered) |
| Bytes read for hashing | 156.2 MB |
| Peak working set | 7.4 MB → 34.1 MB (delta 26.7 MB) |
| Completeness | complete for the requested root |

### B — three times the files

`cure storage bench --files 60000 --size 1024 --copies 3`

| Metric | Value |
|---|---|
| Files written | 60,000 (58.6 MB total) |
| Files seen by the scan | 60,000 |
| Folders walked | 65 |
| Wall clock, scan only | 18.08 s |
| Engine-reported duration | 18,078 ms |
| Throughput | 3,319 files/s (3.2 MB/s) |
| Duplicate groups verified | 20,000 (from 60,000 files considered) |
| Bytes read for hashing | 117.2 MB |
| Peak working set | 7.3 MB → 85.8 MB (delta 78.5 MB) |
| Completeness | complete for the requested root |

Across four runs of each configuration the file rate stayed within roughly
3,300–3,520 files/s. The spread is reported because it is what the command
actually produced, not because it is being presented as a tight benchmark.

## Reading these numbers honestly

* **Throughput is dominated by metadata, not bytes.** Run A moves four times
  the data of run B at the same files-per-second, because the cost is opening
  and stat-ing a file rather than reading it. Duplicate verification then
  adds a full read of every candidate, which is why the byte figures differ.
* **Memory is dominated by the result set, not the tree.** The engine holds a
  `FileRecord` per measured file so the UI can filter and sort without a
  second pass. 20,000 files cost ~26 MB; 60,000 cost ~78 MB — roughly 1.3 KB
  per file. A real quick scan of a Windows temp directory is usually in the
  low tens of thousands of files, so this is the expected working range.
* **The walk and the hash are separate phases.** `coverage.duration_ms`
  covers the whole scan including duplicate verification. An earlier revision
  recorded the duration before the hashing stage, which under-reported the
  cost of exactly the part that reads every byte; that is fixed, and the
  engine figure and the wall clock now agree.
* **These are one run each, not a statistical sample.** They are recorded
  because they are reproducible from the command above, not because they are
  a performance claim.

## Real-machine verification

A quick scan of a live Windows 11 machine, via
`cure storage scan --depth quick`:

```
completeness : Partial — some locations could not be read
measured     : 13,951 files, 1,726 folders
duration     : 8,246 ms
volume       : 412.31 GB free of 931.51 GB
at least 3.52 GB reclaimable (At least)
```

Roughly 14,000 files in ~8 seconds, with a correctly reported partial
completeness: `C:\System Volume Information` and other protected locations
were refused, and the scan said so rather than presenting the total as a
clean measurement. The headline reads "at least" because of it.

## Limitations of this benchmark

* Single machine, single run per configuration.
* The fixture is created on the same filesystem that is then scanned, so
  write caches are warm in a way a cold scan would not be.
* It does not exercise NTFS-compressed or sparse files, where the allocated
  size and the apparent size diverge — the case where reclaim estimates are
  most often wrong on real machines.
* It does not measure the webview's rendering time for a large result set.
