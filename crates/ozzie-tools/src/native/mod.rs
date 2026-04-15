mod activate;
mod editor;
mod execute;
mod file;
mod git;
mod memory;
mod project;
mod schedule;
mod session;
mod skill_create;
mod sub_agent;
mod task;
mod tool_search;
mod web;
mod yield_control;

pub use activate::ActivateTool;
pub use editor::StrReplaceEditorTool;
pub use execute::ExecuteTool;
pub use file::{FileReadTool, FileWriteTool, GlobTool, GrepTool, ListDirTool};
pub use git::GitTool;
pub use memory::{ForgetMemoryTool, QueryMemoriesTool, StoreMemoryTool};
pub use project::{CloseProjectTool, InitProjectTool, ListProjectsTool, OpenProjectTool};
pub use schedule::{ListSchedulesTool, ScheduleTaskTool, TriggerScheduleTool, UnscheduleTaskTool};
pub use session::UpdateSessionTool;
pub use skill_create::CreateSkillTool;
pub use sub_agent::SubAgentTool;
pub use task::{RunSubtaskTool, DEFAULT_SUBTASK_TOOLS};
pub use tool_search::ToolSearchTool;
pub use web::{WebFetchTool, WebSearchTool};
pub use yield_control::YieldControlTool;

use crate::registry::{ToolRegistry, ToolSpec};

/// Registers all native core tools in the registry.
/// If `sandbox` is provided, the execute tool runs commands inside the OS sandbox.
pub fn register_all(
    registry: &ToolRegistry,
    sandbox: Option<std::sync::Arc<dyn ozzie_core::domain::CommandSandbox>>,
) {
    register(
        registry,
        Box::new(ExecuteTool { sandbox }),
        ExecuteTool::spec(),
    );
    register(registry, Box::new(FileReadTool), FileReadTool::spec());
    register(registry, Box::new(FileWriteTool), FileWriteTool::spec());
    register(registry, Box::new(ListDirTool), ListDirTool::spec());
    register(registry, Box::new(GlobTool), GlobTool::spec());
    register(registry, Box::new(GrepTool), GrepTool::spec());
    register(registry, Box::new(GitTool), GitTool::spec());
    register(
        registry,
        Box::new(StrReplaceEditorTool::new()),
        StrReplaceEditorTool::spec(),
    );
    register(registry, Box::new(WebFetchTool::new()), WebFetchTool::spec());
    register(registry, Box::new(WebSearchTool::new()), WebSearchTool::spec());
    register(registry, Box::new(YieldControlTool), YieldControlTool::spec());
}

/// Registers memory tools (store_memory, forget_memory, query_memories) with a shared store and optional pipeline.
pub fn register_memory_tools(
    registry: &ToolRegistry,
    store: std::sync::Arc<dyn ozzie_memory::Store>,
    pipeline: Option<std::sync::Arc<ozzie_memory::Pipeline>>,
) {
    register(
        registry,
        Box::new(StoreMemoryTool::new(store.clone(), pipeline.clone())),
        StoreMemoryTool::spec(),
    );
    register(
        registry,
        Box::new(ForgetMemoryTool::new(store.clone(), pipeline)),
        ForgetMemoryTool::spec(),
    );
    register(
        registry,
        Box::new(QueryMemoriesTool::new(store)),
        QueryMemoriesTool::spec(),
    );
}

/// Registers schedule tools (schedule_task, unschedule_task, list_schedules, trigger_schedule).
pub fn register_schedule_tools(
    registry: &ToolRegistry,
    scheduler: std::sync::Arc<dyn ozzie_core::domain::SchedulerPort>,
    bus: std::sync::Arc<dyn ozzie_core::events::EventBus>,
) {
    register(
        registry,
        Box::new(ScheduleTaskTool::new(scheduler.clone(), bus.clone())),
        ScheduleTaskTool::spec(),
    );
    register(
        registry,
        Box::new(UnscheduleTaskTool::new(scheduler.clone(), bus)),
        UnscheduleTaskTool::spec(),
    );
    register(
        registry,
        Box::new(ListSchedulesTool::new(scheduler.clone())),
        ListSchedulesTool::spec(),
    );
    register(
        registry,
        Box::new(TriggerScheduleTool::new(scheduler)),
        TriggerScheduleTool::spec(),
    );
}

