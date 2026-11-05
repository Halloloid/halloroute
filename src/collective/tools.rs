//! Tool-call validation for the collective workflow state machine.

use crate::collective::types::ToolWorkflowState;
use crate::errors::{ApiError, ApiResult};

/// Validates a proposed tool call without executing workspace mutations.
///
/// The state machine enforces that tool calls can only be submitted when the
/// workflow is actively waiting for one, preventing races and double-execution.
pub fn validate_tool_call(
    state: ToolWorkflowState,
    tool_call: &serde_json::Value,
) -> ApiResult<ToolWorkflowState> {
    if state != ToolWorkflowState::AwaitingCall {
        return Err(ApiError::InvalidRequest(
            "tool call is not valid in the current collective workflow state".into(),
        ));
    }
    let name = tool_call
        .get("function")
        .and_then(|f| f.get("name"))
        .and_then(serde_json::Value::as_str)
        .filter(|n| !n.is_empty());
    if name.is_none() {
        return Err(ApiError::InvalidRequest(
            "collective tool call must include function.name".into(),
        ));
    }
    Ok(ToolWorkflowState::CallValidated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn valid_call_advances_state() {
        let call = json!({ "function": { "name": "read_file" } });
        let next = validate_tool_call(ToolWorkflowState::AwaitingCall, &call).unwrap();
        assert_eq!(next, ToolWorkflowState::CallValidated);
    }

    #[test]
    fn wrong_state_is_rejected() {
        let call = json!({ "function": { "name": "read_file" } });
        assert!(validate_tool_call(ToolWorkflowState::CallValidated, &call).is_err());
        assert!(validate_tool_call(ToolWorkflowState::Completed, &call).is_err());
    }

    #[test]
    fn missing_function_name_is_rejected() {
        let call_no_fn = json!({});
        assert!(validate_tool_call(ToolWorkflowState::AwaitingCall, &call_no_fn).is_err());

        let call_empty_name = json!({ "function": { "name": "" } });
        assert!(validate_tool_call(ToolWorkflowState::AwaitingCall, &call_empty_name).is_err());
    }
}
