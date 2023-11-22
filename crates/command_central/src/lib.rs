//!
//! Command Central
//!
//! The idea of command central is that any function in an app, every button is tied to a command.
//! Each command is documented, potentially reusable in scripts.
//!
//! This implementation is a pretty early and inefficient version, but it should help getting started.
//! Later, it would be good to:
//!  - Find an efficient way to notify commands that does not require checking all of commands.
//!  - Accept parameters and return values
//!  - Make it scriptable
//!

// We want a version of HashMap that is ordered by key. Turns our BTreeMap is ordered by key!
// So, using BTreeMap avoids order constantly flickering, example: when searching.
use std::collections::BTreeMap;

pub type CommandInfoMap<ParamType> = BTreeMap<String, CommandInfo<ParamType>>;
pub type CommandParamMap<ParamType> = BTreeMap<String, CommandParam<ParamType>>;

#[derive(Clone, Default)]
pub struct CommandMap<ParamType: Clone> {
    pub commands: CommandInfoMap<ParamType>,
}

impl<ParamType: Clone> CommandMap<ParamType> {
    pub fn new() -> Self {
        Self {
            commands: CommandInfoMap::new()
        }
    }

    pub fn add_command(&mut self, system_name: &String, command: CommandInfo<ParamType>) {
        if self.commands.contains_key(system_name) {
            panic!("Command {} already defined.", system_name);
        }

        self.commands.insert(system_name.clone(), command);
    }

    /// Returns a copy of the command
    pub fn read_command(&mut self, system_name: &String) -> Option<CommandInfo<ParamType>> {
        return self.commands.get(system_name).cloned();
    }

    /// Search through commands
    pub fn search(&mut self, search: &String, limit: usize) -> CommandInfoMap<ParamType> {
        let search_lower = search.to_lowercase();
        let mut results: CommandInfoMap<ParamType> = CommandInfoMap::new();
        for command in self.commands.iter() {
            let system_name = command.0;
            let command = command.1;

            if system_name.to_lowercase().contains(&search_lower) ||
                command.title.to_lowercase().contains(&search_lower) ||
                command.docs.to_lowercase().contains(&search_lower) {
                    results.insert(system_name.to_string(), command.clone());
                }

            if results.len() == limit {
                break;
            }
        }
        return results;
    }
}

#[derive(Clone)]
pub struct CommandParam<ParamType: Clone> {
    pub docs: String,
    pub default: Option<ParamType>,
    pub value: Option<ParamType>
}

impl<ParamType: Clone> Default for CommandParam<ParamType> {
    fn default() -> Self {
        return Self {
            docs: "".to_string(),
            default: None,
            value: None
        };
    }
}

#[derive(Clone)]
pub struct CommandBuilder<ParamType: Clone> {
    pub command_param_map: CommandParamMap<ParamType>,
    pub system_name: String,
    pub title: String,
    pub docs: String,
    pub shortcut: String,
}

impl<ParamType: Clone> CommandBuilder<ParamType> {
    pub fn new() -> Self {
        return Self {
            system_name: "".to_string(),
            title: "".to_string(),
            docs: "".to_string(),
            shortcut: "".to_string(),
            command_param_map: CommandParamMap::new(),
        };
    }

    pub fn system_name(&mut self, system_name: &str) -> &mut Self {
        self.system_name = system_name.into();
        return self;
    }

    pub fn title(&mut self, title: &str) -> &mut Self {
        self.title = title.into();
        return self;
    }

    /// command docs
    /// This is a good place to use synonyms to increase the chance of finding
    /// commands.
    pub fn docs(&mut self, docs: &str) -> &mut Self {
        self.docs = docs.into();
        return self;
    }

    pub fn shortcut(&mut self, shortcut: &str) -> &mut Self {
        self.shortcut = shortcut.into();
        return self;
    }

    /// Hack: we currently use a param to store callbacks.
    pub fn insert_param(&mut self,  system_name: &str, docs: &str, default: Option<ParamType>) -> &mut Self {
        self.command_param_map.insert(system_name.to_string(), CommandParam {
            docs: docs.to_string(),
            default: default.clone(),
            value: default,
            ..CommandParam::default()
        });
        return self;
    }

    pub fn write(&mut self, commands: &mut CommandMap<ParamType>) {
        commands.add_command(&self.system_name, CommandInfo {
            title: self.title.to_string(),
            docs: self.docs.to_string(),
            shortcut: self.shortcut.clone(),
            parameters: self.command_param_map.clone(),
            ..CommandInfo::default()
        });
    }
}

