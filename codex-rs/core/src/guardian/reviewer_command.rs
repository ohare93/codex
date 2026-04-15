use std::process::Stdio;

use anyhow::Context;
use codex_protocol::protocol::ReviewDecision;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::codex::TurnContext;

use super::GUARDIAN_REVIEW_TIMEOUT;
use super::GuardianApprovalRequest;
use super::approval_request::guardian_approval_request_to_json;
use super::approval_request::guardian_request_turn_id;

#[derive(Debug)]
pub(super) struct CommandReviewResponse {
    pub(super) decision: ReviewDecision,
    pub(super) rationale: Option<String>,
}

#[derive(Serialize)]
struct CommandReviewRequest<'a> {
    version: u32,
    review_id: &'a str,
    turn_id: &'a str,
    cwd: &'a str,
    retry_reason: Option<&'a str>,
    request: Value,
}

#[derive(Deserialize)]
struct CommandReviewResponseWire {
    decision: CommandReviewDecision,
    rationale: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum CommandReviewDecision {
    Approved,
    ApprovedForSession,
    Denied,
    Abort,
}

pub(super) async fn review_with_command(
    turn: &TurnContext,
    review_id: &str,
    request: &GuardianApprovalRequest,
    retry_reason: Option<&str>,
) -> anyhow::Result<CommandReviewResponse> {
    let command = turn
        .config
        .approvals_reviewer_command
        .as_ref()
        .context("missing approvals_reviewer_command")?;
    let Some((program, args)) = command.split_first() else {
        anyhow::bail!("empty approvals_reviewer_command");
    };

    let cwd = turn.cwd.as_path().to_string_lossy().into_owned();
    let payload = serde_json::to_vec(&CommandReviewRequest {
        version: 1,
        review_id,
        turn_id: guardian_request_turn_id(request, &turn.sub_id),
        cwd: &cwd,
        retry_reason,
        request: guardian_approval_request_to_json(request)?,
    })?;

    let mut child = Command::new(program);
    child
        .args(args)
        .current_dir(turn.cwd.as_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = child
        .spawn()
        .with_context(|| format!("failed to spawn approvals reviewer command `{program}`"))?;
    let Some(mut stdin) = child.stdin.take() else {
        anyhow::bail!("failed to open stdin for approvals reviewer command");
    };

    let output = tokio::time::timeout(GUARDIAN_REVIEW_TIMEOUT, async move {
        stdin.write_all(&payload).await?;
        stdin.shutdown().await?;
        drop(stdin);
        child.wait_with_output().await
    })
    .await
    .context("approvals reviewer command timed out")??;

    if !output.status.success() {
        anyhow::bail!(
            "approvals reviewer command exited with status {}",
            output
                .status
                .code()
                .map_or_else(|| "signal".to_string(), |code| code.to_string())
        );
    }

    let response: CommandReviewResponseWire = serde_json::from_slice(&output.stdout)
        .context("approvals reviewer command returned malformed JSON")?;
    let decision = match response.decision {
        CommandReviewDecision::Approved => ReviewDecision::Approved,
        CommandReviewDecision::ApprovedForSession => ReviewDecision::ApprovedForSession,
        CommandReviewDecision::Denied => ReviewDecision::Denied,
        CommandReviewDecision::Abort => ReviewDecision::Abort,
    };

    Ok(CommandReviewResponse {
        decision,
        rationale: response.rationale,
    })
}
