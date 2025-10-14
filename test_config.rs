use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct ConfigToml {
    pub profile: Option<String>,
    pub orchestrator_profile: Option<String>,
    #[serde(default)]
    pub agent_profiles: Option<Vec<String>>,
    #[serde(default)]
    pub profiles: HashMap<String, serde_json::Value>,
}

fn main() {
    let config_path = "/Users/sero/.codex-local/config.toml";

    println!("Reading config from: {}", config_path);

    let config_content = std::fs::read_to_string(config_path)
        .expect("Failed to read config file");

    let config: ConfigToml = toml::from_str(&config_content)
        .expect("Failed to parse config");

    println!("Parsed configuration:");
    println!("  profile: {:?}", config.profile);
    println!("  orchestrator_profile: {:?}", config.orchestrator_profile);
    println!("  agent_profiles: {:?}", config.agent_profiles);

    // Check if required orchestrator profiles exist
    if let Some(orchestrator) = &config.orchestrator_profile {
        println!("  Orchestrator profile requested: {}", orchestrator);
        if !config.profiles.contains_key(orchestrator) {
            println!("  ❌ ERROR: orchestrator_profile '{}' not found in profiles!", orchestrator);
        } else {
            println!("  ✅ Orchestrator profile '{}' found", orchestrator);
        }
    } else {
        println!("  ℹ️  No orchestrator_profile configured");
    }

    if let Some(agents) = &config.agent_profiles {
        println!("  Agent profiles: {:?}", agents);
        for agent in agents {
            if !config.profiles.contains_key(agent) {
                println!("  ❌ ERROR: agent profile '{}' not found in profiles!", agent);
            } else {
                println!("  ✅ Agent profile '{}' found", agent);
            }
        }
    } else {
        println!("  ℹ️  No agent_profiles configured");
    }
}
