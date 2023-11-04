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

// We want a orderered version of HashMap. Turns our BTreeMap is ordered!
// So, using BTreeMap avoids order constantly flickering, example: when searching.
use std::collections::BTreeMap;

pub type CommandInfoMap = BTreeMap<String, CommandInfo>;
pub type CommandParamMap = BTreeMap<String, CommandParam>;

#[derive(Clone)]
pub struct CommandMap {
    pub commands: CommandInfoMap,
}

impl CommandMap {
    pub fn new() -> Self {
        Self {
            commands: CommandInfoMap::new()
        }
    }

    pub fn add_command(&mut self, system_name: &String, command: CommandInfo) {
        if self.commands.contains_key(system_name) {
            panic!("Command {} already defined.", system_name);
        }

        self.commands.insert(system_name.clone(), command);
    }

    /// Returns a copy of the command
    pub fn read_command(&mut self, system_name: &String) -> Option<CommandInfo> {
        return self.commands.get(system_name).cloned();
    }

    /// Returns copy of command  and decrements internal counter if the command has to be run.
    ///
    /// Returns None if nothing has to be done.
    pub fn check_if_has_to_run(&mut self, system_name: &String) -> Option<CommandInfo> {
        match &mut self.commands.get_mut(system_name) {
            Some(command) => {
                if command.check_if_has_to_run() {
                    return Some(command.clone());
                } else {
                    return None;
                }
            },
            _ => { return None; }
        }
    }

    /// Requests to run a command by name.
    pub fn run(&mut self, system_name: &String) {
        let command = self.commands.get_mut(system_name).unwrap();
        let mut params = command.parameters.clone();

        for (_, param) in params.iter_mut() {
            param.clear();
        }

        command.parameters = params;

        return command.run();
    }


    /// Requests to run a command by name again with last used parameters.
    pub fn repeat(&mut self, system_name: &String) {
        self.commands.get_mut(system_name).unwrap().run();
    }

    /// Requests to run a command by name.
    pub fn run_with_params(&mut self, system_name: &String, parameters: &CommandParamMap) {
        let command_option = self.commands.get_mut(system_name);

        match command_option {
            Some(command) => {
                for parameter in parameters.iter() {
                    command.parameters.insert(parameter.0.to_string(), parameter.1.clone());
                }
                command.run();
            }
            _ => {
                panic!("Could not get command!");
            }
        }
    }

