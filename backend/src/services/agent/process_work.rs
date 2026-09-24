// Work entry points. Saved Recipes use their separate executor path.
use super::agent_header::*;
use super::motion_overlay::attach_motion_to_result;
use super::types::*;
use serde_json::json;

impl Agent {
    pub(super) async fn process_work(
        &self,
        request: UserRequest,
        mood_transition: Option<crate::services::agent::merope::MoodTransition>,
        mood_before: Option<f64>,
        round_motion_style: String,
    ) -> Result<AgentResponse, String> {
        if let Some(response) = self
            .mood_refuse_response(
                request.user_id,
                super::merope::refuse_new_task_message(mood_before),
                None,
            )
            .await
        {
            return Ok(response);
        }
        super::merope::note_chat_diary(&self.db, request.user_id, &request.raw_input).await;
        let result = self.start_work_loop(request.clone(), None).await;
        attach_motion_to_result(result, &request, mood_transition, &round_motion_style).await
    }

    pub(super) async fn process_work_with_progress(
        &self,
        request: UserRequest,
        progress_tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
        mood_transition: Option<crate::services::agent::merope::MoodTransition>,
        mood_before: Option<f64>,
        round_motion_style: String,
    ) -> Result<AgentResponse, String> {
        if let Some(response) = self
            .mood_refuse_response(
                request.user_id,
                super::merope::refuse_new_task_message(mood_before),
                Some(&progress_tx),
            )
            .await
        {
            return Ok(response);
        }
        super::merope::note_chat_diary(&self.db, request.user_id, &request.raw_input).await;
        let result = self
            .start_work_loop(request.clone(), Some(progress_tx))
            .await;
        attach_motion_to_result(result, &request, mood_transition, &round_motion_style).await
    }

    pub(super) async fn mood_refuse_response(
        &self,
        user_id: i32,
        message: Option<String>,
        progress_tx: Option<&tokio::sync::mpsc::Sender<AgentProgressEvent>>,
    ) -> Option<AgentResponse> {
        let message = message?;
        if let Some(tx) = progress_tx {
            Self::stream_text_as_tokens(tx, &message).await;
        }
        crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
        Some(AgentResponse {
            response_type: AgentResponseType::Answer,
            message,
            data: Some(json!({ "type": "mood_refuse" })),
            data_display: None,
            suggestions: vec![],
            task: None,
            frontend_action: None,
            performance: None,
        })
    }
}
