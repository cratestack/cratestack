#!/usr/bin/env python3
# Convert Semgrep JSON output to SARIF 2.1.0 format
# Usage: semgrep-to-sarif.py <input.json> <output.sarif>

import sys
import json
from pathlib import Path

def convert_semgrep_to_sarif(input_file, output_file):
    """Convert Semgrep JSON to SARIF 2.1.0."""
    input_path = Path(input_file)
    output_path = Path(output_file)

    if not input_path.exists():
        print(f"error: {input_file} not found", file=sys.stderr)
        sys.exit(1)

    try:
        with open(input_path) as f:
            semgrep_report = json.load(f)
    except json.JSONDecodeError as e:
        print(f"error: failed to parse {input_file}: {e}", file=sys.stderr)
        sys.exit(1)

    # Extract results and metadata
    semgrep_results = semgrep_report.get("results", [])
    semgrep_errors = semgrep_report.get("errors", [])
    semgrep_version = semgrep_report.get("version", "unknown")

    # Build SARIF results from Semgrep findings
    sarif_results = []

    for result in semgrep_results:
        rule_id = result.get("check_id", "unknown-rule")
        message_text = result.get("extra", {}).get("message", result.get("check_id", "No message"))
        severity = result.get("extra", {}).get("severity", "WARNING").upper()
        path = result.get("path", "unknown")

        # Map Semgrep severity to SARIF level
        level = "warning"
        if severity == "ERROR":
            level = "error"
        elif severity == "WARNING":
            level = "warning"
        elif severity == "INFO":
            level = "note"

        # Build location information
        start_line = result.get("start", {}).get("line", 0)
        start_col = result.get("start", {}).get("col", 0)
        end_line = result.get("end", {}).get("line", start_line)
        end_col = result.get("end", {}).get("col", start_col)

        # Normalize path to be relative
        if path.startswith("./"):
            path = path[2:]

        sarif_result = {
            "ruleId": rule_id,
            "message": {
                "text": message_text
            },
            "level": level,
            "locations": [
                {
                    "physicalLocation": {
                        "artifactLocation": {
                            "uri": path
                        },
                        "region": {
                            "startLine": start_line,
                            "startColumn": start_col + 1,  # SARIF uses 1-indexed columns
                            "endLine": end_line,
                            "endColumn": end_col + 1
                        }
                    }
                }
            ]
        }

        # Add additional context if available
        if "extra" in result:
            extra = result["extra"]
            if "lines" in extra:
                sarif_result["codeFlows"] = [
                    {
                        "message": {
                            "text": "Code snippet"
                        }
                    }
                ]

        sarif_results.append(sarif_result)

    # Build the SARIF run
    sarif_run = {
        "tool": {
            "driver": {
                "name": "semgrep",
                "version": semgrep_version,
                "informationUri": "https://semgrep.dev",
                "rules": []
            }
        },
        "results": sarif_results
    }

    # Collect unique rule information
    rules_seen = set()
    for result in sarif_results:
        rule_id = result.get("ruleId", "unknown")
        if rule_id not in rules_seen:
            rules_seen.add(rule_id)
            sarif_run["tool"]["driver"]["rules"].append({
                "id": rule_id,
                "shortDescription": {
                    "text": rule_id
                }
            })

    # Build final SARIF document
    sarif_document = {
        "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json",
        "version": "2.1.0",
        "runs": [sarif_run]
    }

    # Write output
    try:
        with open(output_path, "w") as f:
            json.dump(sarif_document, f, indent=2)
        print(f"Converted {input_file} → {output_file}", file=sys.stderr)
    except IOError as e:
        print(f"error: failed to write {output_file}: {e}", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: semgrep-to-sarif.py <input.json> <output.sarif>", file=sys.stderr)
        sys.exit(1)

    convert_semgrep_to_sarif(sys.argv[1], sys.argv[2])