    /// Search through commands
    pub fn search(&mut self, search: &String, limit: usize) -> CommandInfoMap {
        let search_lower = search.to_lowercase();
        let mut results: CommandInfoMap = CommandInfoMap::new();
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
pub struct CommandParam {
    pub docs: String,
    pub float: Option<f32>,
}

impl Default for CommandParam {
    fn default() -> Self {
        return Self {
            docs: "".to_string(),
            float: None,
        };
    }
}

impl CommandParam {
    fn clear(&mut self) {
        self.float = None;
    }
}

pub struct CommandBuilder {
    pub command_param_map: CommandParamMap,
    pub system_name: String,
    pub title: String,
    pub docs: String,
}

impl CommandBuilder {
    pub fn new() -> Self {
        return Self {
            system_name: "".to_string(),
            title: "".to_string(),
            docs: "".to_string(),
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

    pub fn docs(&mut self, docs: &str) -> &mut Self {
        self.docs = docs.into();
        return self;
    }

    pub fn insert_param(&mut self,  system_name: &str, docs: &str) -> &mut Self {
        self.command_param_map.insert(system_name.to_string(), CommandParam {
            docs: docs.to_string(),
            ..CommandParam::default()
        });
        return self;
    }

    pub fn write(&mut self, commands: &mut CommandMap) {
        commands.add_command(&self.system_name, CommandInfo {
            title: self.title.to_string(),
            docs: self.docs.to_string(),
            parameters: self.command_param_map.clone(),
            ..CommandInfo::default()
        });
    }
}

#[derive(Clone)]
pub struct CommandInfo {
    pub title: String,
    pub docs: String,
    pub keybinding: String,
    pub requested_runs: i32,
    pub parameters: CommandParamMap,
}

impl CommandInfo {
    fn run(&mut self) {
        self.requested_runs = self.requested_runs + 1;
    }

    fn check_if_has_to_run(&mut self) -> bool {
        if self.requested_runs > 0 {
            self.requested_runs = self.requested_runs - 1;
            return true;
        }
        return false;
    }
}

impl Default for CommandInfo {
    fn default() -> Self {
        return Self {
            title: "".to_string(),
            docs: "".to_string(),
            keybinding: "".to_string(),
            requested_runs: 0,
            parameters: BTreeMap::new(),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_adds_and_gets_new_command() {
        let mut commands = CommandMap::new();
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
        let mut commands = CommandMap::new();
        commands.read_command(&"not-existing-command".to_string()).unwrap();
    }

    // This test is useful, but it causes other tests to panic.
    // Let's keep around for development
    #[ignore]
    #[test]
    #[should_panic]
    fn it_detects_if_command_already_exists() {
        let mut commands = CommandMap::new();
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
    fn runs_commands() {
        let sys_name = "test-command-with-callback".to_string();
        let mut commands = CommandMap::new();
        commands.add_command(&sys_name, CommandInfo {
            title: "Test Command".to_string(),
            docs: "Here are some docs about the command".to_string(),
            ..CommandInfo::default()
        });
        commands.read_command(&sys_name).unwrap();

        commands.run(&sys_name);

        // Should return true 1 time
        assert_eq!(commands.check_if_has_to_run(&sys_name).is_some(), true);
        assert_eq!(commands.check_if_has_to_run(&sys_name).is_some(), false);

        commands.run(&sys_name);
        commands.run(&sys_name);

        // Should return true 2 times
        assert_eq!(commands.check_if_has_to_run(&sys_name).is_some(), true);
        assert_eq!(commands.check_if_has_to_run(&sys_name).is_some(), true);
        assert_eq!(commands.check_if_has_to_run(&sys_name).is_some(), false);
    }

    #[test]
    fn does_not_run_non_existant_command() {
        let sys_name = "not-existing-command".to_string();
        let mut commands = CommandMap::new();
        assert_eq!(commands.check_if_has_to_run(&sys_name).is_none(), true);
    }

    #[test]
    fn creates_and_runs_command_with_parameters() {
        let mut commands = CommandMap::new();
        let sys_name = "test-command-with-params".to_string();

        CommandBuilder::new()
            .title("Test Command")
            .system_name("test-command-with-params")
            .docs("Here are some docs about the command")
            .insert_param("x", "X position of the mouse.")
            .insert_param("y", "Y position of the mouse.")
            .insert_param("z", "Z position of the mouse.")
            .write(&mut commands);

        assert_eq!(
            commands.read_command(&sys_name).unwrap().parameters["x"].docs,
            "X position of the mouse.".to_string()
        );

        // Simulate application part where we would trigger the command
        {
            let mut params: CommandParamMap= BTreeMap::new();

            params.insert("x".to_string(), CommandParam {
                docs: "X position of the mouse.".to_string(),
                float: Some(998.3),
                ..CommandParam::default()
            });

            commands.run_with_params(&sys_name, &params);
        }

        #[allow(unused_assignments)]
        let mut side_effect_result: f32 = 0.0;

        // simulate application loop where we would process the command:
        {
            let command = commands.check_if_has_to_run(&sys_name).unwrap();

            let original_x = command.parameters.get(&"x".to_string()).unwrap().float.unwrap();

            side_effect_result = original_x * 2.0;
        }

        assert_eq!(side_effect_result, 998.3 * 2.0);
    }

    #[test]
    fn repeats_last_command_with_parameters() {
        let mut commands = CommandMap::new();
        let sys_name = "test-command-with-params-2".to_string();

        let mut params: CommandParamMap = BTreeMap::new();

        params.insert("x".to_string(), CommandParam {
            docs: "X position of the mouse.".to_string(),
            ..CommandParam::default()
        });

        commands.add_command(&sys_name, CommandInfo {
            title: "Test Command".to_string(),
            docs: "Here are some docs about the command".to_string(),
            parameters: params,
            ..CommandInfo::default()
        });

        // Simulate application part where we would trigger the command
        {
            let mut params: CommandParamMap= BTreeMap::new();

            params.insert("x".to_string(), CommandParam {
                docs: "X position of the mouse.".to_string(),
                float: Some(12.3),
                ..CommandParam::default()
            });

            commands.run_with_params(&sys_name, &params);
        }

        // simulate application loop where we would process the command:
        {
            let command = commands.check_if_has_to_run(&sys_name).unwrap();
            let float_val = command.parameters.get(&"x".to_string()).unwrap().float.unwrap();
            assert_eq!(float_val, 12.3);
        }

        // Simulate application part where we would trigger a repeat of last command.
        {
            commands.repeat(&sys_name);
        }

        // simulate application loop where we would process the command again:
        {
            let command = commands.check_if_has_to_run(&sys_name).unwrap();
            let float_val = command.parameters.get(&"x".to_string()).unwrap().float.unwrap();
            assert_eq!(float_val, 12.3);
        }
    }

    #[test]
    fn searches_commands_by_system_name() {
        let sys_name = "command-to-search-1".to_string();
        let mut commands = CommandMap::new();

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
        let mut commands = CommandMap::new();

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
        let mut commands = CommandMap::new();

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
    fn searches_have_limited() {
        let sys_name = "command-to-search-4-A".to_string();
        let mut commands = CommandMap::new();

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