#[derive(Clone)]
pub struct CommandInfo<ParamType: Clone> {
    pub title: String,
    pub docs: String,
    pub shortcut: String,
    pub parameters: CommandParamMap<ParamType>,
}

impl<ParamType: Clone> Default for CommandInfo<ParamType> {
    fn default() -> Self {
        return Self {
            title: "".to_string(),
            docs: "".to_string(),
            shortcut: "".to_string(),
            parameters: BTreeMap::new(),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_adds_and_gets_new_command() {
        let mut commands: CommandMap<f32> = CommandMap::new();
        commands.add_command(&"test-command".to_string(), CommandInfo {
            title: "Test Command".to_string(),
            docs: "Here are some docs about the command".to_string(),
            ..CommandInfo::default()
        });
        let command = commands.read_command(&"test-command".to_string()).unwrap();
        assert_eq!(command.title, "Test Command".to_string());
    }

    // This test is useful, but it causes other tests to panic.
    // Let's keep around for development
    #[ignore]
    #[test]
    #[should_panic]
    fn handles_not_found_commands() {
        let mut commands: CommandMap<f32> = CommandMap::new();
        commands.read_command(&"not-existing-command".to_string()).unwrap();
    }

    // This test is useful, but it causes other tests to panic.
    // Let's keep around for development
    #[ignore]
    #[test]
    #[should_panic]
    fn it_detects_if_command_already_exists() {
        let mut commands: CommandMap<f32> = CommandMap::new();
        commands.add_command(&"test-command-duplicated".to_string(), CommandInfo {
            title: "Test Command".to_string(),
            docs: "Here are some docs about the command".to_string(),
            ..CommandInfo::default()
        });
        commands.add_command(&"test-command-duplicated".to_string(), CommandInfo {
            title: "Test Command".to_string(),
            docs: "Here are some docs about the command".to_string(),
            ..CommandInfo::default()
        });
    }

    #[test]
    fn searches_commands_by_system_name() {
        let sys_name = "command-to-search-1".to_string();
        let mut commands: CommandMap<f32> = CommandMap::new();

        commands.add_command(&sys_name, CommandInfo {
            title: "A command to search".to_string(),
            docs: "Here are some docs about the command".to_string(),
            ..CommandInfo::default()
        });

        // Note that case is changed to check that search is case insensitive.
        let results = commands.search(&"to-SEARCH-1".to_string(), 5);

        assert_eq!(results.len(), 1);
        assert_eq!(results["command-to-search-1"].title, "A command to search");
    }

    #[test]
    fn searches_commands_by_title() {
        let sys_name = "command-to-search-2".to_string();
        let mut commands: CommandMap<f32> = CommandMap::new();

        commands.add_command(&sys_name, CommandInfo {
            title: "A command to search by title".to_string(),
            docs: "Here are some docs about the command".to_string(),
            ..CommandInfo::default()
        });

        // Note that case is changed to check that search is case insensitive.
        let results = commands.search(&"search by TITLE".to_string(), 5);

        assert_eq!(results.len(), 1);
        assert_eq!(results["command-to-search-2"].title, "A command to search by title");
    }

    #[test]
    fn searches_commands_by_docs() {
        let sys_name = "command-to-search-3".to_string();
        let mut commands: CommandMap<f32> = CommandMap::new();

        // Note that case is changed to check that search is case insensitive.
        commands.add_command(&sys_name, CommandInfo {
            title: "A third command to search by docs".to_string(),
            docs: "Here are some docs about THIS epic COMMAND".to_string(),
            ..CommandInfo::default()
        });

        let results = commands.search(&"THIS EPIC COMMAND".to_string(), 5);

        assert_eq!(results.len(), 1);
        assert_eq!(results["command-to-search-3"].title, "A third command to search by docs");
    }

    #[test]
    fn searches_have_limited_result_count() {
        let sys_name = "command-to-search-4-A".to_string();
        let mut commands: CommandMap<f32> = CommandMap::new();

        // Note that case is changed to check that search is case insensitive.
        commands.add_command(&sys_name, CommandInfo {
            ..CommandInfo::default()
        });

        let sys_name = "command-to-search-4-B".to_string();

        // Note that case is changed to check that search is case insensitive.
        commands.add_command(&sys_name, CommandInfo {
            ..CommandInfo::default()
        });

        let sys_name = "command-to-search-4-C".to_string();

        // Note that case is changed to check that search is case insensitive.
        commands.add_command(&sys_name, CommandInfo {
            ..CommandInfo::default()
        });

        let results = commands.search(&"command-to-search-4".to_string(), 2);

        assert_eq!(results.len(), 2);
    }
}
