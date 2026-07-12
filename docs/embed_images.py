#!/usr/bin/env python3
"""Replace mermaid code blocks with PNG image references in markdown."""

import re
import os
from pathlib import Path

def embed_png_images(md_file: str, output_file: str, diagrams_dir: str):
    """Replace mermaid blocks with PNG image references."""
    with open(md_file, 'r') as f:
        content = f.read()
    
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
    ]
    
    # Replace each mermaid block with image reference
    for i, name in enumerate(diagram_names):
        pattern = r'```mermaid\n.*?\n```'
        replacement = f'\n\n![{name.replace("_", " ").title()}](diagrams/{name}.png)\n\n'
        
        # Replace only the first occurrence
        content = re.sub(pattern, replacement, content, count=1)
    
    with open(output_file, 'w') as f:
        f.write(content)
    
    print(f"Embedded images in {output_file}")

if __name__ == "__main__":
    md_file = "/Users/robertforster/develop/racingpostscrapper/docs/architecture.md"
    output_file = "/Users/robertforster/develop/racingpostscrapper/docs/architecture_with_images.md"
    diagrams_dir = "/Users/robertforster/develop/racingpostscrapper/docs/diagrams"
    embed_png_images(md_file, output_file, diagrams_dir)
