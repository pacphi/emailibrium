-- Audit log for MCP tool calls (ADR-028 Phase 6).
-- Dialect note (ADR-033): id is SQLite's auto-increment integer primary key -> GENERATED ALWAYS AS IDENTITY.
CREATE TABLE IF NOT EXISTS mcp_tool_audit (
    id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    timestamp TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    arguments_hash TEXT NOT NULL,
    result_status TEXT NOT NULL,
    latency_ms INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mcp_audit_timestamp ON mcp_tool_audit(timestamp);
CREATE INDEX IF NOT EXISTS idx_mcp_audit_tool ON mcp_tool_audit(tool_name);
