//! Local TypeSafe System One decision-model support.
//!
//! The public types intentionally normalize the small protocol differences so
//! callers can apply one policy to choices, scores, and binary (`noul`)
//! answers. Network details remain in [`SystemOneHttpModel`].

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use loop_ai::UserMessageContent;
use parking_lot::Mutex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::types::{AgentContext, AgentMessage, AgentTool};
use crate::Message;

/// A set of related decision questions sent to a decision model.
#[derive(Debug, Clone, Serialize)]
pub struct DecisionRequest {
    /// Compact, task-relevant context. It must not contain secrets.
    pub state: Value,
    /// Questions keyed by stable caller-controlled ids.
    pub questions: BTreeMap<String, DecisionQuestion>,
}

/// A System One question.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum DecisionQuestion {
    /// Select exactly one named option.
    Choice {
        /// What selection is needed.
        instructions: String,
        /// Option name to optional explanatory text. `None` serializes as JSON
        /// `null`, as required by System One.
        criteria: BTreeMap<String, Option<String>>,
    },
    /// Pick one of two through ten ordered levels.
    Score {
        /// What the levels mean.
        instructions: String,
        /// Ordered level descriptions, from low to high.
        criteria: Vec<Option<String>>,
    },
    /// A yes/no question whose answer is a probability of yes.
    Noul {
        /// The binary proposition.
        instructions: String,
    },
}

/// A normalized decision answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum DecisionAnswer {
    /// A named choice and probabilities for all returned choices.
    Choice {
        /// Highest-probability option returned by the server.
        value: String,
        /// Per-option probabilities.
        probabilities: BTreeMap<String, f64>,
        /// Server confidence.
        confidence: f64,
    },
    /// A selected score and probabilities for all returned levels.
    Score {
        /// Highest-probability level returned by the server.
        value: u8,
        /// Per-level probabilities.
        probabilities: BTreeMap<u8, f64>,
        /// Server confidence.
        confidence: f64,
    },
    /// Probability that the binary proposition is true.
    Noul {
        /// Probability of yes.
        probability_yes: f64,
        /// Normalized certainty, computed as `|2p - 1|` because the protocol
        /// does not return a separate Noul confidence value.
        confidence: f64,
    },
}

impl DecisionAnswer {
    /// A single normalized confidence value suitable for policy decisions.
    pub fn confidence(&self) -> f64 {
        match self {
            Self::Choice { confidence, .. }
            | Self::Score { confidence, .. }
            | Self::Noul { confidence, .. } => *confidence,
        }
    }
}

/// A complete decision-model response.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Decision {
    /// Answers keyed by the request question id.
    pub answers: BTreeMap<String, DecisionAnswer>,
    /// Model identifier returned or configured for the request.
    pub model: Option<String>,
    /// End-to-end request latency in milliseconds.
    pub latency_ms: u64,
}

/// Failure returned by a decision model. The caller treats all failures as a
/// safe fallback, never as a reason to block the main agent turn.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum DecisionError {
    /// The local service could not be contacted in time.
    #[error("decision service unavailable: {message}")]
    Unavailable {
        /// Underlying transport or timeout description.
        message: String,
    },
    /// The service returned a non-retryable HTTP error.
    #[error("decision service rejected request ({status}): {message}")]
    Rejected {
        /// HTTP status code returned by the server.
        status: u16,
        /// Short response body or failure description.
        message: String,
    },
    /// The service returned data that did not conform to the System One
    /// response contract.
    #[error("invalid decision response: {message}")]
    InvalidResponse {
        /// Contract violation description.
        message: String,
    },
    /// An offline scripted model has no answer left.
    #[error("faux decision model has no scripted response left")]
    Exhausted,
}

/// Async decision-model interface used by the turn loop and tests.
#[async_trait]
pub trait DecisionModel: Send + Sync {
    /// Resolve a set of decision questions.
    async fn decide(&self, req: DecisionRequest) -> Result<Decision, DecisionError>;
}

/// HTTP client for a local TypeSafe System One server such as Lev.
pub struct SystemOneHttpModel {
    base_url: String,
    api_key: String,
    model: Option<String>,
    timeout: Duration,
    first_call_timeout: Duration,
    client: Client,
    first_call: AtomicBool,
}

