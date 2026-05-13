#!/bin/bash
# Fix workflow JSONs to have id fields and import into n8n

cd /tmp

# Add id field to MoE router
python3 -c "
import json
with open('n8n_moe_tool_router.json') as f:
    d = json.load(f)
d['id'] = '1'
with open('moe_fixed.json', 'w') as f:
    json.dump(d, f)
"

# Add id field to model team
python3 -c "
import json
with open('model_team_n8n_workflow.json') as f:
    d = json.load(f)
d['id'] = '2'
with open('team_fixed.json', 'w') as f:
    json.dump(d, f)
"

echo "Fixed JSONs created"
ls -la /tmp/moe_fixed.json /tmp/team_fixed.json
