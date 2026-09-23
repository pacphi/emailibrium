-- Record which caller ran a tool (ADR-028 follow-on, task A1).
--
-- The MCP transport and the in-process chat orchestrator now share one dispatch
-- path, so without this column their calls are indistinguishable in the audit
-- trail. The default keeps rows written before this migration valid and lets an
-- un-migrated read still parse.
ALTER TABLE mcp_tool_audit ADD COLUMN source TEXT NOT NULL DEFAULT 'mcp';