impl SystemOneHttpModel {
    /// Create a client. The first request is allowed at least 120 seconds so
    /// a local Lev server can compile kernels and load its checkpoint.
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: Option<String>,
        timeout: Duration,
    ) -> Result<Self, DecisionError> {
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
            return Err(DecisionError::InvalidResponse {
                message: "base_url must start with http:// or https://".into(),
            });
        }
        let client = Client::builder()
            .build()
            .map_err(|err| DecisionError::Unavailable {
                message: err.to_string(),
            })?;
        Ok(Self {
            base_url,
            api_key: api_key.into(),
            model,
            timeout,
            first_call_timeout: timeout.max(Duration::from_secs(120)),
            client,
            first_call: AtomicBool::new(true),
        })
    }

    /// Override the first-call budget. This is primarily useful for tests.
    pub fn with_first_call_timeout(mut self, timeout: Duration) -> Self {
        self.first_call_timeout = timeout;
        self
    }

    fn endpoint(&self) -> String {
        format!("{}/v1/systemone", self.base_url)
    }

    async fn send_once(
        &self,
        req: &DecisionRequest,
        timeout: Duration,
    ) -> Result<reqwest::Response, DecisionError> {
        self.client
            .post(self.endpoint())
            .bearer_auth(&self.api_key)
            .timeout(timeout)
            .json(req)
            .send()
            .await
            .map_err(|err| DecisionError::Unavailable {
                message: err.to_string(),
            })
    }
}

#[async_trait]
impl DecisionModel for SystemOneHttpModel {
    async fn decide(&self, req: DecisionRequest) -> Result<Decision, DecisionError> {
        validate_request(&req)?;
        let started = Instant::now();
        let first = self
            .first_call
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        let call_timeout = if first {
            self.first_call_timeout
        } else {
            self.timeout
        };

        // One retry only for unavailable/retryable server failures. Validation
        // and 4xx responses remain immediate failures.
        let response = match self.send_once(&req, call_timeout).await {
            Ok(response) if !response.status().is_server_error() => response,
            Ok(response) => {
                tracing::debug!(status = %response.status(), "retrying decision request after server error");
                self.send_once(&req, self.timeout).await?
            }
            Err(first_error) => {
                tracing::debug!(error = %first_error, "retrying unavailable decision request");
                self.send_once(&req, self.timeout).await?
            }
        };

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|err| DecisionError::Unavailable {
                message: err.to_string(),
            })?;
        if !status.is_success() {
            return Err(DecisionError::Rejected {
                status: status.as_u16(),
                message: truncate_error_body(&body),
            });
        }
        parse_response(&req, &body, self.model.clone(), started.elapsed())
    }
}

fn truncate_error_body(body: &str) -> String {
    const MAX: usize = 300;
    let body = body.trim();
    if body.len() <= MAX {
        body.to_owned()
    } else {
        format!("{}…", &body[..MAX])
    }
}

/// A scripted decision model for deterministic, offline tests.
#[derive(Default)]
pub struct FauxDecisionModel {
    answers: Mutex<VecDeque<Result<Decision, DecisionError>>>,
}

impl FauxDecisionModel {
    /// Create with responses returned in FIFO order.
    pub fn new(script: impl IntoIterator<Item = Result<Decision, DecisionError>>) -> Self {
        Self {
            answers: Mutex::new(script.into_iter().collect()),
        }
    }
}

#[async_trait]
impl DecisionModel for FauxDecisionModel {
    async fn decide(&self, _req: DecisionRequest) -> Result<Decision, DecisionError> {
        self.answers
            .lock()
            .pop_front()
            .unwrap_or(Err(DecisionError::Exhausted))
    }
}

/// Policy result for a proposed decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DecisionAction {
    /// The decision may be applied.
    Act,
    /// The decision is plausible but a destructive effect needs confirmation.
    Confirm,
    /// Preserve Loop's ordinary behavior.
    Fallback,
}

/// Confidence thresholds for applying a decision.
#[derive(Debug, Clone, Copy)]
pub struct DecisionPolicy {
    /// Minimum confidence for non-destructive decisions.
    pub minimum_confidence: f64,
    /// Minimum confidence for a destructive decision to execute without
    /// confirmation.
    pub destructive_confidence: f64,
    /// Minimum probability of missing critical context before Loop asks for
    /// clarification instead of beginning work. Phase 6 must calibrate this.
    pub context_missing_probability: f64,
}

impl Default for DecisionPolicy {
    fn default() -> Self {
        Self {
            minimum_confidence: 0.5,
            destructive_confidence: 0.75,
            context_missing_probability: 0.75,
        }
    }
}

impl DecisionPolicy {
    /// Map a normalized confidence to a safe action.
    pub fn action(&self, confidence: Option<f64>, destructive: bool) -> DecisionAction {
        let Some(confidence) = confidence.filter(|value| value.is_finite()) else {
            return DecisionAction::Fallback;
        };
        if confidence < self.minimum_confidence {
            DecisionAction::Fallback
        } else if destructive && confidence < self.destructive_confidence {
            DecisionAction::Confirm
        } else {
            DecisionAction::Act
        }
    }

    /// Whether the Noul triage result is strong enough to ask for context.
    pub fn should_clarify(&self, triage: &TaskTriage) -> bool {
        triage.critical_context_missing_probability >= self.context_missing_probability
            && triage.critical_context_missing_confidence >= self.minimum_confidence
    }
}

