import glob

yaml_header = """---
title: "Formalizing Filesystem Lifecycle Semantics:\\nAn Object-Centric Process Mining Framework for Autonomic Artifact Management"
author: "Sean Chatman"
date: "June 2026"
geometry: margin=1in
fontsize: 12pt
documentclass: report
toc: true
numbersections: true
header-includes:
  - \\usepackage{setspace}
  - \\setstretch{1.5}
---

"""

files = [
    'docs/thesis/chapters/00_Frontmatter.md',
    'docs/thesis/chapters/00_Glossary.md',
    'docs/thesis/chapters/01_Introduction.md',
    'docs/thesis/chapters/02_Literature_Review.md',
    'docs/thesis/chapters/03_Mathematical_Formalisms.md',
    'docs/thesis/chapters/04_System_Architecture.md',
    'docs/thesis/chapters/05_Empirical_Evaluation.md',
    'docs/thesis/chapters/06_Conclusion.md',
    'docs/thesis/chapters/07_References.md'
]

with open('docs/thesis/CHATMAN_SEAN_Formalizing_Filesystem_Lifecycle_Semantics.md', 'w') as out:
    out.write(yaml_header)
    for f in files:
        with open(f, 'r') as infile:
            out.write(infile.read() + '\n\n')
