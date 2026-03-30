use crate::MainWindow;
use crate::workflow::bindings::{show_error, show_success};
use crate::workflow::state::SharedWorkflowState;
use slint::ComponentHandle;

#[derive(Debug, Clone)]
pub(crate) struct PrAnalysisRequest {
    pub(crate) number: String,
    pub(crate) title: String,
    pub(crate) summary: String,
    pub(crate) ci_green: bool,
    pub(crate) reviewers_ready: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct PrAnalysis {
    pub(crate) decision: String,
    pub(crate) summary: String,
}

pub(crate) fn analyze_pr(req: PrAnalysisRequest) -> Result<PrAnalysis, String> {
    let number = req.number.trim();
    let title = req.title.trim();
    if number.is_empty() {
        return Err("PR number is required.".to_string());
    }
    if title.is_empty() {
        return Err("PR title is required.".to_string());
    }

    let haystack = format!("{} {}", title, req.summary).to_lowercase();
    let high_risk_hits = [
        "auth",
        "permission",
        "payment",
        "billing",
        "migration",
        "delete",
        "rollback",
        "security",
    ]
    .into_iter()
    .filter(|needle| haystack.contains(needle))
    .count();

    let decision = if !req.ci_green {
        "Block"
    } else if high_risk_hits >= 2 || !req.reviewers_ready {
        "Needs human review"
    } else {
        "Recommend approve"
    };

    let summary = format!(
        "PR #{number}: CI={}, reviewers_ready={}, high_risk_signals={high_risk_hits}. AI suggests '{decision}' based on delivery risk and review readiness. Human confirmation is still required.",
        if req.ci_green { "green" } else { "not green" },
        if req.reviewers_ready { "yes" } else { "no" },
    );

    Ok(PrAnalysis {
        decision: decision.to_string(),
        summary,
    })
}

pub(crate) fn confirm_pr(decision: bool, ai_decision: &str) -> String {
    if decision {
        format!("Approved by human. AI recommendation was '{ai_decision}'.")
    } else {
        format!("Rejected by human. AI recommendation was '{ai_decision}'.")
    }
}

pub(crate) fn bind_pr(ui: &MainWindow, shared_state: SharedWorkflowState) {
    let ui_weak = ui.as_weak();
    let state_analyze = shared_state.clone();
    ui.on_workflow_pr_analyze(move |number, title, summary, ci_green, reviewers_ready| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        match analyze_pr(PrAnalysisRequest {
            number: number.to_string(),
            title: title.to_string(),
            summary: summary.to_string(),
            ci_green,
            reviewers_ready,
        }) {
            Ok(result) => {
                if let Ok(mut guard) = state_analyze.lock() {
                    guard.upsert_current(|view| {
                        view.last_pr_decision = result.decision.clone();
                        view.pr_ai_decision = result.decision.clone();
                        view.pr_ai_summary = result.summary.clone();
                        view.pr_manual_status = "Waiting for human confirmation.".to_string();
                    });
                }
                ui.set_workflow_pr_ai_decision(result.decision.into());
                ui.set_workflow_pr_ai_summary(result.summary.into());
                ui.set_workflow_pr_manual_status("Waiting for human confirmation.".into());
                show_success(&ui, "PR analysis completed.");
            }
            Err(error) => {
                if let Ok(mut guard) = state_analyze.lock() {
                    guard.upsert_current(|view| {
                        view.pr_ai_decision = "Analysis failed".to_string();
                        view.pr_ai_summary = error.clone();
                    });
                }
                ui.set_workflow_pr_ai_decision("Analysis failed".into());
                ui.set_workflow_pr_ai_summary(error.clone().into());
                show_error(&ui, &error);
            }
        }
    });

    let ui_weak = ui.as_weak();
    let state_confirm = shared_state.clone();
    ui.on_workflow_pr_confirm(move |approved| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let ai_decision = state_confirm
            .lock()
            .ok()
            .map(|guard| guard.with_current(|_, view| view.last_pr_decision.clone()))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "No AI recommendation yet".to_string());
        let status = confirm_pr(approved, &ai_decision);
        if let Ok(mut guard) = state_confirm.lock() {
            guard.upsert_current(|view| view.pr_manual_status = status.clone());
        }
        ui.set_workflow_pr_manual_status(status.clone().into());
        show_success(&ui, &status);
    });
}