/// Coarse task class returned by the advisory triage workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskType {
    /// The user primarily needs an explanation or direct answer.
    Answer,
    /// The user needs existing code, files, or data examined.
    Inspect,
    /// The user requests a code or artifact change.
    Modify,
    /// The user requests a command, build, test, or other execution.
    Execute,
    /// The task needs material clarification before it can proceed safely.
    Clarify,
}

impl TaskType {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "answer" => Some(Self::Answer),
            "inspect" => Some(Self::Inspect),
            "modify" => Some(Self::Modify),
            "execute" => Some(Self::Execute),
            "clarify" => Some(Self::Clarify),
            _ => None,
        }
    }
}

/// Independent task-triage answers from one System One request.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskTriage {
    /// Coarse task category.
    pub task_type: TaskType,
    /// Server confidence for the task-type choice.
    pub task_type_confidence: f64,
    /// Probability that material context is missing.
    pub critical_context_missing_probability: f64,
    /// Normalized Noul confidence, `|2p - 1|`.
    pub critical_context_missing_confidence: f64,
}

/// Advisory output for the two Phase 4 workflows.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnDecisionAdvice {
    /// A high-confidence tool promotion, or `None` when the main model should
    /// receive Loop's original full tool menu.
    pub selected_tool: Option<String>,
    /// Task triage.
    pub triage: TaskTriage,
    /// A matching skill or prompt selected by task type, when configured.
    pub route: Option<TaskRoute>,
    /// Raw, validated System One decision for visibility and evaluation.
    pub decision: Decision,
}

/// Serializable record emitted after Loop applies a decision.
#[derive(Debug, Clone, Serialize)]
pub struct DecisionEvent {
    /// Unix timestamp in milliseconds when Loop applied the decision.
    pub timestamp_ms: i64,
    /// Validated answers returned by Lev.
    pub answers: BTreeMap<String, DecisionAnswer>,
    /// Tool selected by the tool Choice, including `none` when applicable.
    pub tool: Option<String>,
    /// Confidence attached to the selected tool Choice.
    pub tool_confidence: Option<f64>,
    /// What Loop actually did with this decision.
    pub action: String,
    /// Local decision request duration.
    pub latency_ms: u64,
    /// Lev model identifier when reported.
    pub model: Option<String>,
}

impl DecisionEvent {
    /// One-line UI representation without raw task state.
    pub fn display_line(&self) -> String {
        let tool = self.tool.as_deref().unwrap_or("none");
        match self.tool_confidence {
            Some(confidence) => format!(
                "◆ decision: tool={tool} conf={confidence:.2} ({}ms) → {}",
                self.latency_ms, self.action
            ),
            None => format!(
                "◆ decision: tool={tool} ({}ms) → {}",
                self.latency_ms, self.action
            ),
        }
    }
}

/// Per-turn skills and prompts eligible for task-type routing.
#[derive(Debug, Clone, Default)]
pub struct DecisionWorkflowResources {
    skills: BTreeMap<String, String>,
    prompts: BTreeMap<String, String>,
}

impl DecisionWorkflowResources {
    /// Build routing candidates from `(name, body)` pairs. A task type matches
    /// a resource only on an exact name match.
    pub fn new(
        skills: impl IntoIterator<Item = (String, String)>,
        prompts: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        Self {
            skills: skills.into_iter().collect(),
            prompts: prompts.into_iter().collect(),
        }
    }

    fn route_for(&self, task_type: TaskType) -> Option<TaskRoute> {
        let name = match task_type {
            TaskType::Answer => "answer",
            TaskType::Inspect => "inspect",
            TaskType::Modify => "modify",
            TaskType::Execute => "execute",
            TaskType::Clarify => "clarify",
        };
        self.skills
            .get(name)
            .map(|body| TaskRoute::Skill {
                name: name.into(),
                body: body.clone(),
            })
            .or_else(|| {
                self.prompts.get(name).map(|body| TaskRoute::Prompt {
                    name: name.into(),
                    body: body.clone(),
                })
            })
    }
}

/// Resource selected by the task-type workflow.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskRoute {
    /// A skill with a name exactly matching the task type.
    Skill {
        /// Skill name.
        name: String,
        /// Skill markdown body.
        body: String,
    },
    /// A prompt template with a name exactly matching the task type.
    Prompt {
        /// Prompt name.
        name: String,
        /// Prompt markdown body, without user-supplied template arguments.
        body: String,
    },
}

impl TaskRoute {
    /// A system-prompt addition applying the selected resource.
    pub fn system_prompt_addition(&self) -> String {
        match self {
            Self::Skill { name, body } => {
                format!(
                    "<decision-selected-skill name=\"{name}\">\n{body}\n</decision-selected-skill>"
                )
            }
            Self::Prompt { name, body } => {
                format!("<decision-selected-prompt name=\"{name}\">\n{body}\n</decision-selected-prompt>")
            }
        }
    }
}

