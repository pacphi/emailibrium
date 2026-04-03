// JSON Schema validation for rule definitions

use super::schema::RuleSet;
use anyhow::{Context, Result};
use serde_json::json;

/// JSON Schema v7 validator for rule definitions
pub struct RuleValidator {
    schema: serde_json::Value,
}

impl RuleValidator {
    /// Create a new validator with the default rule schema
    pub fn new() -> Self {
        Self {
            schema: Self::default_schema(),
        }
    }

    /// Create a validator with a custom schema
    pub fn with_schema(schema: serde_json::Value) -> Self {
        Self { schema }
    }

    /// Get the default JSON Schema v7 for rule definitions
    fn default_schema() -> serde_json::Value {
        json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "title": "Email Rule Schema",
            "type": "object",
            "required": ["version", "rules"],
            "properties": {
                "version": {
                    "type": "string",
                    "pattern": "^[0-9]+\\.[0-9]+$"
                },
                "rules": {
                    "type": "array",
                    "items": {
                        "$ref": "#/definitions/rule"
                    },
                    "maxItems": 5000
                }
            },
            "definitions": {
                "rule": {
                    "type": "object",
                    "required": ["id", "name", "conditions", "actions"],
                    "properties": {
                        "id": {
                            "type": "string",
                            "minLength": 1
                        },
                        "name": {
                            "type": "string",
                            "minLength": 1
                        },
                        "description": {
                            "type": "string"
                        },
                        "enabled": {
                            "type": "boolean"
                        },
                        "priority": {
                            "type": "string",
                            "enum": ["low", "medium", "high", "critical"]
                        },
                        "conditions": {
                            "$ref": "#/definitions/condition_node"
                        },
                        "actions": {
                            "type": "array",
                            "items": {
                                "$ref": "#/definitions/action"
                            },
                            "minItems": 1
                        },
                        "metadata": {
                            "type": "object"
                        }
                    }
                },
                "condition_node": {
                    "oneOf": [
                        {
                            "$ref": "#/definitions/single_condition"
                        },
                        {
                            "$ref": "#/definitions/composite_condition"
                        }
                    ]
                },
                "single_condition": {
                    "type": "object",
                    "required": ["field", "operator", "value"],
                    "properties": {
                        "field": {
                            "type": "string",
                            "enum": [
                                "subject", "from", "to", "cc", "bcc",
                                "body", "body_text", "body_html",
                                "label", "is_read", "is_starred", "is_archived",
                                "received_at", "received_date", "received_time",
                                "message_id"
                            ]
                        },
                        "operator": {
                            "type": "string",
                            "enum": [
                                "equals", "not_equals",
                                "contains", "not_contains",
                                "starts_with", "ends_with",
                                "matches",
                                "greater_than", "less_than",
                                "greater_or_equal", "less_or_equal",
                                "in", "not_in"
                            ]
                        },
                        "value": {},
                        "case_sensitive": {
                            "type": "boolean"
                        }
                    }
                },
                "composite_condition": {
                    "type": "object",
                    "required": ["op", "conditions"],
                    "properties": {
                        "op": {
                            "type": "string",
                            "enum": ["AND", "OR", "NOT"]
                        },
                        "conditions": {
                            "type": "array",
                            "items": {
                                "$ref": "#/definitions/condition_node"
                            },
                            "minItems": 1
                        }
                    }
                },
                "action": {
                    "type": "object",
                    "required": ["type"],
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": [
                                "add_label", "remove_label", "move_to",
                                "mark_read", "mark_unread",
                                "star", "unstar",
                                "archive", "delete",
                                "forward", "set_priority", "script", "stop"
                            ]
                        }
                    },
                    "allOf": [
                        {
                            "if": {
                                "properties": {
                                    "type": {
                                        "const": "add_label"
                                    }
                                }
                            },
                            "then": {
                                "required": ["labels"],
                                "properties": {
                                    "labels": {
                                        "type": "array",
                                        "items": {
                                            "type": "string"
                                        },
                                        "minItems": 1
                                    }
                                }
                            }
                        },
                        {
                            "if": {
                                "properties": {
                                    "type": {
                                        "const": "remove_label"
                                    }
                                }
                            },
                            "then": {
                                "required": ["labels"],
                                "properties": {
                                    "labels": {
                                        "type": "array",
                                        "items": {
                                            "type": "string"
                                        },
                                        "minItems": 1
                                    }
                                }
                            }
                        },
                        {
                            "if": {
                                "properties": {
                                    "type": {
                                        "const": "move_to"
                                    }
                                }
                            },
                            "then": {
                                "required": ["folder"],
                                "properties": {
                                    "folder": {
                                        "type": "string",
                                        "minLength": 1
                                    }
                                }
                            }
                        },
                        {
                            "if": {
                                "properties": {
                                    "type": {
                                        "const": "forward"
                                    }
                                }
                            },
                            "then": {
                                "required": ["to"],
                                "properties": {
                                    "to": {
                                        "type": "array",
                                        "items": {
                                            "type": "string",
                                            "format": "email"
                                        },
                                        "minItems": 1
                                    }
                                }
                            }
                        },
                        {
                            "if": {
                                "properties": {
                                    "type": {
                                        "const": "set_priority"
                                    }
                                }
                            },
                            "then": {
                                "required": ["priority"],
                                "properties": {
                                    "priority": {
                                        "type": "string",
                                        "enum": ["low", "medium", "high", "critical"]
                                    }
                                }
                            }
                        },
                        {
                            "if": {
                                "properties": {
                                    "type": {
                                        "const": "script"
                                    }
                                }
                            },
                            "then": {
                                "required": ["code"],
                                "properties": {
                                    "code": {
                                        "type": "string",
                                        "minLength": 1
                                    }
                                }
                            }
                        }
                    ]
                }
            }
        })
    }

    /// Validate a RuleSet against the schema
    pub fn validate(&self, ruleset: &RuleSet) -> Result<()> {
        // First, validate using the schema's own validation logic
        ruleset
            .validate()
            .map_err(|e| anyhow::anyhow!("Schema validation failed: {}", e))?;

        // Convert RuleSet to JSON for schema validation
        let json_value =
            serde_json::to_value(ruleset).context("Failed to serialize ruleset to JSON")?;

        // Compile and validate against JSON schema
        let compiled_schema = jsonschema::draft7::new(&self.schema)
            .map_err(|e| anyhow::anyhow!("Failed to compile JSON schema: {}", e))?;

        if let Err(validation_error) = compiled_schema.validate(&json_value) {
            // Extract the error message from the validation error
            let error_msg = format!("JSON Schema validation failed: {}", validation_error);
            anyhow::bail!(error_msg);
        }

        Ok(())
    }

    /// Validate and return detailed error information
    pub fn validate_detailed(&self, ruleset: &RuleSet) -> Result<Vec<String>> {
        let mut errors = Vec::new();

        // Schema-level validation
        if let Err(e) = ruleset.validate() {
            errors.push(e);
        }

        // JSON schema validation
        let json_value = serde_json::to_value(ruleset).context("Failed to serialize ruleset")?;

        let compiled_schema = jsonschema::draft7::new(&self.schema)
            .map_err(|e| anyhow::anyhow!("Failed to compile JSON schema: {}", e))?;

        if let Err(validation_error) = compiled_schema.validate(&json_value) {
            errors.push(format!(
                "JSON Schema validation failed: {}",
                validation_error
            ));
        }

        if errors.is_empty() {
            Ok(vec!["Validation successful".to_string()])
        } else {
            Err(anyhow::anyhow!("Validation errors:\n{}", errors.join("\n")))
        }
    }

    /// Get the schema as JSON string
    pub fn schema_json(&self) -> Result<String> {
        serde_json::to_string_pretty(&self.schema).context("Failed to serialize schema")
    }
}

impl Default for RuleValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to validate a RuleSet
pub fn validate_ruleset(ruleset: &RuleSet) -> Result<()> {
    RuleValidator::new().validate(ruleset)
}
