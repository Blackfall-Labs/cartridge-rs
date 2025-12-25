# Benchmark Data Markup Format

**Purpose:** Define a standardized way to mark up performance documentation with data spans that can be automatically updated from benchmark results.

---

## Markup Format

Use HTML comments to denote data insertion points:

```markdown
<!-- BENCH_DATA: benchmark_name.metric_name [format] -->actual_value<!-- /BENCH_DATA -->
```

### Components:

1. **benchmark_name**: The Criterion benchmark name (e.g., `file_write_64kb`)
2. **metric_name**: The specific metric (e.g., `mean_throughput`, `lower_bound`, `upper_bound`)
3. **format**: Optional format specifier (e.g., `GiB/s`, `ms`, `μs`, `blocks/ms`)
4. **actual_value**: The current value (will be replaced by parser)

---

## Example Usage

### File I/O Performance Table

```markdown
| Operation | Throughput | Notes |
|-----------|------------|-------|
| **Read (64KB)** | <!-- BENCH_DATA: file_read_64kb.mean_throughput [GiB/s] -->17.91<!-- /BENCH_DATA --> GiB/s | Mean throughput |
| **Write (64KB)** | <!-- BENCH_DATA: file_write_64kb.mean_throughput [GiB/s] -->9.41<!-- /BENCH_DATA --> GiB/s | Mean throughput |
```

### Compression Performance

```markdown
**Peak LZ4 Compression:** <!-- BENCH_DATA: lz4_compress_64kb.peak_throughput [GiB/s] -->9.77<!-- /BENCH_DATA --> GiB/s at 64KB blocks
**Peak LZ4 Decompression:** <!-- BENCH_DATA: lz4_decompress_64kb.peak_throughput [GiB/s] -->38.12<!-- /BENCH_DATA --> GiB/s at 64KB blocks
```

### Latency Measurements

```markdown
- Mean Latency: <!-- BENCH_DATA: allocation_extent.mean_latency [μs] -->10.4<!-- /BENCH_DATA --> μs
- Lower Bound: <!-- BENCH_DATA: allocation_extent.lower_bound [μs] -->7.67<!-- /BENCH_DATA --> μs
- Upper Bound: <!-- BENCH_DATA: allocation_extent.upper_bound [μs] -->7.98<!-- /BENCH_DATA --> μs
```

---

## Supported Metrics

### Throughput Metrics
- `mean_throughput`: Mean throughput (primary metric)
- `peak_throughput`: Upper bound throughput
- `low_throughput`: Lower bound throughput

### Latency Metrics
- `mean_latency`: Mean time per operation
- `lower_bound`: Lower bound latency (95% CI)
- `upper_bound`: Upper bound latency (95% CI)

### Count Metrics
- `operations_per_second`: Operations/sec
- `items_per_second`: Items processed/sec
- `blocks_per_ms`: Blocks allocated/ms

---

## Benchmark Result Parser

### Input Format (Criterion JSON)

Criterion outputs benchmark results in JSON format to `target/criterion/*/estimates.json`:

```json
{
  "mean": {
    "point_estimate": 1234.5,
    "confidence_interval": {
      "lower_bound": 1200.0,
      "upper_bound": 1260.0
    }
  },
  "slope": { ... }
}
```

### Conversion Logic

```python
def parse_bench_data(benchmark_name, metric_name):
    """
    Parse Criterion benchmark results and extract metric

    Args:
        benchmark_name: Name of the benchmark (e.g., "file_write_64kb")
        metric_name: Metric to extract (e.g., "mean_throughput")

    Returns:
        Numeric value for the metric
    """
    # Read estimates.json
    path = f"target/criterion/{benchmark_name}/estimates.json"
    with open(path) as f:
        data = json.load(f)

    # Map metric names to JSON paths
    if metric_name == "mean_throughput":
        # Throughput = size / time
        mean_time_ns = data["mean"]["point_estimate"]
        # Benchmark sets throughput via Criterion::throughput()
        # Read from benchmark metadata
        return calculate_throughput(mean_time_ns)

    elif metric_name == "mean_latency":
        return data["mean"]["point_estimate"]

    elif metric_name == "lower_bound":
        return data["mean"]["confidence_interval"]["lower_bound"]

    elif metric_name == "upper_bound":
        return data["mean"]["confidence_interval"]["upper_bound"]
```

### Update Script

