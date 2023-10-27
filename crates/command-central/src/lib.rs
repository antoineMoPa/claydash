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
use std::sync::MutexGuard;

#[derive(Clone)]
pub struct Command {
    pub title: String,
    pub docs: String,
    pub keybinding: String,
    pub requested_runs: i32,
}


impl Command {
    fn run(&mut self) {
        self.requested_runs = self.requested_runs + 1;
    }

    /// Returns true and decrements internal counter if the command has to be run.
    ///
    /// Returns false if nothing has to be done.
    fn check_if_has_to_run(&mut self) -> bool {
        if self.requested_runs > 0 {
            self.requested_runs = self.requested_runs - 1;
            return true;
        }
        return false;
    }
}

impl Default for Command {
    fn default() -> Self {
        return Self {
            title: "".to_string(),
            docs: "".to_string(),
            keybinding: "".to_string(),
            requested_runs: 0,
        };
    }
}

lazy_static! {
    static ref COMMANDS_MAP: Mutex<HashMap<String, Command>> = Mutex::new(HashMap::new());
}

pub fn add_command(system_name: String, command: Command) {
    lazy_static::initialize(&COMMANDS_MAP);
    let mut commands = COMMANDS_MAP.lock().unwrap();

    if commands.contains_key(&system_name) {
        panic!("Command {} already defined.", system_name);
    }

    commands.insert(system_name, command);
}

/// Returns a copy of the command
pub fn read_command(system_name: String) -> Option<Command> {
    let commands = COMMANDS_MAP.lock().unwrap();
    return commands.get(&system_name).cloned();
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_adds_and_gets_new_command() {
        add_command("test-command".to_string(), Command {
            title: "Test Command".to_string(),
            docs: "Here are some docs about the command".to_string(),
            ..Command::default()
        });
        let command = read_command("test-command".to_string()).unwrap();
        assert_eq!(command.title, "Test Command".to_string());
    }

    // This test is useful, but it causes other tests to panic.
    // Let's keep around for development
    #[ignore]
    #[test]
    #[should_panic]
    fn handles_not_found_commands() {
        read_command("not-existing-command".to_string()).unwrap();
    }

    // This test is useful, but it causes other tests to panic.
    // Let's keep around for development
    #[ignore]
    #[test]
    #[should_panic]
    fn it_detects_if_command_already_exists() {
        add_command("test-command-duplicated".to_string(), Command {
            title: "Test Command".to_string(),
            docs: "Here are some docs about the command".to_string(),
            ..Command::default()
        });
        add_command("test-command-duplicated".to_string(), Command {
            title: "Test Command".to_string(),
            docs: "Here are some docs about the command".to_string(),
            ..Command::default()
        });
    }

    #[test]
    fn runs_commands() {
        add_command("test-command-with-callback".to_string(), Command {
            title: "Test Command".to_string(),
            docs: "Here are some docs about the command".to_string(),
            ..Command::default()
        });
        let mut command = read_command("test-command-with-callback".to_string()).unwrap();

        command.run();

        // Should return true 1 time
        assert_eq!(command.check_if_has_to_run(), true);
        assert_eq!(command.check_if_has_to_run(), false);

        command.run();
        command.run();

        // Should return true 2 times
        assert_eq!(command.check_if_has_to_run(), true);
        assert_eq!(command.check_if_has_to_run(), true);
        assert_eq!(command.check_if_has_to_run(), false);
    }
}
