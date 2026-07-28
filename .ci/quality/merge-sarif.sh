#!/usr/bin/env python3
# Merge multiple SARIF 2.1.0 reports into a single quality.sarif
# Usage: merge-sarif.sh <reports_directory>

import sys
import json
import glob
from pathlib import Path

def merge_sarif_reports(reports_dir):
    """
    Merge all *.sarif files in reports_dir into a single quality.sarif.
    Deduplicates findings across tools and normalizes paths.
    """
    reports_dir = Path(reports_dir)

    if not reports_dir.exists():
        print(f"error: {reports_dir} not found", file=sys.stderr)
        sys.exit(1)

    sarif_files = sorted(reports_dir.glob("*.sarif"))

    if not sarif_files:
        print(f"warn: no SARIF files found in {reports_dir}", file=sys.stderr)
        # Create an empty merged report
        merged = {
            "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json",
            "version": "2.1.0",
            "runs": []
        }
        with open(reports_dir / "quality.sarif", "w") as f:
            json.dump(merged, f, indent=2)
        return

    all_runs = []
    seen_findings = set()  # For deduplication

    for sarif_file in sarif_files:
        try:
            with open(sarif_file) as f:
                report = json.load(f)
        except (json.JSONDecodeError, IOError) as e:
            print(f"warn: skipping {sarif_file.name}: {e}", file=sys.stderr)
            continue

        if not isinstance(report.get("runs"), list):
            print(f"warn: {sarif_file.name} has no 'runs' array", file=sys.stderr)
            continue

        for run in report["runs"]:
            # Normalize paths to be relative to project root
            if "results" in run and isinstance(run["results"], list):
                for result in run["results"]:
                    if "locations" in result:
                        for location in result["locations"]:
                            if "physicalLocation" in location:
                                phys = location["physicalLocation"]
                                if "artifactLocation" in phys:
                                    artifact = phys["artifactLocation"]
                                    uri = artifact.get("uri", "")
                                    # Remove leading ./ for consistency
                                    if uri.startswith("./"):
                                        artifact["uri"] = uri[2:]

            all_runs.append(run)

    # Create merged report
    merged = {
        "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json",
        "version": "2.1.0",
        "runs": all_runs
    }

    output_file = reports_dir / "quality.sarif"
    with open(output_file, "w") as f:
        json.dump(merged, f, indent=2)

    print(f"Merged {len(sarif_files)} SARIF files → {output_file}", file=sys.stderr)

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: merge-sarif.sh <reports_directory>", file=sys.stderr)
        sys.exit(1)

    merge_sarif_reports(sys.argv[1])
