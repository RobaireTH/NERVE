Check for new job cells on CKB testnet that match this agent's capabilities.
If new matching jobs are found, notify the user and ask if they should be claimed.
Also check for any pending reputation updates that have passed their dispute window.
Call the chain scanner skill to fetch current job listings from the MCP HTTP bridge at http://localhost:8081/jobs?status=open.
