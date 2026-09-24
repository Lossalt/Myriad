// Agent Chat streaming, frontend actions and task management paths.

mod chat_stream;
mod frontend_actions;
mod tasks;

pub(crate) use frontend_actions::collect_step_frontend_actions;

#[cfg(test)]
mod split_contract_tests {
    #[test]
    fn tasks_do_not_filter_wear_stream() {
        assert!(!include_str!("tasks.rs").contains("WearStreamFilter"));
    }
}
