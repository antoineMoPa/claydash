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

use std::collections::HashMap;
use lazy_static::lazy_static;
use std::sync::Mutex;

pub type CommandParamMap = HashMap<String, CommandParam>;

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
            parameters: HashMap::new(),
        };
    }
}

lazy_static! {
    static ref COMMANDS_MAP: Mutex<HashMap<String, CommandInfo>> = Mutex::new(HashMap::new());
}

pub fn add_command(system_name: &String, command: CommandInfo) {
    lazy_static::initialize(&COMMANDS_MAP);
    let mut commands = COMMANDS_MAP.lock().unwrap();

    if commands.contains_key(system_name) {
        panic!("Command {} already defined.", system_name);
    }

    commands.insert(system_name.clone(), command);
}

/// Returns a copy of the command
pub fn read_command(system_name: &String) -> Option<CommandInfo> {
    let commands = COMMANDS_MAP.lock().unwrap();
    return commands.get(system_name).cloned();
}

/// Returns copy of command  and decrements internal counter if the command has to be run.
///
/// Returns None if nothing has to be done.
pub fn check_if_has_to_run(system_name: &String) -> Option<CommandInfo> {
    let mut commands = COMMANDS_MAP.lock().unwrap();

    match commands.get_mut(system_name) {
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
pub fn run(system_name: &String) {
    let mut commands = COMMANDS_MAP.lock().unwrap();
    let command = commands.get_mut(system_name).unwrap();
    let mut params = command.parameters.clone();

    for (_, param) in params.iter_mut() {
        param.clear();
    }

    command.parameters = params;

    return command.run();
}


/// Requests to run a command by name again with last used parameters.
pub fn repeat(system_name: &String) {
    let mut commands = COMMANDS_MAP.lock().unwrap();
    let command = commands.get_mut(system_name).unwrap();
    return command.run();
}

/// Requests to run a command by name.
pub fn run_with_params(system_name: &String, parameters: &CommandParamMap) {
    let mut commands = COMMANDS_MAP.lock().unwrap();
    let command_option = commands.get_mut(system_name);

    match command_option {
        Some(command) => {
            command.parameters = parameters.clone();
            command.run();
        }
        _ => {
            panic!("Could not get command!");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_adds_and_gets_new_command() {
        add_command(&"test-command".to_string(), CommandInfo {
            title: "Test Command".to_string(),
            docs: "Here are some docs about the command".to_string(),
            ..CommandInfo::default()
        });
        let command = read_command(&"test-command".to_string()).unwrap();
        assert_eq!(command.title, "Test Command".to_string());
    }

    // This test is useful, but it causes other tests to panic.
    // Let's keep around for development
    #[ignore]
    #[test]
    #[should_panic]
    fn handles_not_found_commands() {
        read_command(&"not-existing-command".to_string()).unwrap();
    }

    // This test is useful, but it causes other tests to panic.
    // Let's keep around for development
    #[ignore]
    #[test]
    #[should_panic]
    fn it_detects_if_command_already_exists() {
        add_command(&"test-command-duplicated".to_string(), CommandInfo {
            title: "Test Command".to_string(),
            docs: "Here are some docs about the command".to_string(),
            ..CommandInfo::default()
        });
        add_command(&"test-command-duplicated".to_string(), CommandInfo {
            title: "Test Command".to_string(),
            docs: "Here are some docs about the command".to_string(),
            ..CommandInfo::default()
        });
    }

    #[test]
    fn runs_commands() {
        let sys_name = "test-command-with-callback".to_string();
        add_command(&sys_name, CommandInfo {
            title: "Test Command".to_string(),
            docs: "Here are some docs about the command".to_string(),
            ..CommandInfo::default()
        });
        read_command(&sys_name).unwrap();

        run(&sys_name);

        // Should return true 1 time
        assert_eq!(check_if_has_to_run(&sys_name).is_some(), true);
        assert_eq!(check_if_has_to_run(&sys_name).is_some(), false);

        run(&sys_name);
        run(&sys_name);

        // Should return true 2 times
        assert_eq!(check_if_has_to_run(&sys_name).is_some(), true);
        assert_eq!(check_if_has_to_run(&sys_name).is_some(), true);
        assert_eq!(check_if_has_to_run(&sys_name).is_some(), false);
    }

    #[test]
    fn does_not_run_non_existant_command() {
        let sys_name = "not-existing-command".to_string();

        assert_eq!(check_if_has_to_run(&sys_name).is_none(), true);
    }

    #[test]
    fn creates_and_runs_command_with_parameters() {
        let sys_name = "test-command-with-params".to_string();

        let mut params: CommandParamMap= HashMap::new();

        params.insert("x".to_string(), CommandParam {
            docs: "X position of the mouse.".to_string(),
            ..CommandParam::default()
        });

        params.insert("y".to_string(), CommandParam {
            docs: "Y position of the mouse.".to_string(),
            ..CommandParam::default()
        });

        add_command(&sys_name, CommandInfo {
            title: "Test Command".to_string(),
            docs: "Here are some docs about the command".to_string(),
            parameters: params,
            ..CommandInfo::default()
        });

        assert_eq!(
            read_command(&sys_name).unwrap().parameters["x"].docs,
            "X position of the mouse.".to_string()
        );

        // Simulate application part where we would trigger the command
        {
            let mut params: CommandParamMap= HashMap::new();

            params.insert("x".to_string(), CommandParam {
                docs: "X position of the mouse.".to_string(),
                float: Some(998.3),
                ..CommandParam::default()
            });

            run_with_params(&sys_name, &params);
        }

        let mut side_effect_result: f32 = 0.0;

        // simulate application loop where we would process the command:
        {
            let command = check_if_has_to_run(&sys_name).unwrap();

            let original_x = command.parameters.get(&"x".to_string()).unwrap().float.unwrap();

            side_effect_result = original_x * 2.0;
        }

        assert_eq!(side_effect_result, 998.3 * 2.0);
    }

    #[test]
    fn repeats_last_command_with_parameters() {
        let sys_name = "test-command-with-params-2".to_string();

        let mut params: CommandParamMap= HashMap::new();

        params.insert("x".to_string(), CommandParam {
            docs: "X position of the mouse.".to_string(),
            ..CommandParam::default()
        });

        add_command(&sys_name, CommandInfo {
            title: "Test Command".to_string(),
            docs: "Here are some docs about the command".to_string(),
            parameters: params,
            ..CommandInfo::default()
        });

        // Simulate application part where we would trigger the command
        {
            let mut params: CommandParamMap= HashMap::new();

            params.insert("x".to_string(), CommandParam {
                docs: "X position of the mouse.".to_string(),
                float: Some(12.3),
                ..CommandParam::default()
            });

            run_with_params(&sys_name, &params);
        }

        // simulate application loop where we would process the command:
        {
            let command = check_if_has_to_run(&sys_name).unwrap();
            let float_val = command.parameters.get(&"x".to_string()).unwrap().float.unwrap();
            assert_eq!(float_val, 12.3);
        }

        // Simulate application part where we would trigger a repeat of last command.
        {
            repeat(&sys_name);
        }

        // simulate application loop where we would process the command again:
        {
            let command = check_if_has_to_run(&sys_name).unwrap();
            let float_val = command.parameters.get(&"x".to_string()).unwrap().float.unwrap();
            assert_eq!(float_val, 12.3);
        }
    }
}
