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

    # Exclude quality.sarif itself: it's this script's own output filename,
    # and matches the *.sarif glob just like every input report. Without
    # this, re-running the pipeline locally (without wiping reports/ first)
    # folds the previous merge's runs into the new one, silently doubling
    # results on every repeated invocation instead of starting fresh.
    sarif_files = sorted(
        f for f in reports_dir.glob("*.sarif") if f.name != "quality.sarif"
    )

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
            tool_name = run.get("tool", {}).get("driver", {}).get("name", "unknown")

            if "results" in run and isinstance(run["results"], list):
                deduped_results = []

                for result in run["results"]:
                    # Normalize paths to be relative to project root
                    first_uri = ""
                    first_line = 0
                    if "locations" in result:
                        for location in result["locations"]:
                            if "physicalLocation" in location:
                                phys = location["physicalLocation"]
                                if "artifactLocation" in phys:
                                    artifact = phys["artifactLocation"]
                                    uri = artifact.get("uri", "")
                                    if uri.startswith("./"):
                                        uri = uri[2:]
                                        artifact["uri"] = uri
                                    if not first_uri:
                                        first_uri = uri
                                region = phys.get("region", {})
                                if not first_line:
                                    first_line = region.get("startLine", 0)

                    # A finding is a duplicate only if the same tool reports
                    # the same rule at the same location with the same
                    # message — this guards against stale reports left over
                    # from a prior local run.sh invocation being merged
                    # alongside a fresh one, not against distinct tools
                    # legitimately flagging the same line differently.
                    message_text = result.get("message", {}).get("text", "")
                    fingerprint = (
                        tool_name,
                        result.get("ruleId", ""),
                        first_uri,
                        first_line,
                        message_text,
                    )

                    if fingerprint in seen_findings:
                        continue
                    seen_findings.add(fingerprint)
                    deduped_results.append(result)

                run["results"] = deduped_results

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
