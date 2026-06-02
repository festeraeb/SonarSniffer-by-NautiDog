# integrate/unmapped/laptopdump_wreckhunter_build/config_agent.py

## Verdict
ARCHIVE_STUB

## Target path
/codebase/projects/pipelines/archive/laptopdump_wreckhunter_build/config_agent.py

## Steps
1. Move file to `/codebase/projects/pipelines/archive/laptopdump_wreckhunter_build/config_agent.py`
2. Prepend deprecation header: `# DEPRECATED: Legacy interactive config helper. Use fleet YAML/JSON config schema instead.`
3. Strip hardcoded Windows path (`r"C:\Users\thomf\..."`) and replace with
