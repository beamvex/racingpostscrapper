#!/usr/bin/env python3
"""Extract mermaid diagrams from architecture.md and convert to PNG using mmdc."""

import re
import subprocess
import os
from pathlib import Path

def extract_mermaid_diagrams(md_file: str, output_dir: str):
    """Extract all mermaid diagrams from markdown file."""
    with open(md_file, 'r') as f:
        content = f.read()
    
    # Find all mermaid blocks
    pattern = r'```mermaid\n(.*?)\n```'
    matches = re.findall(pattern, content, re.DOTALL)
    
    print(f"Found {len(matches)} mermaid diagrams")
    
    os.makedirs(output_dir, exist_ok=True)
    
    diagram_names = [
        "high_level_architecture",
        "component_interaction",
        "end_to_end_data_pipeline",
        "daily_pipeline_steps",
        "vpc_networking",
        "ecs_cluster_architecture",
        "s3_bucket_structure",
        "lambda_api_gateway",
        "pipeline_orchestration",
        "time_based_filtering",
        "dynamic_scheduling",
        "rule_naming_convention",
        "timezone_handling",
        "module_organization",
        "scraping_module",
        "parser_module",
        "utilities",
        "lambda_architecture",
        "html_report_structure",
        "prefix_structure",
        "file_naming_conventions",
        "timezone_conversion_flow",
        "docker_build_process",
        "terraform_deployment",
        "ecs_task_deployment",
        "error_handling_pipeline",
        "error_handling_lambda",
    ]
    
    for i, diagram_code in enumerate(matches):
        if i < len(diagram_names):
            name = diagram_names[i]
        else:
            name = f"diagram_{i+1}"
        mmd_file = os.path.join(output_dir, f"{name}.mmd")
        png_file = os.path.join(output_dir, f"{name}.png")
        
        with open(mmd_file, 'w') as f:
            f.write(diagram_code)
        
        print(f"Extracted: {mmd_file}")
        
        # Convert to PNG using mmdc
        try:
            subprocess.run(
                ['mmdc', '-i', mmd_file, '-o', png_file, '-t', 'default', '-w', '1200'],
                check=True
            )
            print(f"Generated: {png_file}")
        except subprocess.CalledProcessError as e:
            print(f"Error converting {mmd_file}: {e}")
        except FileNotFoundError:
            print("mmdc not found. Install with: npm install -g @mermaid-js/mermaid-cli")
            return

if __name__ == "__main__":
    md_file = "/Users/robertforster/develop/racingpostscrapper/docs/architecture.md"
    output_dir = "/Users/robertforster/develop/racingpostscrapper/docs/diagrams"
    extract_mermaid_diagrams(md_file, output_dir)
