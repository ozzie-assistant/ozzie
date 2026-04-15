mod catalog;
mod composer;
mod persona;
mod sections;

pub use catalog::{AGENT_INSTRUCTIONS, AGENT_INSTRUCTIONS_COMPACT, DEFAULT_PERSONA, DEFAULT_PERSONA_COMPACT, SUB_AGENT_INSTRUCTIONS, SUB_AGENT_INSTRUCTIONS_COMPACT};
pub use composer::{Composer, Section};
pub use persona::{agent_instructions_for_tier, load_persona, persona_for_tier, sub_agent_instructions_for_tier};
pub use sections::{actor_section, memory_section, project_section, session_section, skill_section, tool_section, truncate_utf8, user_profile_section, MemoryInfo};