```python
#!/usr/bin/env python3
"""
update_benchmark_data.py

Automatically update benchmark data spans in documentation
"""

import re
import json
from pathlib import Path

BENCH_DATA_PATTERN = r'<!-- BENCH_DATA: ([\w_.]+)\[([\w/]+)\] -->(.+?)<!-- /BENCH_DATA -->'

def update_file(file_path: Path, benchmark_data: dict):
    """Update all BENCH_DATA spans in a file"""
    content = file_path.read_text()

    def replace_span(match):
        bench_metric = match.group(1)  # e.g., "file_write_64kb.mean_throughput"
        format_spec = match.group(2)   # e.g., "GiB/s"
        old_value = match.group(3)     # Current value

        # Look up new value from benchmark data
        new_value = benchmark_data.get(bench_metric)
        if new_value is None:
            print(f"Warning: No data for {bench_metric}, keeping {old_value}")
            return match.group(0)  # Keep original

        # Format the value
        formatted = format_value(new_value, format_spec)

        # Return updated span
        return f'<!-- BENCH_DATA: {bench_metric}[{format_spec}] -->{formatted}<!-- /BENCH_DATA -->'

    updated_content = re.sub(BENCH_DATA_PATTERN, replace_span, content)

    if updated_content != content:
        file_path.write_text(updated_content)
        print(f"Updated {file_path}")
    else:
        print(f"No changes needed for {file_path}")

def format_value(value: float, format_spec: str) -> str:
    """Format a value according to the format specifier"""
    if format_spec == "GiB/s":
        return f"{value:.2f}"
    elif format_spec in ["ms", "μs", "ns"]:
        return f"{value:.2f}"
    elif format_spec == "blocks/ms":
        return f"{int(value):,}"
    else:
        return str(value)

# Usage:
# python update_benchmark_data.py docs/performance.md README.md
```

---

## Example: Complete Performance Table with Spans

```markdown
## File Operations Performance

Tested on **AMD Ryzen 9 7950X** with NVMe SSD:

| Size | Mean Latency | Throughput | Lower Bound | Upper Bound |
|------|--------------|------------|-------------|-------------|
| 1KB | <!-- BENCH_DATA: file_write_1kb.mean_latency [μs] -->1.52<!-- /BENCH_DATA --> μs | <!-- BENCH_DATA: file_write_1kb.mean_throughput [MiB/s] -->643.96<!-- /BENCH_DATA --> MiB/s | <!-- BENCH_DATA: file_write_1kb.lower_throughput [MiB/s] -->600.47<!-- /BENCH_DATA --> MiB/s | <!-- BENCH_DATA: file_write_1kb.upper_throughput [MiB/s] -->683.70<!-- /BENCH_DATA --> MiB/s |
| 4KB | <!-- BENCH_DATA: file_write_4kb.mean_latency [μs] -->1.25<!-- /BENCH_DATA --> μs | <!-- BENCH_DATA: file_write_4kb.mean_throughput [GiB/s] -->3.05<!-- /BENCH_DATA --> GiB/s | <!-- BENCH_DATA: file_write_4kb.lower_throughput [GiB/s] -->2.79<!-- /BENCH_DATA --> GiB/s | <!-- BENCH_DATA: file_write_4kb.upper_throughput [GiB/s] -->3.29<!-- /BENCH_DATA --> GiB/s |
| 64KB | <!-- BENCH_DATA: file_write_64kb.mean_latency [μs] -->6.48<!-- /BENCH_DATA --> μs | <!-- BENCH_DATA: file_write_64kb.mean_throughput [GiB/s] -->9.41<!-- /BENCH_DATA --> GiB/s | <!-- BENCH_DATA: file_write_64kb.lower_throughput [GiB/s] -->9.22<!-- /BENCH_DATA --> GiB/s | <!-- BENCH_DATA: file_write_64kb.upper_throughput [GiB/s] -->9.59<!-- /BENCH_DATA --> GiB/s |

**Peak Write Performance:** <!-- BENCH_DATA: file_write_64kb.upper_throughput [GiB/s] -->9.59<!-- /BENCH_DATA --> GiB/s at 64KB block size
```

---

## Benefits

1. **Accuracy**: Values come directly from benchmark results
2. **Traceability**: Each value is linked to specific benchmark
3. **Automation**: Can be updated automatically after each benchmark run
4. **Verification**: Easy to compare claimed vs measured values
5. **Transparency**: Markers show which benchmarks support which claims

---

## Workflow

### After Running Benchmarks:

```bash
# 1. Run benchmarks
cargo bench

# 2. Extract data and update documentation
python scripts/update_benchmark_data.py \
    --input target/criterion \
    --docs README.md docs/performance.md docs/ARCHITECTURE.md

# 3. Review changes
git diff README.md docs/performance.md

# 4. Commit
git add README.md docs/performance.md
git commit -m "docs: Update benchmark data from latest run"
```

---

## Manual Verification

To verify a claim:

1. Find the BENCH_DATA comment in the source
2. Extract the benchmark name (e.g., `file_write_64kb.mean_throughput`)
3. Look up in `target/criterion/file_write_64kb/estimates.json`
4. Confirm the value matches

Example:
```bash
# Claim in README: "Write: 9.41 GiB/s"
# Find: <!-- BENCH_DATA: file_write_64kb.mean_throughput [GiB/s] -->9.41<!-- /BENCH_DATA -->
# Verify:
jq '.mean.point_estimate' target/criterion/file_write_64kb/estimates.json
```

---

## Next Steps

1. Add BENCH_DATA spans to existing documentation
2. Create `scripts/update_benchmark_data.py`
3. Fix benchmark compilation issues
4. Run benchmarks and test automated update
5. Add to CI/CD pipeline

---

**Note:** This is a proposed format. Implementation is pending benchmark fixes documented in BENCHMARK_STATUS.md.