/// Registers the tool_search tool for discovering available tools.
///
/// `core_names` lists tools always sent to the LLM — everything else is
/// searchable via this tool.
pub fn register_tool_search(
    registry: &ToolRegistry,
    tool_registry: std::sync::Arc<ToolRegistry>,
    core_names: Vec<String>,
) {
    register(
        registry,
        Box::new(ToolSearchTool::new(tool_registry, core_names)),
        ToolSearchTool::spec(),
    );
}

/// Registers the activate tool with shared ToolSet and registries.
pub fn register_activate_tool(
    registry: &ToolRegistry,
    tool_set: std::sync::Arc<ozzie_core::domain::ToolSet>,
    tool_registry: std::sync::Arc<ToolRegistry>,
    skill_registry: Option<std::sync::Arc<ozzie_core::skills::SkillRegistry>>,
) {
    register(
        registry,
        Box::new(ActivateTool::new(tool_set, tool_registry, skill_registry)),
        ActivateTool::spec(),
    );
}

/// Registers the run_subtask tool with a shared subtask runner.
pub fn register_subtask_tool(
    registry: &ToolRegistry,
    runner: std::sync::Arc<dyn ozzie_core::domain::SubtaskRunner>,
) {
    register(
        registry,
        Box::new(RunSubtaskTool::new(runner)),
        RunSubtaskTool::spec(),
    );
}

/// Registers the update_session tool with a shared session store.
pub fn register_session_tools(
    registry: &ToolRegistry,
    store: std::sync::Arc<dyn ozzie_core::domain::SessionStore>,
) {
    register(
        registry,
        Box::new(UpdateSessionTool::new(store)),
        UpdateSessionTool::spec(),
    );
}

/// Registers the create_skill tool.
pub fn register_create_skill_tool(
    registry: &ToolRegistry,
    skill_registry: std::sync::Arc<ozzie_core::skills::SkillRegistry>,
    project_registry: std::sync::Arc<ozzie_core::project::ProjectRegistry>,
    session_store: std::sync::Arc<dyn ozzie_core::domain::SessionStore>,
    skills_path: std::path::PathBuf,
) {
    register(
        registry,
        Box::new(CreateSkillTool::new(
            skill_registry,
            project_registry,
            session_store,
            skills_path,
        )),
        CreateSkillTool::spec(),
    );
}

/// Registers project tools (init_project, open_project, close_project, list_projects).
pub fn register_project_tools(
    registry: &ToolRegistry,
    project_registry: std::sync::Arc<ozzie_core::project::ProjectRegistry>,
    skill_registry: std::sync::Arc<ozzie_core::skills::SkillRegistry>,
    session_store: std::sync::Arc<dyn ozzie_core::domain::SessionStore>,
    workspaces_root: std::path::PathBuf,
) {
    register(
        registry,
        Box::new(InitProjectTool::new(project_registry.clone(), workspaces_root)),
        InitProjectTool::spec(),
    );
    register(
        registry,
        Box::new(OpenProjectTool::new(
            project_registry.clone(),
            skill_registry.clone(),
            session_store.clone(),
        )),
        OpenProjectTool::spec(),
    );
    register(
        registry,
        Box::new(CloseProjectTool::new(skill_registry, session_store)),
        CloseProjectTool::spec(),
    );
    register(
        registry,
        Box::new(ListProjectsTool::new(project_registry)),
        ListProjectsTool::spec(),
    );
}

/// Registers sub-agent tools from configuration.
pub fn register_sub_agent_tools(
    registry: &ToolRegistry,
    agents: &ozzie_core::config::SubAgentsConfig,
    runner: std::sync::Arc<dyn ozzie_core::domain::SubAgentRunner>,
) {
    for (name, config) in &agents.0 {
        let tool = SubAgentTool::new(name.clone(), config.clone(), runner.clone());
        let spec = tool.spec();
        register(registry, Box::new(tool), spec);
    }
}

fn register(registry: &ToolRegistry, tool: Box<dyn ozzie_core::domain::Tool>, spec: ToolSpec) {
    registry.register(tool, spec);
}