/// Decision-model hook used by the hand-written turn loop.
#[derive(Clone)]
pub struct DecisionToolSelector {
    model: Arc<dyn DecisionModel>,
    policy: DecisionPolicy,
}

impl DecisionToolSelector {
    /// Create a selector with the default conservative policy.
    pub fn new(model: Arc<dyn DecisionModel>) -> Self {
        Self {
            model,
            policy: DecisionPolicy::default(),
        }
    }

    /// Replace the default policy.
    pub fn with_policy(mut self, policy: DecisionPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Whether a completed advisory result should ask for clarification before
    /// exposing tools or invoking the main model.
    pub fn requires_clarification(&self, advice: &TurnDecisionAdvice) -> bool {
        self.policy.should_clarify(&advice.triage)
    }

    /// Convert a validated decision and Loop's applied action into a durable
    /// visibility record. It intentionally excludes raw request state.
    pub fn event(&self, advice: &TurnDecisionAdvice, action: impl Into<String>) -> DecisionEvent {
        let (tool, tool_confidence) = match advice.decision.answers.get("tool") {
            Some(DecisionAnswer::Choice {
                value, confidence, ..
            }) => (Some(value.clone()), Some(*confidence)),
            _ => (None, None),
        };
        DecisionEvent {
            timestamp_ms: loop_ai::now_ms(),
            answers: advice.decision.answers.clone(),
            tool,
            tool_confidence,
            action: action.into(),
            latency_ms: advice.decision.latency_ms,
            model: advice.decision.model.clone(),
        }
    }

    /// Run tool selection and task triage in one System One request.
    ///
    /// The two questions are intentionally independent. This method exposes
    /// triage to hosts while keeping it advisory until Phase 6 calibration.
    pub async fn assess_turn(
        &self,
        context: &AgentContext,
        resources: &DecisionWorkflowResources,
    ) -> Result<TurnDecisionAdvice, DecisionError> {
        let tools = context
            .tools
            .as_ref()
            .ok_or_else(|| DecisionError::InvalidResponse {
                message: "task assessment requires a tool registry".into(),
            })?;
        if tools.is_empty() {
            return Err(DecisionError::InvalidResponse {
                message: "task assessment requires at least one tool".into(),
            });
        }
        let decision = self
            .model
            .decide(turn_workflow_request(context, tools))
            .await?;
        let triage = task_triage_from_decision(&decision)?;
        let selected_tool = selected_tool_from_decision(&decision, tools, self.policy)?;
        Ok(TurnDecisionAdvice {
            selected_tool,
            route: resources.route_for(triage.task_type),
            triage,
            decision,
        })
    }

    /// Select one tool for the current turn, or return `None` to retain the
    /// original full tool set. The chosen set is advisory: Loop's main model
    /// still decides whether to issue a tool call.
    pub async fn select_tools(
        &self,
        context: &AgentContext,
        resources: &DecisionWorkflowResources,
    ) -> Option<Vec<AgentTool>> {
        let advice = match self.assess_turn(context, resources).await {
            Ok(advice) => advice,
            Err(error) => {
                tracing::debug!(error = %error, "decision workflow unavailable; using full tool set");
                return None;
            }
        };
        tracing::debug!(
            task_type = ?advice.triage.task_type,
            task_type_confidence = advice.triage.task_type_confidence,
            critical_context_missing_probability = advice.triage.critical_context_missing_probability,
            critical_context_missing_confidence = advice.triage.critical_context_missing_confidence,
            "advisory task triage completed"
        );
        let selected = advice.selected_tool?;
        context
            .tools
            .as_ref()?
            .iter()
            .find(|tool| tool.name == selected)
            .cloned()
            .map(|tool| vec![tool])
    }
}

fn is_destructive_tool(name: &str) -> bool {
    matches!(name, "write" | "edit" | "bash")
}

fn turn_workflow_request(context: &AgentContext, tools: &[AgentTool]) -> DecisionRequest {
    let mut criteria = BTreeMap::new();
    criteria.insert(
        "none".into(),
        Some("No tool is needed; answer directly.".into()),
    );
    for tool in tools {
        criteria.insert(tool.name.clone(), Some(tool.description.clone()));
    }
    let latest_user_message = context
        .messages
        .iter()
        .rev()
        .find_map(|message| match message {
            AgentMessage::Llm(Message::User(user)) => match &user.content {
                UserMessageContent::Text(text) => Some(text.clone()),
                UserMessageContent::Blocks(_) => Some("[multimodal user message]".into()),
            },
            _ => None,
        });
    DecisionRequest {
        state: serde_json::json!({
            "latest_user_message": latest_user_message,
            "available_tool_names": tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>(),
        }),
        questions: BTreeMap::from([
            (
                "tool".into(),
                DecisionQuestion::Choice {
                    instructions: "Which tool should run next?".into(),
                    criteria,
                },
            ),
            (
                "task_type".into(),
                DecisionQuestion::Choice {
                    instructions: "What kind of task is this?".into(),
                    criteria: task_type_criteria(),
                },
            ),
            (
                "critical_context_missing".into(),
                DecisionQuestion::Noul {
                    instructions: "Is critical context missing such that execution should wait for clarification?".into(),
                },
            ),
        ]),
    }
}

fn task_type_criteria() -> BTreeMap<String, Option<String>> {
    BTreeMap::from([
        ("answer".into(), Some("answer or explain directly".into())),
        (
            "inspect".into(),
            Some("inspect existing code, files, or data".into()),
        ),
        (
            "modify".into(),
            Some("change code, files, or an artifact".into()),
        ),
        (
            "execute".into(),
            Some("run a command, build, test, or external action".into()),
        ),
        (
            "clarify".into(),
            Some("obtain material task constraints before proceeding".into()),
        ),
    ])
}

fn task_triage_from_decision(decision: &Decision) -> Result<TaskTriage, DecisionError> {
    let Some(DecisionAnswer::Choice {
        value, confidence, ..
    }) = decision.answers.get("task_type")
    else {
        return Err(DecisionError::InvalidResponse {
            message: "decision response did not return a task_type choice".into(),
        });
    };
    let task_type = TaskType::parse(value).ok_or_else(|| DecisionError::InvalidResponse {
        message: format!("decision response returned unknown task_type `{value}`"),
    })?;
    let Some(DecisionAnswer::Noul {
        probability_yes,
        confidence: missing_confidence,
    }) = decision.answers.get("critical_context_missing")
    else {
        return Err(DecisionError::InvalidResponse {
            message: "decision response did not return a critical_context_missing Noul answer"
                .into(),
        });
    };
    Ok(TaskTriage {
        task_type,
        task_type_confidence: *confidence,
        critical_context_missing_probability: *probability_yes,
        critical_context_missing_confidence: *missing_confidence,
    })
}

fn selected_tool_from_decision(
    decision: &Decision,
    tools: &[AgentTool],
    policy: DecisionPolicy,
) -> Result<Option<String>, DecisionError> {
    let Some(DecisionAnswer::Choice {
        value, confidence, ..
    }) = decision.answers.get("tool")
    else {
        return Err(DecisionError::InvalidResponse {
            message: "decision response did not return a tool choice".into(),
        });
    };
    if value == "none" {
        return Ok(None);
    }
    let tool = tools
        .iter()
        .find(|tool| tool.name == *value)
        .ok_or_else(|| DecisionError::InvalidResponse {
            message: format!("decision response selected unknown tool `{value}`"),
        })?;
    match policy.action(Some(*confidence), is_destructive_tool(&tool.name)) {
        DecisionAction::Act => Ok(Some(tool.name.clone())),
        DecisionAction::Confirm | DecisionAction::Fallback => Ok(None),
    }
}

fn validate_request(request: &DecisionRequest) -> Result<(), DecisionError> {
    if request.questions.is_empty() {
        return Err(DecisionError::InvalidResponse {
            message: "a decision request must include at least one question".into(),
        });
    }
    for (id, question) in &request.questions {
        if id.trim().is_empty() {
            return Err(DecisionError::InvalidResponse {
                message: "question ids must not be empty".into(),
            });
        }
        match question {
            DecisionQuestion::Choice { criteria, .. } if criteria.is_empty() => {
                return Err(DecisionError::InvalidResponse {
                    message: format!("choice question `{id}` must contain at least one option"),
                });
            }
            DecisionQuestion::Score { criteria, .. } if !(2..=10).contains(&criteria.len()) => {
                return Err(DecisionError::InvalidResponse {
                    message: format!("score question `{id}` must have 2 to 10 ordered levels"),
                });
            }
            _ => {}
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct WireResponse {
    #[serde(default)]
    answers: BTreeMap<String, WireAnswer>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum WireAnswer {
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
    Score {
        score: u8,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
    Noul {
        noul: f64,
    },
}

fn parse_response(
    request: &DecisionRequest,
    body: &str,
    configured_model: Option<String>,
    latency: Duration,
) -> Result<Decision, DecisionError> {
    let response: WireResponse =
        serde_json::from_str(body).map_err(|error| DecisionError::InvalidResponse {
            message: error.to_string(),
        })?;
    let mut answers = BTreeMap::new();
    for (id, question) in &request.questions {
        let answer = response
            .answers
            .get(id)
            .ok_or_else(|| DecisionError::InvalidResponse {
                message: format!("response omitted answer for question `{id}`"),
            })?;
        let normalized = match (question, answer) {
            (
                DecisionQuestion::Choice { criteria, .. },
                WireAnswer::Choice {
                    choice,
                    probabilities,
                    confidence,
                },
            ) => {
                if !criteria.contains_key(choice) {
                    return Err(DecisionError::InvalidResponse {
                        message: format!(
                            "choice answer `{choice}` is not an offered option for `{id}`"
                        ),
                    });
                }
                validate_probability_map(probabilities, id)?;
                validate_confidence(*confidence, id)?;
                DecisionAnswer::Choice {
                    value: choice.clone(),
                    probabilities: probabilities.clone(),
                    confidence: *confidence,
                }
            }
            (
                DecisionQuestion::Score { criteria, .. },
                WireAnswer::Score {
                    score,
                    probabilities,
                    confidence,
                },
            ) => {
                if *score == 0 || usize::from(*score) > criteria.len() {
                    return Err(DecisionError::InvalidResponse {
                        message: format!(
                            "score `{score}` is outside the offered levels for `{id}`"
                        ),
                    });
                }
                let probabilities = probabilities
                    .iter()
                    .map(|(level, probability)| {
                        let level =
                            level
                                .parse::<u8>()
                                .map_err(|_| DecisionError::InvalidResponse {
                                    message: format!(
                                    "score probability key `{level}` for `{id}` is not an integer"
                                ),
                                })?;
                        Ok((level, *probability))
                    })
                    .collect::<Result<BTreeMap<_, _>, DecisionError>>()?;
                if probabilities
                    .keys()
                    .any(|level| *level == 0 || usize::from(*level) > criteria.len())
                {
                    return Err(DecisionError::InvalidResponse {
                        message: format!("score probabilities contain a level outside `{id}`"),
                    });
                }
                validate_probability_map(&probabilities, id)?;
                validate_confidence(*confidence, id)?;
                DecisionAnswer::Score {
                    value: *score,
                    probabilities,
                    confidence: *confidence,
                }
            }
            (DecisionQuestion::Noul { .. }, WireAnswer::Noul { noul }) => {
                validate_probability(*noul, id)?;
                DecisionAnswer::Noul {
                    probability_yes: *noul,
                    confidence: (2.0 * *noul - 1.0).abs(),
                }
            }
            _ => {
                return Err(DecisionError::InvalidResponse {
                    message: format!("response type does not match question `{id}`"),
                });
            }
        };
        answers.insert(id.clone(), normalized);
    }
    Ok(Decision {
        answers,
        model: response.model.or(configured_model),
        latency_ms: latency.as_millis().try_into().unwrap_or(u64::MAX),
    })
}

fn validate_probability_map<K: Ord>(
    probabilities: &BTreeMap<K, f64>,
    question_id: &str,
) -> Result<(), DecisionError> {
    if probabilities.is_empty() {
        return Err(DecisionError::InvalidResponse {
            message: format!("probabilities for `{question_id}` must not be empty"),
        });
    }
    let mut total = 0.0;
    for probability in probabilities.values() {
        validate_probability(*probability, question_id)?;
        total += probability;
    }
    if (total - 1.0).abs() > 0.01 {
        return Err(DecisionError::InvalidResponse {
            message: format!("probabilities for `{question_id}` sum to {total:.3}, not 1"),
        });
    }
    Ok(())
}

fn validate_probability(probability: f64, question_id: &str) -> Result<(), DecisionError> {
    if !probability.is_finite() || !(0.0..=1.0).contains(&probability) {
        return Err(DecisionError::InvalidResponse {
            message: format!("probability for `{question_id}` must be finite and between 0 and 1"),
        });
    }
    Ok(())
}

fn validate_confidence(confidence: f64, question_id: &str) -> Result<(), DecisionError> {
    if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
        return Err(DecisionError::InvalidResponse {
            message: format!("confidence for `{question_id}` must be finite and between 0 and 1"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn choice_request() -> DecisionRequest {
        DecisionRequest {
            state: serde_json::json!({"task": "read a file"}),
            questions: BTreeMap::from([(
                "tool".into(),
                DecisionQuestion::Choice {
                    instructions: "Which tool?".into(),
                    criteria: BTreeMap::from([
                        ("read_file".into(), Some("read a file".into())),
                        ("none".into(), None),
                    ]),
                },
            )]),
        }
    }

    #[test]
    fn parses_all_system_one_answer_shapes() {
        let request = DecisionRequest {
            state: Value::Null,
            questions: BTreeMap::from([
                (
                    "choice".into(),
                    choice_request().questions.remove("tool").unwrap(),
                ),
                (
                    "score".into(),
                    DecisionQuestion::Score {
                        instructions: "How urgent?".into(),
                        criteria: vec![Some("low".into()), Some("high".into())],
                    },
                ),
                (
                    "safe".into(),
                    DecisionQuestion::Noul {
                        instructions: "Is this safe?".into(),
                    },
                ),
            ]),
        };
        let parsed = parse_response(
            &request,
            r#"{"model":"lev","answers":{"choice":{"type":"choice","choice":"read_file","probabilities":{"read_file":0.9,"none":0.1},"confidence":0.9},"score":{"type":"score","score":2,"probabilities":{"1":0.1,"2":0.9},"confidence":0.9},"safe":{"type":"noul","noul":0.8}}}"#,
            None,
            Duration::from_millis(7),
        )
        .unwrap();
        assert_eq!(parsed.model.as_deref(), Some("lev"));
        assert_eq!(parsed.latency_ms, 7);
        assert!((parsed.answers["safe"].confidence() - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn rejects_invalid_score_levels() {
        let request = DecisionRequest {
            state: Value::Null,
            questions: BTreeMap::from([(
                "score".into(),
                DecisionQuestion::Score {
                    instructions: "Rate".into(),
                    criteria: vec![None],
                },
            )]),
        };
        assert!(matches!(
            validate_request(&request),
            Err(DecisionError::InvalidResponse { .. })
        ));
    }

    #[test]
    fn policy_is_conservative() {
        let policy = DecisionPolicy::default();
        assert_eq!(policy.action(Some(0.49), false), DecisionAction::Fallback);
        assert_eq!(policy.action(Some(0.50), false), DecisionAction::Act);
        assert_eq!(policy.action(Some(0.50), true), DecisionAction::Confirm);
        assert_eq!(policy.action(Some(0.75), true), DecisionAction::Act);
        assert_eq!(policy.action(None, false), DecisionAction::Fallback);
    }

    #[test]
    fn context_missing_requires_a_conservative_probability() {
        let policy = DecisionPolicy::default();
        let advice = |probability, confidence| TurnDecisionAdvice {
            selected_tool: None,
            triage: TaskTriage {
                task_type: TaskType::Clarify,
                task_type_confidence: 0.9,
                critical_context_missing_probability: probability,
                critical_context_missing_confidence: confidence,
            },
            route: None,
            decision: Decision {
                answers: BTreeMap::new(),
                model: None,
                latency_ms: 0,
            },
        };
        assert!(!policy.should_clarify(&advice(0.74, 0.48).triage));
        assert!(policy.should_clarify(&advice(0.75, 0.5).triage));
    }

    #[test]
    fn task_type_prefers_an_exact_skill_then_a_prompt() {
        let resources = DecisionWorkflowResources::new(
            [("inspect".into(), "inspect skill".into())],
            [
                ("inspect".into(), "inspect prompt".into()),
                ("modify".into(), "modify prompt".into()),
            ],
        );
        assert!(matches!(
            resources.route_for(TaskType::Inspect),
            Some(TaskRoute::Skill { body, .. }) if body == "inspect skill"
        ));
        assert!(matches!(
            resources.route_for(TaskType::Modify),
            Some(TaskRoute::Prompt { body, .. }) if body == "modify prompt"
        ));
    }

    #[test]
    fn decision_event_formats_the_tui_line() {
        let event = DecisionEvent {
            timestamp_ms: 1,
            answers: BTreeMap::new(),
            tool: Some("read_file".into()),
            tool_confidence: Some(0.93),
            action: "narrowed_toolset".into(),
            latency_ms: 212,
            model: Some("lev".into()),
        };
        assert_eq!(
            event.display_line(),
            "◆ decision: tool=read_file conf=0.93 (212ms) → narrowed_toolset"
        );
    }

    #[test]
    fn turn_workflow_batches_tool_selection_and_independent_triage() {
        let request = turn_workflow_request(&AgentContext::default(), &[]);
        assert!(matches!(
            request.questions.get("tool"),
            Some(DecisionQuestion::Choice { criteria, .. }) if criteria.contains_key("none")
        ));
        assert!(matches!(
            request.questions.get("task_type"),
            Some(DecisionQuestion::Choice { criteria, .. })
                if criteria.keys().map(String::as_str).eq(["answer", "clarify", "execute", "inspect", "modify"])
        ));
        assert!(matches!(
            request.questions.get("critical_context_missing"),
            Some(DecisionQuestion::Noul { .. })
        ));
    }

    #[test]
    fn triage_normalizes_choice_and_noul_independently() {
        let decision = Decision {
            answers: BTreeMap::from([
                (
                    "task_type".into(),
                    DecisionAnswer::Choice {
                        value: "inspect".into(),
                        probabilities: BTreeMap::from([("inspect".into(), 0.8)]),
                        confidence: 0.8,
                    },
                ),
                (
                    "critical_context_missing".into(),
                    DecisionAnswer::Noul {
                        probability_yes: 0.25,
                        confidence: 0.5,
                    },
                ),
            ]),
            model: Some("faux".into()),
            latency_ms: 0,
        };
        assert_eq!(
            task_triage_from_decision(&decision).unwrap(),
            TaskTriage {
                task_type: TaskType::Inspect,
                task_type_confidence: 0.8,
                critical_context_missing_probability: 0.25,
                critical_context_missing_confidence: 0.5,
            }
        );
    }

    #[tokio::test]
    async fn faux_model_is_fifo_and_offline() {
        let decision = Decision {
            answers: BTreeMap::new(),
            model: Some("faux".into()),
            latency_ms: 0,
        };
        let model = FauxDecisionModel::new([Ok(decision.clone())]);
        assert_eq!(model.decide(choice_request()).await.unwrap(), decision);
        assert_eq!(
            model.decide(choice_request()).await.unwrap_err(),
            DecisionError::Exhausted
        );
    }

    #[tokio::test]
    async fn http_model_retries_one_server_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for (status, body) in [
                ("503 Service Unavailable", "temporarily loading"),
                (
                    "200 OK",
                    r#"{"model":"lev","answers":{"tool":{"type":"choice","choice":"read_file","probabilities":{"read_file":0.9,"none":0.1},"confidence":0.9}}}"#,
                ),
            ] {
                let (mut socket, _) = listener.accept().unwrap();
                let mut request = [0u8; 4096];
                let _ = socket.read(&mut request).unwrap();
                write!(
                    socket,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        let model = SystemOneHttpModel::new(
            format!("http://{address}"),
            "local",
            None,
            Duration::from_secs(1),
        )
        .unwrap()
        .with_first_call_timeout(Duration::from_secs(1));
        let decision = model.decide(choice_request()).await.unwrap();
        assert!(matches!(
            decision.answers.get("tool"),
            Some(DecisionAnswer::Choice { value, .. }) if value == "read_file"
        ));
        server.join().unwrap();
    }

    #[tokio::test]
    #[ignore = "requires a running local Lev server"]
    async fn live_lev_systemone_smoke_test() {
        let base_url = std::env::var("LOOP_LEV_TEST_BASE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8000".into());
        let api_key = std::env::var("LOOP_LEV_TEST_API_KEY").unwrap_or_else(|_| "local".into());
        let timeout_s = std::env::var("LOOP_LEV_TEST_TIMEOUT_S")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(120);
        let request = DecisionRequest {
            state: serde_json::json!({
                "latest_user_message": "Inspect crates/loop-agent/src/decision.rs without modifying files.",
                "available_tool_names": ["none", "read", "edit", "bash"],
            }),
            questions: BTreeMap::from([
                (
                    "tool".into(),
                    DecisionQuestion::Choice {
                        instructions: "Which tool should run next?".into(),
                        criteria: BTreeMap::from([
                            ("none".into(), Some("No tool is needed.".into())),
                            ("read".into(), Some("Read a file.".into())),
                            ("edit".into(), Some("Modify a file.".into())),
                            ("bash".into(), Some("Run a command.".into())),
                        ]),
                    },
                ),
                (
                    "task_type".into(),
                    DecisionQuestion::Choice {
                        instructions: "Classify the task.".into(),
                        criteria: BTreeMap::from([
                            ("answer".into(), Some("Answer directly.".into())),
                            ("clarify".into(), Some("Ask for context.".into())),
                            ("execute".into(), Some("Run a command.".into())),
                            ("inspect".into(), Some("Investigate without changes.".into())),
                            ("modify".into(), Some("Change files.".into())),
                        ]),
                    },
                ),
                (
                    "critical_context_missing".into(),
                    DecisionQuestion::Noul {
                        instructions: "Is critical context missing before acting?".into(),
                    },
                ),
            ]),
        };
        let model = SystemOneHttpModel::new(
            base_url,
            api_key,
            None,
            Duration::from_secs(timeout_s),
        )
        .unwrap()
        .with_first_call_timeout(Duration::from_secs(timeout_s));
        let decision = model.decide(request).await.unwrap();
        assert!(matches!(
            decision.answers.get("tool"),
            Some(DecisionAnswer::Choice { value, .. }) if ["none", "read", "edit", "bash"].contains(&value.as_str())
        ));
        assert!(matches!(
            decision.answers.get("task_type"),
            Some(DecisionAnswer::Choice { value, .. }) if ["answer", "clarify", "execute", "inspect", "modify"].contains(&value.as_str())
        ));
        assert!(matches!(
            decision.answers.get("critical_context_missing"),
            Some(DecisionAnswer::Noul { probability_yes, .. }) if (0.0..=1.0).contains(probability_yes)
        ));
    }
}
